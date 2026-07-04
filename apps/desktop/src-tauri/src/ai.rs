//! AI commands: sessions, turns, credentials — the glue between the harbly-ai
//! engine and the library.
//!
//! One turn = `ai_send`: the user text is appended to the session, the engine
//! runs it on the session's supply (streaming progress through a Channel),
//! and the assistant reply is appended and returned. All library access from
//! models goes through the shared tool surface (`Library::execute_ai_tool`):
//! BYOK loops call it in-process via [`AppExecutor`], Claude Code calls it
//! through the spawned harbly-mcp server. Writes surface as attributed
//! versions either way; a failed turn returns Err and leaves no assistant
//! message (the user message stays, ready to retry).

use crate::commands::{cur_lang, enqueue_missing_thumbs};
use crate::state::AppState;
use harbly_ai::{
    AgentInfo, AgentKind, AiError, AssetRef, ByokProvider, CancelFlag, ChatTurn, Role, SessionTask,
    Supply, ToolExecutor,
};
use harbly_core::{AiToolCtx, Library};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager};

/// Keychain coordinates: one entry per BYOK provider, never written to disk.
const KEYRING_SERVICE: &str = "Harbly";

/// Turns of context replayed to BYOK supplies (agents resume natively).
const HISTORY_TURNS: usize = 30;

pub const BYOK_PROVIDERS: [&str; 3] = ["anthropic", "openai", "openrouter"];

fn default_model(provider: ByokProvider) -> &'static str {
    match provider {
        ByokProvider::Anthropic => "claude-sonnet-5",
        ByokProvider::OpenAi => "gpt-5.1",
        ByokProvider::OpenRouter => "anthropic/claude-sonnet-5",
    }
}

fn key_entry(provider: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, &format!("byok-{provider}")).map_err(|e| e.to_string())
}

fn read_key(provider: &str) -> Option<String> {
    key_entry(provider).ok()?.get_password().ok()
}

// ---------- Configuration & credentials ----------

/// Detect installed agent CLIs (claude / codex). Runs `--version` with a short
/// cap, so it is safe to call every time the panel or settings open.
#[tauri::command]
pub async fn ai_detect_agents() -> Vec<AgentInfo> {
    let (claude, codex) = tokio::join!(
        harbly_ai::detect_agent(AgentKind::ClaudeCode),
        harbly_ai::detect_agent(AgentKind::Codex),
    );
    [claude, codex].into_iter().flatten().collect()
}

#[tauri::command]
pub fn ai_key_status() -> HashMap<String, bool> {
    BYOK_PROVIDERS
        .iter()
        .map(|p| (p.to_string(), read_key(p).is_some()))
        .collect()
}

/// Store (or clear, when empty) a BYOK key in the OS keychain.
#[tauri::command]
pub fn ai_set_key(provider: String, key: String) -> Result<(), String> {
    if !BYOK_PROVIDERS.contains(&provider.as_str()) {
        return Err("未知的 AI 供给".to_string());
    }
    let entry = key_entry(&provider)?;
    let key = key.trim();
    if key.is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    } else {
        entry.set_password(key).map_err(|e| e.to_string())
    }
}

/// Non-secret AI preferences (default supply, per-provider model names),
/// stored in the app config.json next to language/library.
#[tauri::command]
pub fn ai_get_config(app: AppHandle) -> serde_json::Value {
    crate::commands::read_config_value(&app, "ai").unwrap_or_else(|| json!({}))
}

#[tauri::command]
pub fn ai_set_config(app: AppHandle, config: serde_json::Value) {
    crate::commands::write_config_key(&app, "ai", config);
}

// ---------- Sessions ----------

