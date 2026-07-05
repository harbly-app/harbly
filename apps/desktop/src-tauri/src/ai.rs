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

use crate::commands::cur_lang;
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

/// Fallbacks when the session pins nothing (verified against provider docs
/// 2026-07; the per-session picker mirrors these).
fn default_model(provider: ByokProvider) -> &'static str {
    match provider {
        ByokProvider::Anthropic => "claude-sonnet-5",
        ByokProvider::OpenAi => "gpt-5.5",
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
/// cap, so it is safe to call every time the panel or settings open. The
/// results refresh the send-path cache in [`AppState::agent_cache`].
#[tauri::command]
pub async fn ai_detect_agents(app: AppHandle) -> Vec<AgentInfo> {
    let (claude, codex) = tokio::join!(
        harbly_ai::detect_agent(AgentKind::ClaudeCode),
        harbly_ai::detect_agent(AgentKind::Codex),
    );
    let found: Vec<AgentInfo> = [claude, codex].into_iter().flatten().collect();
    let state = app.state::<AppState>();
    let mut cache = state.agent_cache.lock().unwrap();
    cache.clear();
    for info in &found {
        cache.insert(info.kind.clone(), info.clone());
    }
    found
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

/// Delete a conversation, keeping its snapshot for the undo toast — sessions
/// follow the product's undo-over-confirm rule like file deletion does.
#[tauri::command]
pub async fn ai_session_delete(app: AppHandle, id: String) -> Result<(), String> {
    // A streaming turn holds this transcript open: cancel it and wait for the
    // busy mark to clear, so the snapshot below is complete and nothing
    // appends to (or records runs against) a session that no longer exists.
    let flag = {
        let state = app.state::<AppState>();
        let busy = state.ai_busy.lock().unwrap();
        busy.get(&id).cloned()
    };
    if let Some(flag) = flag {
        flag.cancel();
        let mut cleared = false;
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            let state = app.state::<AppState>();
            if !state.ai_busy.lock().unwrap().contains_key(&id) {
                cleared = true;
                break;
            }
        }
        if !cleared {
            return Err("该会话已有正在进行的回合".to_string());
        }
    }
    let state = app.state::<AppState>();
    let snap = state
        .lib()?
        .delete_ai_session(&id)
        .map_err(|e| e.to_string())?;
    if snap.is_some() {
        *state.ai_deleted_session.lock().unwrap() = snap;
    }
    Ok(())
}

/// Undo the most recent session deletion. Returns the restored session id so
/// the panel can re-select it (None when there is nothing to restore).
#[tauri::command]
pub async fn ai_session_restore(app: AppHandle) -> Result<Option<String>, String> {
    let state = app.state::<AppState>();
    let snap = state.ai_deleted_session.lock().unwrap().take();
    let Some(snap) = snap else { return Ok(None) };
    let restored_id = snap.session.id.clone();
    let outcome = state
        .lib()
        .and_then(|lib| lib.restore_ai_session(&snap).map_err(|e| e.to_string()));
    if let Err(e) = outcome {
        // Keep Undo retryable: put the snapshot back — unless a newer
        // deletion claimed the slot while we were failing.
        let mut slot = state.ai_deleted_session.lock().unwrap();
        if slot.is_none() {
            *slot = Some(snap);
        }
        return Err(e);
    }
    Ok(Some(restored_id))
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
        AiError::StepLimit => "已达到单回合步数上限".to_string(),
        AiError::Http(d) => format!("网络错误: {d}"),
        AiError::Provider(d) => format!("AI 服务错误: {d}"),
        AiError::Agent(d) => format!("本地 agent 出错: {d}"),
        AiError::Io(d) => format!("IO 错误: {d}"),
    }
}

/// The claude CLI's "resume id no longer exists" failure — the only agent
/// error worth an automatic fresh-run retry.
fn is_stale_resume(result: &Result<harbly_ai::TurnOutput, AiError>) -> bool {
    matches!(result, Err(AiError::Agent(m)) if m.to_ascii_lowercase().contains("no conversation found"))
}

