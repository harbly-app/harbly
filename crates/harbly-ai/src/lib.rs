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

/// What the caller wants done with the asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    /// Rewrite the file; the engine yields the complete new content.
    Revise,
    /// Produce a report; the file is never modified.
    Review,
}

#[derive(Debug, Clone)]
pub struct AiTask {
    pub kind: TaskKind,
    /// User instruction (revise) or extra focus areas (review, may be empty).
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
    /// Revise: complete new file content; `None` when the model changed nothing.
    pub new_content: Option<String>,
    /// Review: the report text.
    pub report: Option<String>,
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
    #[error("no file content in reply")]
    NoFileInReply,
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

/// System prompt for BYOK revise: the reply must carry the whole file in one
/// fence so extraction is mechanical. Four backticks so Markdown files with
/// embedded ``` blocks nest safely.
pub(crate) fn revise_system(task: &AiTask) -> String {
    format!(
        "You are the revision engine of Harbly, a local manager for single-file {kind} assets. \
         Rewrite the given file according to the user's instruction.\n\
         Rules:\n\
         - Reply with the COMPLETE revised file wrapped in ONE fence of exactly four backticks (````{tag} … ````).\n\
         - Before the fence, write a single summary line of what changed, in {lang}. Nothing after the fence.\n\
         - Preserve everything the instruction does not ask to change.\n\
         - Keep the file self-contained; do not introduce external network resources unless the instruction asks.",
        kind = if task.is_markdown { "Markdown" } else { "HTML" },
        tag = file_lang_tag(task),
        lang = task.reply_lang,
    )
}

pub(crate) fn review_system(task: &AiTask) -> String {
    format!(
        "You are the reviewer of Harbly, a local manager for single-file {kind} assets. \
         Produce a concise, actionable review of the given file covering: \
         security (scripts, external requests, data collection), usability and accessibility, \
         copy quality, and a short prioritized fix list. \
         Use compact markdown with short sections. Respond entirely in {lang}.",
        kind = if task.is_markdown { "Markdown" } else { "HTML" },
        lang = task.reply_lang,
    )
}

/// User message for BYOK: metadata header + fenced current content (+ optional
/// instruction). Shared by revise and review.
pub(crate) fn user_message(task: &AiTask) -> String {
    let mut msg = format!(
        "File name: {}\nAsset title: {}\n\nCurrent file content:\n`````{}\n{}\n`````\n",
        task.file_name,
        task.title,
        file_lang_tag(task),
        task.content,
    );
    match task.kind {
        TaskKind::Revise => {
            msg.push_str("\nInstruction: ");
            msg.push_str(&task.instruction);
        }
        TaskKind::Review => {
            if !task.instruction.trim().is_empty() {
                msg.push_str("\nExtra focus: ");
                msg.push_str(&task.instruction);
            }
        }
    }
    msg
}

/// Pull the revised file out of an assistant reply: the content of the largest
/// backtick fence (3+ backticks, closing fence must be at least as long), or
/// the whole reply when it plainly IS an HTML document.
pub(crate) fn extract_file_from_reply(reply: &str, is_markdown: bool) -> Option<String> {
    let mut best: Option<String> = None;
    let mut lines = reply.lines().peekable();
    while let Some(line) = lines.next() {
        let t = line.trim_start();
        let fence_len = t.chars().take_while(|c| *c == '`').count();
        if fence_len < 3 {
            continue;
        }
        let mut body = String::new();
        for inner in lines.by_ref() {
            let it = inner.trim_start();
            let close = it.chars().take_while(|c| *c == '`').count();
            if close >= fence_len && it.chars().all(|c| c == '`' || c.is_whitespace()) {
                break;
            }
            body.push_str(inner);
            body.push('\n');
        }
        if best.as_ref().map(|b| body.len() > b.len()).unwrap_or(true) {
            best = Some(body);
        }
    }
    if let Some(b) = best {
        let trimmed = b.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string() + "\n");
        }
    }
    // No usable fence: accept a bare reply only when it is unmistakably the file
    let t = reply.trim();
    let lower = t.to_ascii_lowercase();
    if !is_markdown && (lower.starts_with("<!doctype") || lower.starts_with("<html")) {
        return Some(t.to_string() + "\n");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(kind: TaskKind) -> AiTask {
        AiTask {
            kind,
            instruction: "make it dark".into(),
            file_name: "pricing.html".into(),
            content: "<!doctype html><html><body>hi</body></html>".into(),
            is_markdown: false,
            title: "Pricing".into(),
            reply_lang: "zh-CN".into(),
        }
    }

    #[test]
    fn extracts_largest_fence() {
        let reply = "Changed the theme.\n````html\n<!doctype html>\n<html>big</html>\n````\nAlso:\n```\ntiny\n```";
        let got = extract_file_from_reply(reply, false).unwrap();
        assert!(got.contains("<html>big</html>"));
        assert!(!got.contains("tiny"));
    }

    #[test]
    fn extracts_nested_markdown_fences() {
        let reply = "Summary.\n````markdown\n# Title\n```js\nconsole.log(1)\n```\ntail\n````\n";
        let got = extract_file_from_reply(reply, true).unwrap();
        assert!(got.contains("```js"));
        assert!(got.trim_end().ends_with("tail"));
    }

    #[test]
    fn accepts_bare_html_reply() {
        let got = extract_file_from_reply("<!DOCTYPE html><html>x</html>", false).unwrap();
        assert!(got.to_lowercase().starts_with("<!doctype"));
    }

    #[test]
    fn rejects_prose_reply() {
        assert!(extract_file_from_reply("I cannot do that.", false).is_none());
        assert!(extract_file_from_reply("Some prose about markdown.", true).is_none());
    }

    #[test]
    fn oversized_content_is_rejected() {
        let mut t = task(TaskKind::Revise);
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
    fn prompts_mention_language_and_fence() {
        let t = task(TaskKind::Revise);
        let sys = revise_system(&t);
        assert!(sys.contains("zh-CN"));
        assert!(sys.contains("````html"));
        let usr = user_message(&t);
        assert!(usr.contains("pricing.html"));
        assert!(usr.contains("Instruction: make it dark"));
    }
}