#[tauri::command]
pub async fn ai_sessions_list(app: AppHandle) -> Result<Vec<harbly_core::AiSession>, String> {
    app.state::<AppState>()
        .lib()?
        .list_ai_sessions(100)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ai_session_create(
    app: AppHandle,
    supply: String,
    model: String,
    effort: String,
) -> Result<harbly_core::AiSession, String> {
    app.state::<AppState>()
        .lib()?
        .create_ai_session(&supply, &model, &effort)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ai_session_delete(app: AppHandle, id: String) -> Result<(), String> {
    app.state::<AppState>()
        .lib()?
        .delete_ai_session(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ai_session_set_prefs(
    app: AppHandle,
    id: String,
    supply: String,
    model: String,
    effort: String,
) -> Result<(), String> {
    app.state::<AppState>()
        .lib()?
        .set_ai_session_prefs(&id, &supply, &model, &effort)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ai_session_messages(
    app: AppHandle,
    id: String,
) -> Result<Vec<harbly_core::AiMessage>, String> {
    app.state::<AppState>()
        .lib()?
        .list_ai_messages(&id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ai_runs_list(
    app: AppHandle,
    id: String,
    limit: Option<i64>,
) -> Result<Vec<harbly_core::AiRunRecord>, String> {
    app.state::<AppState>()
        .lib()?
        .list_ai_runs(&id, limit.unwrap_or(50))
        .map_err(|e| e.to_string())
}

// ---------- Running a turn ----------

#[tauri::command]
pub async fn ai_cancel(app: AppHandle, job: String) {
    let flag = {
        let state = app.state::<AppState>();
        let jobs = state.ai_jobs.lock().unwrap();
        jobs.get(&job).cloned()
    };
    if let Some(flag) = flag {
        flag.cancel();
    }
}

/// Canonical (Chinese) message per engine error; dynamic detail keeps a fixed
/// prefix so the frontend can localize by prefix match.
fn err_message(e: &AiError) -> String {
    match e {
        AiError::Cancelled => "已取消".to_string(),
        AiError::Timeout => "AI 请求超时".to_string(),
        AiError::Http(d) => format!("网络错误: {d}"),
        AiError::Provider(d) => format!("AI 服务错误: {d}"),
        AiError::Agent(d) => format!("本地 agent 出错: {d}"),
        AiError::Io(d) => format!("IO 错误: {d}"),
    }
}

/// Removes the job's cancel flag even on early returns/panics.
struct JobGuard {
    app: AppHandle,
    job: String,
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        let state = self.app.state::<AppState>();
        state.ai_jobs.lock().unwrap().remove(&self.job);
    }
}

/// The in-process tool executor: BYOK loops and codex write-back run through
/// it. Every write refreshes the UI immediately (the version is already on
/// disk — the user sees the preview flip while the model keeps talking).
struct AppExecutor {
    app: AppHandle,
    lib: Arc<Library>,
    ctx: AiToolCtx,
    wrote: Mutex<bool>,
}

impl ToolExecutor for AppExecutor {
    fn execute(&self, name: &str, args: &serde_json::Value) -> Result<serde_json::Value, String> {
        let (value, outcome) = self.lib.execute_ai_tool(name, args, &self.ctx)?;
        if outcome.is_some() {
            *self.wrote.lock().unwrap() = true;
            enqueue_missing_thumbs(&self.app);
            let _ = self.app.emit("library-changed", ());
        }
        Ok(value)
    }
}

/// The harbly-mcp binary ships next to the app binary (dev: same target dir;
/// bundle: sidecar).
fn mcp_server_path() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or_else(|| "no exe dir".to_string())?;
    let p = dir.join("harbly-mcp");
    if p.is_file() {
        Ok(p)
    } else {
        Err("未找到 harbly-mcp".to_string())
    }
}

async fn resolve_supply(lib: &Library, session: &harbly_core::AiSession) -> Result<Supply, String> {
    if let Some(kind) = AgentKind::from_id(&session.supply) {
        let info = harbly_ai::detect_agent(kind)
            .await
            .ok_or_else(|| "未找到本地 agent".to_string())?;
        let workdir = lib.root().join(".harbly").join("ai-workspace");
        let mcp_config_json = if kind == AgentKind::ClaudeCode {
            let server = mcp_server_path()?;
            Some(
                json!({
                    "mcpServers": {
                        "harbly": {
                            "command": server,
                            "args": [
                                "--library", lib.root(),
                                "--session", session.id,
                                "--supply", session.supply,
                            ],
                        }
                    }
                })
                .to_string(),
            )
        } else {
            None
        };
        return Ok(Supply::Agent {
            kind,
            program: info.path,
            model: Some(session.model.clone()).filter(|m| !m.trim().is_empty()),
            workdir,
            mcp_config_json,
        });
    }
    let provider =
        ByokProvider::from_id(&session.supply).ok_or_else(|| "未知的 AI 供给".to_string())?;
    let api_key = read_key(provider.id()).ok_or_else(|| "未配置 API Key".to_string())?;
    let model = Some(session.model.clone())
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| default_model(provider).to_string());
    Ok(Supply::Byok {
        provider,
        api_key,
        model,
    })
}

#[tauri::command]
pub async fn ai_send(
    app: AppHandle,
    job: String,
    session_id: String,
    text: String,
    current_asset_id: Option<String>,
    on_event: Channel<harbly_ai::AiEvent>,
) -> Result<harbly_core::AiMessage, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("指令不能为空".to_string());
    }
    let lib = app.state::<AppState>().lib()?;
    let session = lib
        .get_ai_session(&session_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "会话不存在".to_string())?;
    let supply = resolve_supply(&lib, &session).await?;

    // Context BEFORE this turn's user message lands in the transcript
    let history: Vec<ChatTurn> = lib
        .list_ai_messages(&session_id)
        .map_err(|e| e.to_string())?
        .iter()
        .rev()
        .take(HISTORY_TURNS)
        .rev()
        .map(|m| ChatTurn {
            role: if m.role == "assistant" {
                Role::Assistant
            } else {
                Role::User
            },
            text: m.content.clone(),
        })
        .collect();

    let current_asset = match &current_asset_id {
        Some(id) => lib.asset(id).ok().map(|a| AssetRef {
            id: a.id,
            file_name: a.file_name,
            title: a.title,
        }),
        None => None,
    };

    let turn_start = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    lib.append_ai_message(&session_id, "user", &text, &[])
        .map_err(|e| e.to_string())?;

    let task = SessionTask {
        instruction: text,
        history,
        current_asset,
        reply_lang: cur_lang(&app),
        effort: session.effort.clone(),
    };

    let cancel = CancelFlag::new();
    {
        let state = app.state::<AppState>();
        state
            .ai_jobs
            .lock()
            .unwrap()
            .insert(job.clone(), cancel.clone());
    }
    let _guard = JobGuard {
        app: app.clone(),
        job,
    };

    let executor = AppExecutor {
        app: app.clone(),
        lib: lib.clone(),
        ctx: AiToolCtx {
            supply: session.supply.clone(),
            model: session.model.clone(),
            session_id: Some(session_id.clone()),
        },
        wrote: Mutex::new(false),
    };

    // Persist tool-activity labels alongside the reply so the transcript
    // replays faithfully after a restart.
    let actions: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let actions2 = actions.clone();
    let mut sink = move |ev: harbly_ai::AiEvent| {
        if let harbly_ai::AiEvent::Action { label } = &ev {
            actions2.lock().unwrap().push(label.clone());
        }
        let _ = on_event.send(ev);
    };

    let resume = session.agent_session_id.as_deref();
    let result = harbly_ai::run_turn(&task, &supply, &executor, resume, cancel, &mut sink).await;

    match result {
        Ok(out) => {
            if let Some(agent_id) = &out.agent_session_id {
                let _ = lib.set_ai_session_agent_id(&session_id, agent_id);
            }
            let actions = actions.lock().unwrap().clone();
            let msg = lib
                .append_ai_message(&session_id, "assistant", &out.reply, &actions)
                .map_err(|e| e.to_string())?;
            let _ = lib.link_runs_to_message(&session_id, &msg.id, turn_start);
            // Claude's writes happen in the MCP server process; one refresh
            // after the turn covers them (in-process writes already emitted).
            if !*executor.wrote.lock().unwrap() {
                let _ = app.emit("library-changed", ());
            }
            Ok(msg)
        }
        Err(e) => Err(err_message(&e)),
    }
}