/// Removes the job's cancel flag and the session's busy mark even on early
/// returns/panics.
struct JobGuard {
    app: AppHandle,
    job: String,
    session_id: String,
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        let state = self.app.state::<AppState>();
        state.ai_jobs.lock().unwrap().remove(&self.job);
        state.ai_busy.lock().unwrap().remove(&self.session_id);
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
        if let Some(o) = outcome {
            *self.wrote.lock().unwrap() = true;
            // Just the touched asset — a full missing-thumb scan per write
            // would stat the whole library on every call of a multi-write
            // turn. Deletions (ver 0) need no thumbnail.
            if o.ver > 0 {
                crate::commands::enqueue_thumb_for(&self.app, &o.asset_id);
            }
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

async fn resolve_supply(
    app: &AppHandle,
    lib: &Library,
    session: &harbly_core::AiSession,
) -> Result<Supply, String> {
    if let Some(kind) = AgentKind::from_id(&session.supply) {
        // Cache-first: a fresh detect_agent spawns `--version` (hundreds of
        // ms) — pure latency on EVERY send. The panel/settings probe fills
        // the cache; a stale path just fails the actual run with a clearer
        // error than "not found".
        let cached = {
            let state = app.state::<AppState>();
            let cache = state.agent_cache.lock().unwrap();
            cache.get(kind.id()).cloned()
        };
        let info = match cached {
            Some(info) => info,
            None => {
                let detected = harbly_ai::detect_agent(kind)
                    .await
                    .ok_or_else(|| "未找到本地 agent".to_string())?;
                let state = app.state::<AppState>();
                state
                    .agent_cache
                    .lock()
                    .unwrap()
                    .insert(kind.id().to_string(), detected.clone());
                detected
            }
        };
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

    // Backend single-flight per session: two interleaved turns would corrupt
    // one transcript and cross-link run rows. The cancel flag registers in
    // the same breath so Stop already works while the supply resolves.
    let cancel = CancelFlag::new();
    {
        let state = app.state::<AppState>();
        let mut busy = state.ai_busy.lock().unwrap();
        if busy.contains_key(&session_id) {
            return Err("该会话已有正在进行的回合".to_string());
        }
        busy.insert(session_id.clone(), cancel.clone());
        state
            .ai_jobs
            .lock()
            .unwrap()
            .insert(job.clone(), cancel.clone());
    }
    let _guard = JobGuard {
        app: app.clone(),
        job,
        session_id: session_id.clone(),
    };

    let supply = resolve_supply(&app, &lib, &session).await?;

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
    // replays faithfully after a restart. The event counter feeds the
    // stale-resume check: zero events ⇒ nothing streamed AND no tool ran.
    let actions: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let actions2 = actions.clone();
    let events_seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let events2 = events_seen.clone();
    let mut sink = move |ev: harbly_ai::AiEvent| {
        events2.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let harbly_ai::AiEvent::Action { label } = &ev {
            actions2.lock().unwrap().push(label.clone());
        }
        let _ = on_event.send(ev);
    };

    let resume = session.agent_session_id.as_deref();
    let mut result =
        harbly_ai::run_turn(&task, &supply, &executor, resume, cancel.clone(), &mut sink).await;
    // The claude CLI garbage-collects old sessions; a dead resume id would
    // otherwise brick this conversation on every send. Drop the id and rerun
    // fresh once — without a resume id the engine replays the transcript.
    // The wording match is only a fast path: CLI copy moves around (observed:
    // the detail hides in the result event's `errors` array), so the
    // load-bearing signal is "agent failed before ANY event" — no events
    // means no tool ran, making a fresh rerun side-effect-free.
    let resumed_dead = resume.is_some()
        && matches!(&result, Err(AiError::Agent(_)))
        && (is_stale_resume(&result)
            || events_seen.load(std::sync::atomic::Ordering::Relaxed) == 0);
    if resumed_dead {
        let _ = lib.clear_ai_session_agent_id(&session_id);
        result =
            harbly_ai::run_turn(&task, &supply, &executor, None, cancel.clone(), &mut sink).await;
    }

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
