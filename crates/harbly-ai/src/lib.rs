//! AI supply engine: runs one conversation turn against the user's library,
//! streaming progress events and returning the assistant's reply.
//!
//! Every supply gets the same tool surface over the library (search / read /
//! write / create):
//! - BYOK (Anthropic native, OpenAI & OpenRouter via chat-completions) runs a
//!   native streaming tool-use loop, executing tools through the caller's
//!   [`ToolExecutor`];
//! - Claude Code runs headless with the Harbly MCP server attached and ONLY
//!   those tools allowed — the CLI process never touches library files;
//! - Codex (no MCP wiring yet) falls back to a scratch-copy of the current
//!   asset; an observed diff is written back through the same executor.
//!
//! The crate stays standalone (no harbly-core dependency): all library access
//! goes through the executor/MCP boundary, which is where writes become
//! versions. There is no task-kind anywhere — intent is routed by the model,
//! outcomes are whatever the tools record.

mod agent;
mod byok;
mod sse;
pub mod tools;

pub use agent::{detect_agent, AgentInfo};

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

/// One prior turn, text only. Past tool traffic is not replayed — the current
/// state of the library speaks for itself.
#[derive(Debug, Clone)]
pub struct ChatTurn {
    pub role: Role,
    pub text: String,
}

/// The asset the user is currently viewing — attached as context so "this
/// file" needs no name, read only if the model decides it's relevant.
#[derive(Debug, Clone)]
pub struct AssetRef {
    pub id: String,
    pub file_name: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct SessionTask {
    pub instruction: String,
    /// Prior turns, oldest first.
    pub history: Vec<ChatTurn>,
    pub current_asset: Option<AssetRef>,
    /// BCP-47 UI language; replies come back in it.
    pub reply_lang: String,
    /// "" (no knob) or a provider effort token — Anthropic/claude CLI accept
    /// low|medium|high|xhigh|max, OpenAI/codex none|minimal|low|medium|high|xhigh.
    pub effort: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByokProvider {
    Anthropic,
    OpenAi,
    OpenRouter,
}

impl ByokProvider {
    pub fn id(&self) -> &'static str {
        match self {
            ByokProvider::Anthropic => "anthropic",
            ByokProvider::OpenAi => "openai",
            ByokProvider::OpenRouter => "openrouter",
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        match s {
            "anthropic" => Some(ByokProvider::Anthropic),
            "openai" => Some(ByokProvider::OpenAi),
            "openrouter" => Some(ByokProvider::OpenRouter),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    ClaudeCode,
    Codex,
}

impl AgentKind {
    pub fn id(&self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude",
            AgentKind::Codex => "codex",
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        match s {
            "claude" => Some(AgentKind::ClaudeCode),
            "codex" => Some(AgentKind::Codex),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Supply {
    Byok {
        provider: ByokProvider,
        api_key: String,
        model: String,
    },
    Agent {
        kind: AgentKind,
        /// Absolute path to the executable (resolved by the caller's detection).
        program: String,
        /// Optional model override (`claude --model`, `codex -m`).
        model: Option<String>,
        /// Stable working directory for the CLI (sessions resume by cwd).
        workdir: std::path::PathBuf,
        /// MCP server config JSON for CLIs that support it (claude). The
        /// caller builds it — server binary path, library root, attribution.
        mcp_config_json: Option<String>,
    },
}

/// One incremental step of a running turn, in supply-agnostic form.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AiEvent {
    Delta { text: String },
    Action { label: String },
}

/// Executes library tools on behalf of the model. Implemented by the app over
/// harbly-core; the MCP server is the same surface for external CLIs. This is
/// THE write boundary: models never see paths or raw disk.
pub trait ToolExecutor: Send + Sync {
    fn execute(&self, name: &str, args: &serde_json::Value) -> Result<serde_json::Value, String>;
}

#[derive(Debug, Clone, Default)]
pub struct TurnOutput {
    /// The assistant's final text for this turn.
    pub reply: String,
    /// The agent CLI's own session id (claude), for resuming the next turn.
    pub agent_session_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("cancelled")]
    Cancelled,
    #[error("timeout")]
    Timeout,
    #[error("http: {0}")]
    Http(String),
    #[error("provider: {0}")]
    Provider(String),
    /// The per-turn tool budget ran out before the model produced any reply.
    #[error("step limit")]
    StepLimit,
    #[error("agent: {0}")]
    Agent(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Cooperative cancellation: the caller flips the flag, loops observe it
/// between chunks/lines and abort (killing the child process for agents).
#[derive(Debug, Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

pub type EventSink<'a> = &'a mut (dyn FnMut(AiEvent) + Send);

/// Effort → thinking-token budget for the LEGACY Anthropic thinking shape
/// (Haiku 4.5 / Claude ≤4.5 era). Budgets stay below the request max_tokens.
/// "none"/"minimal" map to no thinking at all.
pub(crate) fn thinking_budget(effort: &str) -> Option<u32> {
    match effort {
        "low" => Some(4_000),
        "medium" => Some(10_000),
        "high" => Some(24_000),
        "xhigh" => Some(28_000),
        "max" => Some(31_000),
        _ => None,
    }
}

/// The effort tokens Anthropic's output_config and Claude Code's --effort
/// actually accept.
pub(crate) fn is_anthropic_effort(effort: &str) -> bool {
    matches!(effort, "low" | "medium" | "high" | "xhigh" | "max")
}

/// Run one conversation turn on the given supply. `resume` is the agent CLI's
/// session id from the previous turn (None for the first turn / BYOK).
pub async fn run_turn(
    task: &SessionTask,
    supply: &Supply,
    executor: &dyn ToolExecutor,
    resume: Option<&str>,
    cancel: CancelFlag,
    on_event: EventSink<'_>,
) -> Result<TurnOutput, AiError> {
    match supply {
        Supply::Byok {
            provider,
            api_key,
            model,
        } => byok::run_turn(task, *provider, api_key, model, executor, cancel, on_event).await,
        Supply::Agent { kind, .. } => match kind {
            AgentKind::ClaudeCode => {
                agent::run_claude_turn(task, supply, resume, cancel, on_event).await
            }
            AgentKind::Codex => {
                agent::run_codex_turn(task, supply, executor, cancel, on_event).await
            }
        },
    }
}

/// System prompt for supplies that expose the shared library tool surface
/// (Claude via MCP, BYOK): identity + tool names + tool-use rules + context.
pub(crate) fn system_prompt(task: &SessionTask) -> String {
    build_system_prompt(task, true)
}

/// System prompt for supplies with NO library tools (the Codex CLI works on a
/// scratch copy): the same identity and context, but without advertising tools
/// it cannot call. The caller supplies its own file-handling instructions.
pub(crate) fn system_prompt_core(task: &SessionTask) -> String {
    build_system_prompt(task, false)
}

fn build_system_prompt(task: &SessionTask, tools: bool) -> String {
    let mut s = String::from(
        "You are the AI workbench of Harbly, a local-first manager for single-file HTML and \
         Markdown assets.",
    );
    if tools {
        s.push_str(
            " You operate on the user's library exclusively through tools: search_library \
             (full text), list_assets (enumeration with sizes), read_asset, write_asset, \
             create_asset, delete_asset.",
        );
    }
    s.push_str("\nRules:\n");
    if tools {
        s.push_str(
            "- Read an asset before modifying it. write_asset must carry the COMPLETE new file \
             content; every write becomes a new version the user can inspect and roll back.\n\
             - Write or delete only when the user asks for it; questions and reviews get prose \
             answers. Deletions go to the system Trash (user-recoverable).\n",
        );
    }
    s.push_str(
        "- Keep files self-contained; do not introduce external network resources unless asked.\n",
    );
    if tools {
        s.push_str(
            "- Never invent asset ids — obtain them from search_library/list_assets or the \
             context below.\n",
        );
    }
    s.push_str(&format!("- Respond in {}.", task.reply_lang));
    if let Some(a) = &task.current_asset {
        s.push_str(&format!(
            "\nContext: the user is currently viewing \"{}\" (title: {}, asset_id: {}). \
             When they say \"this file/page/document\", they mean it.",
            a.file_name, a.title, a.id
        ));
    }
    s
}

/// Compact history block for supplies that cannot replay a message array
/// (fresh agent CLI runs without a resume id).
pub(crate) fn history_block(history: &[ChatTurn], max_chars: usize) -> String {
    if history.is_empty() {
        return String::new();
    }
    let mut lines: Vec<String> = Vec::new();
    let mut used = 0usize;
    for turn in history.iter().rev() {
        let who = match turn.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
        };
        let text: String = turn.text.chars().take(600).collect();
        let line = format!("{who}: {text}");
        used += line.chars().count();
        if used > max_chars {
            break;
        }
        lines.push(line);
    }
    lines.reverse();
    format!("Earlier in this conversation:\n{}\n\n", lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn task() -> SessionTask {
        SessionTask {
            instruction: "make it dark".into(),
            history: vec![],
            current_asset: Some(AssetRef {
                id: "a1".into(),
                file_name: "pricing.html".into(),
                title: "Pricing".into(),
            }),
            reply_lang: "zh-CN".into(),
            effort: String::new(),
        }
    }

    #[test]
    fn system_prompt_carries_context_and_language() {
        let t = task();
        let s = system_prompt(&t);
        assert!(s.contains("zh-CN"));
        assert!(s.contains("pricing.html"));
        assert!(s.contains("asset_id: a1"));
        assert!(s.contains("write_asset"));
        let mut bare = t.clone();
        bare.current_asset = None;
        assert!(!system_prompt(&bare).contains("currently viewing"));
    }

    #[test]
    fn core_prompt_omits_tools_but_keeps_context() {
        let s = system_prompt_core(&task());
        // Codex has no library tools, so none may be advertised…
        for tool in [
            "search_library",
            "read_asset",
            "write_asset",
            "create_asset",
            "delete_asset",
        ] {
            assert!(!s.contains(tool), "core prompt leaked tool: {tool}");
        }
        // …but identity, language, and current-asset context still apply.
        assert!(s.contains("Harbly"));
        assert!(s.contains("zh-CN"));
        assert!(s.contains("asset_id: a1"));
    }

    #[test]
    fn history_block_truncates_oldest_first() {
        let history = vec![
            ChatTurn {
                role: Role::User,
                text: "old ".repeat(200),
            },
            ChatTurn {
                role: Role::User,
                text: "改成深色".into(),
            },
            ChatTurn {
                role: Role::Assistant,
                text: "已改成深色。".into(),
            },
        ];
        let block = history_block(&history, 200);
        assert!(block.contains("改成深色"));
        assert!(block.contains("Assistant: 已改成深色。"));
        assert!(!block.contains("old old"));
        assert!(history_block(&[], 200).is_empty());
    }
}
