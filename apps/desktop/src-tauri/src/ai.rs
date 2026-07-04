//! AI commands: the glue between the harbly-ai engine and the library.
//!
//! Contract with the frontend: `ai_run` returns Err only for pre-flight
//! failures (no key, agent not installed, unreadable asset) — nothing is
//! recorded and a toast is enough. Once the engine actually runs, the outcome
//! (ok / error / cancelled) is persisted as an ai_run row and returned as
//! Ok(record), so the timeline and the run history always agree.

use crate::commands::{cur_lang, enqueue_missing_thumbs};
use crate::state::AppState;
use harbly_ai::{
    AgentInfo, AgentKind, AiError, AiTask, ByokProvider, CancelFlag, Supply, TaskKind,
};
use serde_json::json;
use std::collections::HashMap;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager};

/// Keychain coordinates: one entry per BYOK provider, never written to disk.
const KEYRING_SERVICE: &str = "Harbly";

/// Version-chain label for AI rewrites (canonical Chinese, like every other
/// label the core writes; the frontend localizes known labels for display).
const AI_VERSION_LABEL: &str = "AI 改版";

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

/// Non-secret AI preferences (chosen supply, per-provider model names), stored
/// in the app config.json next to language/library.
#[tauri::command]
pub fn ai_get_config(app: AppHandle) -> serde_json::Value {
    crate::commands::read_config_value(&app, "ai").unwrap_or_else(|| json!({}))
}

#[tauri::command]
pub fn ai_set_config(app: AppHandle, config: serde_json::Value) {
    crate::commands::write_config_key(&app, "ai", config);
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

// ---------- Running ----------

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
        AiError::ContentTooLarge => "文件过大，无法交给 AI 处理".to_string(),
        AiError::Http(d) => format!("网络错误: {d}"),
        AiError::Provider(d) => format!("AI 服务错误: {d}"),
        AiError::Agent(d) => format!("本地 agent 出错: {d}"),
        AiError::NoFileInReply => "AI 回复中没有可用的文件内容".to_string(),
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

async fn resolve_supply(supply: &str, model: Option<String>) -> Result<Supply, String> {
    if let Some(kind) = AgentKind::from_id(supply) {
        let info = harbly_ai::detect_agent(kind)
            .await
            .ok_or_else(|| "未找到本地 agent".to_string())?;
        return Ok(Supply::Agent {
            kind,
            program: info.path,
        });
    }
    let provider = ByokProvider::from_id(supply).ok_or_else(|| "未知的 AI 供给".to_string())?;
    let api_key = read_key(provider.id()).ok_or_else(|| "未配置 API Key".to_string())?;
    let model = model
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| default_model(provider).to_string());
    Ok(Supply::Byok {
        provider,
        api_key,
        model,
    })
}

#[tauri::command]
#[expect(
    clippy::too_many_arguments,
    reason = "tauri IPC maps one arg per payload field; a params struct would just mirror them"
)]
pub async fn ai_run(
    app: AppHandle,
    job: String,
    id: String,
    kind: String,
    instruction: String,
    supply: String,
    model: Option<String>,
    on_event: Channel<harbly_ai::AiEvent>,
) -> Result<harbly_core::AiRunRecord, String> {
    let lib = app.state::<AppState>().lib()?;
    let asset = lib.asset(&id).map_err(|e| e.to_string())?;
    let content = lib.read_asset_text(&id).map_err(|e| e.to_string())?;

    let task_kind = match kind.as_str() {
        "revise" => TaskKind::Revise,
        "review" => TaskKind::Review,
        _ => return Err("未知的 AI 任务类型".to_string()),
    };
    let task = AiTask {
        kind: task_kind,
        instruction: instruction.clone(),
        file_name: asset.file_name.clone(),
        content,
        is_markdown: matches!(
            asset.file_name.rsplit('.').next().map(|e| e.to_ascii_lowercase()),
            Some(ref e) if e == "md" || e == "markdown"
        ),
        title: asset.title.clone(),
        reply_lang: cur_lang(&app),
    };
    let resolved = resolve_supply(&supply, model.clone()).await?;

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

    let mut sink = move |ev: harbly_ai::AiEvent| {
        let _ = on_event.send(ev);
    };
    let result = harbly_ai::run_task(&task, &resolved, cancel, &mut sink).await;

    let mut new = harbly_core::NewAiRun {
        asset_id: id.clone(),
        kind: kind.clone(),
        supply: supply.clone(),
        model: match &resolved {
            Supply::Byok { model, .. } => model.clone(),
            Supply::Agent { .. } => String::new(),
        },
        instruction,
        status: "ok".into(),
        ver: None,
        report: None,
        error: None,
    };

    match result {
        Ok(out) => {
            if let Some(report) = out.report {
                new.report = Some(report);
            }
            if let Some(text) = out.new_content {
                let ver = lib
                    .apply_ai_output(&id, &text, AI_VERSION_LABEL)
                    .map_err(|e| e.to_string())?;
                new.ver = Some(ver);
                enqueue_missing_thumbs(&app);
                let _ = app.emit("library-changed", ());
            }
        }
        Err(AiError::Cancelled) => {
            new.status = "cancelled".into();
        }
        Err(e) => {
            new.status = "error".into();
            new.error = Some(err_message(&e));
        }
    }
    lib.record_ai_run(&new).map_err(|e| e.to_string())
}
