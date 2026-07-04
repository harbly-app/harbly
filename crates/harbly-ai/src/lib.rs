//! AI supply engine: turns one asset + one instruction into a unified event
//! stream and a final output, regardless of where the model runs.
//!
//! Two supply families share the same surface:
//! - BYOK: direct streaming HTTP to Anthropic / OpenAI / OpenRouter;
//! - Local agent: a spawned `claude` / `codex` CLI editing a scratch copy of
//!   the file, so the agent never touches the library directly.
//!
//! The crate is deliberately standalone (no harbly-core dependency): callers
//! feed it file content and write the results back through their own APIs,
//! which keeps the write/permission boundary outside the engine.

mod agent;
mod byok;
mod sse;

pub use agent::{detect_agent, AgentInfo};

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Reject absurdly large inputs before building prompts: single-file artifacts
/// are typically tens of KB; anything past this would blow context windows.
pub const MAX_CONTENT_BYTES: usize = 400 * 1024;

/// The sentinel fence tag: a reply replaces the file if and only if it wraps
/// the complete new content in ````harbly-file … ````. An ordinary code block
/// quoted inside a prose answer can never be mistaken for a file replacement.
pub const FILE_FENCE_TAG: &str = "harbly-file";

/// One instruction against one file. There is deliberately NO task-kind field:
/// intent (change vs. question/review) is routed by the model and classified
/// afterwards by outcome — the file changed, or it didn't. The version chain
/// is the safety net for misfires.
#[derive(Debug, Clone)]
pub struct AiTask {
    pub instruction: String,
    pub file_name: String,
    pub content: String,
    pub is_markdown: bool,
    pub title: String,
    /// BCP-47 UI language; prose replies and reports come back in it.
    pub reply_lang: String,
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
    },
}

/// One incremental step of a running task, in supply-agnostic form. BYOK only
/// ever emits `Delta`; agents add `Action` for tool activity.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AiEvent {
    Delta { text: String },
    Action { label: String },
}

#[derive(Debug, Clone, Default)]
pub struct TaskOutput {
    /// Complete new file content; `None` when the run changed nothing.
    pub new_content: Option<String>,
    /// The textual reply (answer / review / "nothing to change" note). Set
    /// exactly when `new_content` is `None` — outcomes are one or the other.
    pub reply: Option<String>,
    /// Everything the assistant said (kept for run records / debugging).
    pub assistant_text: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("cancelled")]
    Cancelled,
    #[error("timeout")]
    Timeout,
    #[error("content too large")]
    ContentTooLarge,
    #[error("http: {0}")]
    Http(String),
    #[error("provider: {0}")]
    Provider(String),
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

/// Run one task to completion on the given supply, streaming progress into
/// `on_event`. Timeouts: BYOK 5 min, agent 15 min (agents legitimately take
/// long on big rewrites).
pub async fn run_task(
    task: &AiTask,
    supply: &Supply,
    cancel: CancelFlag,
    on_event: EventSink<'_>,
) -> Result<TaskOutput, AiError> {
    if task.content.len() > MAX_CONTENT_BYTES {
        return Err(AiError::ContentTooLarge);
    }
    match supply {
        Supply::Byok {
            provider,
            api_key,
            model,
        } => byok::run(task, *provider, api_key, model, cancel, on_event).await,
        Supply::Agent { kind, program } => agent::run(task, *kind, program, cancel, on_event).await,
    }
}

pub(crate) fn file_lang_tag(task: &AiTask) -> &'static str {
    if task.is_markdown {
        "markdown"
    } else {
        "html"
    }
}

/// The single BYOK system prompt: change requests come back as a complete file
/// inside the sentinel fence; everything else comes back as prose. Four
/// backticks so Markdown files with embedded ``` blocks nest safely.
pub(crate) fn unified_system(task: &AiTask) -> String {
    format!(
        "You are the AI workbench of Harbly, a local manager for single-file {kind} assets. \
         The user gives one instruction about one file; it may ask you to change the file, \
         or ask a question / request a review of it.\n\
         If it asks for CHANGES:\n\
         - Reply with a single summary line in {lang}, then the COMPLETE revised file wrapped \
           in one fence opened by exactly ````{tag} and closed by ```` (four backticks). \
           Nothing after the closing fence.\n\
         - Preserve everything the instruction does not ask to change; keep the file \
           self-contained; do not introduce external network resources unless asked.\n\
         Otherwise: answer directly and concisely in markdown, in {lang}. \
         Never use the {tag} fence unless you are replacing the file.",
        kind = if task.is_markdown { "Markdown" } else { "HTML" },
        tag = FILE_FENCE_TAG,
        lang = task.reply_lang,
    )
}

/// User message for BYOK: metadata header + fenced current content + instruction.
pub(crate) fn user_message(task: &AiTask) -> String {
    format!(
        "File name: {}\nAsset title: {}\n\nCurrent file content:\n`````{}\n{}\n`````\n\nInstruction: {}",
        task.file_name,
        task.title,
        file_lang_tag(task),
        task.content,
        task.instruction,
    )
}

/// Pull a file replacement out of an assistant reply. Only two shapes count:
/// the sentinel fence (````harbly-file … ````), or a bare reply that IS an
/// HTML document. Ordinary ``` code blocks in a prose answer never match, so
/// an answer can quote snippets without being misread as a rewrite. An
/// unclosed sentinel fence is rejected (truncated stream must not half-apply).
pub(crate) fn extract_file_from_reply(reply: &str) -> Option<String> {
    let mut lines = reply.lines();
    while let Some(line) = lines.next() {
        let t = line.trim();
        let fence_len = t.chars().take_while(|c| *c == '`').count();
        if fence_len < 3 || t[fence_len..].trim() != FILE_FENCE_TAG {
            continue;
        }
        let mut body = String::new();
        for inner in lines.by_ref() {
            let it = inner.trim();
            let close = it.chars().take_while(|c| *c == '`').count();
            if close >= fence_len && it.chars().all(|c| c == '`') {
                let b = body.trim();
                return (!b.is_empty()).then(|| b.to_string() + "\n");
            }
            body.push_str(inner);
            body.push('\n');
        }
        return None;
    }
    let t = reply.trim();
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("<!doctype") || lower.starts_with("<html") {
        return Some(t.to_string() + "\n");
    }
    None
}

/// The prose part of a reply that also carried a sentinel fence — used as the
/// textual reply when the fenced content turned out identical to the original.
pub(crate) fn prose_before_fence(reply: &str) -> String {
    let head: Vec<&str> = reply
        .lines()
        .take_while(|line| {
            let t = line.trim();
            let n = t.chars().take_while(|c| *c == '`').count();
            n < 3 || t[n..].trim() != FILE_FENCE_TAG
        })
        .collect();
    let head = head.join("\n").trim().to_string();
    if head.is_empty() {
        reply.trim().to_string()
    } else {
        head
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> AiTask {
        AiTask {
            instruction: "make it dark".into(),
            file_name: "pricing.html".into(),
            content: "<!doctype html><html><body>hi</body></html>".into(),
            is_markdown: false,
            title: "Pricing".into(),
            reply_lang: "zh-CN".into(),
        }
    }

    #[test]
    fn extracts_sentinel_fence() {
        let reply = "改成了深色主题。\n````harbly-file\n<!doctype html>\n<html>dark</html>\n````\n";
        let got = extract_file_from_reply(reply).unwrap();
        assert!(got.contains("<html>dark</html>"));
        assert!(!got.contains("改成了"));
    }

    #[test]
    fn sentinel_nests_markdown_code_blocks() {
        let reply = "Summary.\n````harbly-file\n# Title\n```js\nconsole.log(1)\n```\ntail\n````\n";
        let got = extract_file_from_reply(reply).unwrap();
        assert!(got.contains("```js"));
        assert!(got.trim_end().ends_with("tail"));
    }

    // The load-bearing safety property of the unified mode: a prose answer that
    // quotes ordinary code blocks must never be misread as a file replacement.
    #[test]
    fn plain_code_blocks_are_not_replacements() {
        let reply = "问题出在这段脚本：\n```js\nvar x = document.title;\n```\n建议移除。";
        assert!(extract_file_from_reply(reply).is_none());
        let reply2 = "可以这样写：\n````html\n<html>snippet</html>\n````\n完整替换请再说一声。";
        assert!(extract_file_from_reply(reply2).is_none());
    }

    #[test]
    fn unclosed_sentinel_fence_is_rejected() {
        let reply = "Summary.\n````harbly-file\n<!doctype html><html>half";
        assert!(extract_file_from_reply(reply).is_none());
    }

    #[test]
    fn accepts_bare_html_reply() {
        let got = extract_file_from_reply("<!DOCTYPE html><html>x</html>").unwrap();
        assert!(got.to_lowercase().starts_with("<!doctype"));
    }

    #[test]
    fn rejects_prose_reply() {
        assert!(extract_file_from_reply("I cannot do that.").is_none());
        assert!(extract_file_from_reply("Some prose about markdown.").is_none());
    }

    #[test]
    fn prose_head_survives_identical_rewrite() {
        let reply = "内容已经是深色，无需修改。\n````harbly-file\n<html>same</html>\n````\n";
        assert_eq!(prose_before_fence(reply), "内容已经是深色，无需修改。");
        assert_eq!(prose_before_fence("只有散文。"), "只有散文。");
    }

    #[test]
    fn oversized_content_is_rejected() {
        let mut t = task();
        t.content = "x".repeat(MAX_CONTENT_BYTES + 1);
        let supply = Supply::Byok {
            provider: ByokProvider::Anthropic,
            api_key: "k".into(),
            model: "m".into(),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(run_task(&t, &supply, CancelFlag::new(), &mut |_| {}))
            .unwrap_err();
        assert!(matches!(err, AiError::ContentTooLarge));
    }

    #[test]
    fn unified_prompt_mentions_language_and_sentinel() {
        let t = task();
        let sys = unified_system(&t);
        assert!(sys.contains("zh-CN"));
        assert!(sys.contains("````harbly-file"));
        assert!(sys.contains("Never use"));
        let usr = user_message(&t);
        assert!(usr.contains("pricing.html"));
        assert!(usr.contains("Instruction: make it dark"));
    }
}
