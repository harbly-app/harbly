//! Local agent supplies.
//!
//! Claude Code runs headless with the Harbly MCP server attached and ONLY
//! those tools allowed — it reads and writes assets through the same
//! versioned surface as everyone else, never through raw file access. Its own
//! session id is captured so the next turn resumes with full context.
//!
//! Codex has no MCP wiring here yet: it gets a scratch copy of the current
//! asset, edits it with its own sandboxed tools, and the observed diff is
//! written back through the caller's executor. Conversation context is
//! replayed in the prompt.

use crate::tools::call_label;
use crate::{
    history_block, system_prompt, system_prompt_core, AgentKind, AiError, AiEvent, CancelFlag,
    EventSink, SessionTask, Supply, ToolExecutor, TurnOutput,
};
use serde_json::Value;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;

const AGENT_TIMEOUT: Duration = Duration::from_secs(900);
/// Poll interval while waiting for output — lets cancellation land quickly
/// even when the agent is silent.
const POLL: Duration = Duration::from_millis(400);
/// Grace period after the agent closes its output stream: a well-behaved CLI
/// exits at once, so a process still alive past this has wedged and gets killed
/// (its stream is already complete, so the parsed reply is kept).
const EXIT_GRACE: Duration = Duration::from_secs(5);
/// Cap on replayed conversation context for fresh (non-resumed) agent runs.
const HISTORY_CHARS: usize = 6_000;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    /// "claude" | "codex"
    pub kind: String,
    pub path: String,
    pub version: Option<String>,
}

/// GUI apps launch with a minimal PATH (`/usr/bin:/bin:…`), while agent CLIs
/// live in Homebrew/npm/bun install dirs — so detection and the child PATH
/// both consider these explicitly.
fn extra_bin_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join(".claude/local"));
        dirs.push(home.join(".bun/bin"));
        dirs.push(home.join(".cargo/bin"));
        dirs.push(home.join(".npm-global/bin"));
        // Node version managers put npm globals behind a stable "default" alias;
        // the per-shell shim dirs (fnm_multishells etc.) die with the shell, so
        // a GUI-launched app must look here instead.
        dirs.push(home.join(".local/share/fnm/aliases/default/bin"));
        dirs.push(home.join(".volta/bin"));
    }
    dirs
}

fn search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    for d in extra_bin_dirs() {
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    dirs
}

/// PATH for the child process: current PATH + the well-known bin dirs (npm
/// shim CLIs need node, which may not be on a GUI app's PATH).
fn child_path_env() -> std::ffi::OsString {
    std::env::join_paths(search_dirs()).unwrap_or_else(|_| "/usr/bin:/bin".into())
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.is_file()
            && std::fs::metadata(p)
                .map(|m| m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// Locate an agent CLI and confirm it answers `--version` (3s cap). Returns
/// `None` when not installed — the UI shows the supply as unavailable.
pub async fn detect_agent(kind: AgentKind) -> Option<AgentInfo> {
    let name = kind.id();
    let path = search_dirs()
        .into_iter()
        .map(|d| d.join(name))
        .find(|p| is_executable(p))?;
    let out = tokio::time::timeout(
        Duration::from_secs(3),
        Command::new(&path)
            .arg("--version")
            .env("PATH", child_path_env())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !out.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Some(AgentInfo {
        kind: name.to_string(),
        path: path.to_string_lossy().to_string(),
        version,
    })
}

fn agent_fields(supply: &Supply) -> (&str, Option<&str>, &Path, Option<&str>) {
    match supply {
        Supply::Agent {
            program,
            model,
            workdir,
            mcp_config_json,
            ..
        } => (
            program.as_str(),
            model.as_deref(),
            workdir.as_path(),
            mcp_config_json.as_deref(),
        ),
        Supply::Byok { .. } => unreachable!("agent runner called with a BYOK supply"),
    }
}

// ---------- Claude Code (MCP mode) ----------

pub(crate) async fn run_claude_turn(
    task: &SessionTask,
    supply: &Supply,
    resume: Option<&str>,
    cancel: CancelFlag,
    on_event: EventSink<'_>,
) -> Result<TurnOutput, AiError> {
    let (program, model, workdir, mcp_config) = agent_fields(supply);
    std::fs::create_dir_all(workdir)?;

    // The MCP config is REQUIRED: the tool sandbox (allowed/disallowed lists
    // below) is what confines a headless run to the library, and it only
    // makes sense alongside the harbly server. Running without it would fall
    // back to the user's global permission rules — the raw file/exec leak
    // this mode was built to prevent.
    let Some(mcp_json) = mcp_config else {
        return Err(AiError::Agent("missing MCP config for claude".into()));
    };
    // The config goes through a temp file: robust across CLI versions
    // (inline-JSON support varies) and keeps the command line short.
    let mut config_file = tempfile::Builder::new()
        .prefix("harbly-mcp-")
        .suffix(".json")
        .tempfile()?;
    config_file.write_all(mcp_json.as_bytes())?;
    config_file.flush()?;
    let config_path = config_file.path().to_path_buf();

    // With a resume id claude carries its own context; otherwise replay a
    // compact transcript so a fresh CLI session still knows the conversation.
    let mut prompt = String::new();
    if resume.is_none() {
        prompt.push_str(&history_block(&task.history, HISTORY_CHARS));
    }
    prompt.push_str(&task.instruction);

    let mut cmd = Command::new(program);
    cmd.arg("-p")
        .arg(&prompt)
        .args(["--output-format", "stream-json", "--verbose"])
        // Token-level deltas: without this the CLI only emits whole assistant
        // messages and the panel shows each reply popping in at once.
        .arg("--include-partial-messages")
        .args(["--max-turns", "40"])
        .args(["--append-system-prompt", &system_prompt(task)]);
    cmd.arg("--mcp-config").arg(&config_path);
    // Pre-approve ONLY the Harbly tools (both server- and tool-level
    // patterns, for CLI-version tolerance)...
    cmd.args(["--allowedTools", "mcp__harbly,mcp__harbly__*"]);
    // ...and explicitly forbid raw file/exec access: the user's own
    // global Claude Code permission rules would otherwise leak into this
    // headless run (observed: Bash find escaping the library). Web and
    // subagent tools are closed too — a library-only run has no business
    // browsing, and Task would launder the other bans through a subagent.
    cmd.args([
        "--disallowedTools",
        "Bash,Read,Edit,Write,MultiEdit,NotebookEdit,Glob,Grep,WebFetch,WebSearch,Task",
    ]);
    if let Some(id) = resume {
        cmd.args(["--resume", id]);
    }
    if let Some(m) = model {
        cmd.args(["--model", m]);
    }
    // Claude Code's session effort knob accepts low|medium|high|xhigh|max;
    // anything else (legacy empty, foreign values) is simply not passed.
    if crate::is_anthropic_effort(&task.effort) {
        cmd.args(["--effort", &task.effort]);
    }
    cmd.current_dir(workdir);

    let parsed = run_agent_process(cmd, AgentKind::ClaudeCode, cancel, on_event).await?;
    let reply = parsed
        .final_text
        .clone()
        .unwrap_or_else(|| parsed.assistant_text());
    if reply.trim().is_empty() {
        return Err(AiError::Agent("empty reply".into()));
    }
    Ok(TurnOutput {
        reply,
        agent_session_id: parsed.session_id,
    })
}

// ---------- Codex (scratch-copy mode) ----------

pub(crate) async fn run_codex_turn(
    task: &SessionTask,
    supply: &Supply,
    executor: &dyn ToolExecutor,
    cancel: CancelFlag,
    on_event: EventSink<'_>,
) -> Result<TurnOutput, AiError> {
    let (program, model, _workdir, _mcp) = agent_fields(supply);
    let scratch = tempfile::Builder::new().prefix("harbly-ai-").tempdir()?;

    // Materialize the current asset (if any) into the scratch dir via the
    // executor — codex has no library tools, so this copy is its whole world.
    let mut file_ctx: Option<(String, PathBuf, String)> = None;
    if let Some(a) = &task.current_asset {
        let read = executor
            .execute(crate::tools::READ, &serde_json::json!({ "asset_id": a.id }))
            .map_err(AiError::Agent)?;
        let content = read["content"].as_str().unwrap_or_default().to_string();
        let path = scratch.path().join(&a.file_name);
        std::fs::write(&path, &content)?;
        file_ctx = Some((a.id.clone(), path, content));
    }

    // Core prompt only: codex has no library tools (it edits the scratch copy
    // staged below), so it must not be told about tools it cannot call.
    let mut prompt = system_prompt_core(task);
    prompt.push_str("\n\n");
    prompt.push_str(&history_block(&task.history, HISTORY_CHARS));
    if let Some(a) = &task.current_asset {
        prompt.push_str(&format!(
            "A working copy of \"{}\" is in the current directory. If the instruction asks for \
             changes, edit that file in place (only that file; keep it self-contained; no new \
             files, no package managers or network commands). Otherwise do not modify anything \
             and answer directly.\n\n",
            a.file_name
        ));
    }
    prompt.push_str("Instruction: ");
    prompt.push_str(&task.instruction);

    let mut cmd = Command::new(program);
    cmd.args(["exec", "--json", "--full-auto", "--skip-git-repo-check"]);
    if let Some(m) = model {
        cmd.args(["-m", m]);
    }
    if !task.effort.is_empty() {
        cmd.args(["-c", &format!("model_reasoning_effort={}", task.effort)]);
    }
    cmd.arg(&prompt).current_dir(scratch.path());

    let parsed = run_agent_process(cmd, AgentKind::Codex, cancel, on_event).await?;
    let reply = parsed
        .final_text
        .clone()
        .unwrap_or_else(|| parsed.assistant_text());

    // Outcome by observation: a changed scratch copy is written back through
    // the executor — the same versioned path every other supply uses.
    if let Some((asset_id, path, before)) = file_ctx {
        let after = std::fs::read_to_string(&path)?;
        if after != before {
            let summary: String = task.instruction.chars().take(80).collect();
            let args = serde_json::json!({
                "asset_id": asset_id, "content": after, "summary": summary,
            });
            on_event(AiEvent::Action {
                label: call_label(crate::tools::WRITE, &args),
            });
            executor
                .execute(crate::tools::WRITE, &args)
                .map_err(AiError::Agent)?;
        }
    }
    if reply.trim().is_empty() {
        // Surface silence as a failure — echoing the user's instruction back
        // as a fake assistant reply would mask the agent dying quietly. Any
        // write-back above already landed as a version, so nothing is lost.
        return Err(AiError::Agent("empty reply".into()));
    }
    Ok(TurnOutput {
        reply,
        agent_session_id: None,
    })
}

// ---------- Shared process driver ----------

struct ParsedRun {
    texts: Vec<String>,
    final_text: Option<String>,
    session_id: Option<String>,
}

impl ParsedRun {
    fn assistant_text(&self) -> String {
        self.texts.join("\n")
    }
}

async fn run_agent_process(
    mut cmd: Command,
    kind: AgentKind,
    cancel: CancelFlag,
    on_event: EventSink<'_>,
) -> Result<ParsedRun, AiError> {
    cmd.env("PATH", child_path_env())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // Own process group so a teardown reaches every subprocess the CLI spawned,
    // not just the CLI itself (agent CLIs routinely fork helpers).
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd
        .spawn()
        .map_err(|e| AiError::Agent(format!("spawn failed: {e}")))?;

    // Drain stderr concurrently so a chatty CLI can't deadlock on a full pipe;
    // keep the tail for error reporting.
    let stderr_tail = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    if let Some(mut se) = child.stderr.take() {
        let tail = stderr_tail.clone();
        tokio::spawn(async move {
            let mut buf = String::new();
            let _ = se.read_to_string(&mut buf).await;
            let mut start = buf.len().saturating_sub(2000);
            // Byte offset may land inside a multi-byte char (CJK stderr)
            while !buf.is_char_boundary(start) {
                start += 1;
            }
            *tail.lock().unwrap() = buf[start..].to_string();
        });
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AiError::Agent("no stdout".into()))?;
    let mut lines = BufReader::new(stdout).lines();

    let started = Instant::now();
    let mut parser = StreamParser::new(kind);
    loop {
        if cancel.is_cancelled() {
            kill_group(&mut child);
            return Err(AiError::Cancelled);
        }
        if started.elapsed() > AGENT_TIMEOUT {
            kill_group(&mut child);
            return Err(AiError::Timeout);
        }
        match tokio::time::timeout(POLL, lines.next_line()).await {
            Err(_) => continue,
            Ok(Err(e)) => {
                kill_group(&mut child);
                return Err(AiError::Agent(e.to_string()));
            }
            Ok(Ok(None)) => break,
            Ok(Ok(Some(line))) => {
                for ev in parser.feed(&line) {
                    on_event(ev);
                }
                if let Some(err) = parser.error.take() {
                    kill_group(&mut child);
                    return Err(AiError::Agent(err));
                }
            }
        }
    }

    // stdout hit EOF, so the stream is complete and the reply is already parsed.
    // Bound the wait for the process to actually exit: a CLI that closed its
    // pipe but never exits (or a lingering grandchild) must not wedge the
    // session "busy" forever, and cancellation must still land here.
    let grace = Instant::now() + EXIT_GRACE;
    let status = loop {
        if cancel.is_cancelled() {
            kill_group(&mut child);
            return Err(AiError::Cancelled);
        }
        match tokio::time::timeout(POLL, child.wait()).await {
            Ok(Ok(status)) => break Some(status),
            Ok(Err(e)) => {
                kill_group(&mut child);
                return Err(AiError::Agent(e.to_string()));
            }
            // Still alive after the grace window: tear it (and any subprocess)
            // down and keep the reply the completed stream already yielded.
            Err(_) if Instant::now() >= grace => {
                kill_group(&mut child);
                break None;
            }
            Err(_) => {}
        }
    };
    if let Some(status) = status {
        if !status.success() {
            let tail = stderr_tail.lock().unwrap().clone();
            let tail = tail.trim();
            return Err(AiError::Agent(if tail.is_empty() {
                format!("exit code {}", status.code().unwrap_or(-1))
            } else {
                tail.chars().take(300).collect()
            }));
        }
    }

    Ok(ParsedRun {
        texts: parser.texts,
        final_text: parser.final_text,
        session_id: parser.session_id,
    })
}

/// Kill the agent and every subprocess it spawned. The child leads its own
/// process group (see `process_group(0)` at spawn), so signalling the negated
/// pid reaches the whole tree; `start_kill` then lets tokio reap the direct
/// child. On non-unix, only the direct child can be killed.
fn kill_group(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // SAFETY: a bare kill(2); an already-reaped pid just yields ESRCH.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    let _ = child.start_kill();
}

/// Tolerant JSONL reader for both CLIs (and both generations of the codex
/// event schema). Unknown lines are ignored rather than fatal — agent CLIs
/// add event types faster than we ship releases.
struct StreamParser {
    kind: AgentKind,
    texts: Vec<String>,
    final_text: Option<String>,
    session_id: Option<String>,
    error: Option<String>,
    /// Token deltas streamed since the last complete message — that message
    /// repeats the same text and must not re-emit it.
    streamed_partial: bool,
}

impl StreamParser {
    fn new(kind: AgentKind) -> Self {
        Self {
            kind,
            texts: Vec::new(),
            final_text: None,
            session_id: None,
            error: None,
            streamed_partial: false,
        }
    }

    fn feed(&mut self, line: &str) -> Vec<AiEvent> {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
            return vec![];
        };
        match self.kind {
            AgentKind::ClaudeCode => self.feed_claude(&v),
            AgentKind::Codex => self.feed_codex(&v),
        }
    }

    fn push_text(&mut self, t: &str) -> Option<AiEvent> {
        if t.trim().is_empty() {
            return None;
        }
        self.texts.push(t.to_string());
        Some(AiEvent::Delta {
            text: t.to_string(),
        })
    }

    /// Record text for the transcript fallback without emitting a Delta —
    /// used when the same text already streamed as partial deltas.
    fn record_text(&mut self, t: &str) {
        if !t.trim().is_empty() {
            self.texts.push(t.to_string());
        }
    }

    fn feed_claude(&mut self, v: &Value) -> Vec<AiEvent> {
        let mut out = vec![];
        if let Some(id) = v["session_id"].as_str() {
            self.session_id = Some(id.to_string());
        }
        match v["type"].as_str() {
            // --include-partial-messages wraps the raw API stream: token
            // deltas arrive here, ahead of the complete assistant message.
            Some("stream_event") => {
                let ev = &v["event"];
                if ev["type"] == "content_block_delta" && ev["delta"]["type"] == "text_delta" {
                    if let Some(t) = ev["delta"]["text"].as_str() {
                        if !t.is_empty() {
                            self.streamed_partial = true;
                            out.push(AiEvent::Delta {
                                text: t.to_string(),
                            });
                        }
                    }
                }
            }
            Some("assistant") => {
                // The complete message repeats text that already streamed as
                // deltas — record it for the transcript, don't re-emit it
                // (that would make the reply pop in twice).
                let quiet = std::mem::take(&mut self.streamed_partial);
                if let Some(blocks) = v["message"]["content"].as_array() {
                    for b in blocks {
                        match b["type"].as_str() {
                            Some("text") => {
                                let text = b["text"].as_str().unwrap_or("");
                                if quiet {
                                    self.record_text(text);
                                } else if let Some(ev) = self.push_text(text) {
                                    out.push(ev);
                                }
                            }
                            Some("tool_use") => {
                                out.push(AiEvent::Action {
                                    label: tool_label(
                                        b["name"].as_str().unwrap_or("tool"),
                                        &b["input"],
                                    ),
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
            Some("result") => {
                if v["subtype"] == "success" {
                    self.final_text = v["result"].as_str().map(String::from);
                } else if let Some(sub) = v["subtype"].as_str() {
                    // The CLI splits detail across fields: `result` (string,
                    // sometimes absent) and an `errors` array — e.g. a stale
                    // --resume id reports subtype "error_during_execution"
                    // with the actual "No conversation found …" only inside
                    // `errors`. Fold everything in so callers can match on it.
                    let mut msg = v["result"].as_str().unwrap_or(sub).to_string();
                    if let Some(errs) = v["errors"].as_array() {
                        let details: Vec<String> = errs
                            .iter()
                            .map(|e| {
                                e.as_str()
                                    .map(String::from)
                                    .unwrap_or_else(|| e.to_string())
                            })
                            .collect();
                        if !details.is_empty() {
                            msg.push_str(": ");
                            msg.push_str(&details.join("; "));
                        }
                    }
                    self.error = Some(msg);
                }
            }
            _ => {}
        }
        out
    }

    fn feed_codex(&mut self, v: &Value) -> Vec<AiEvent> {
        let mut out = vec![];
        // Legacy schema: {"msg":{"type":…}}
        let msg = &v["msg"];
        if let Some(t) = msg["type"].as_str() {
            match t {
                "agent_message_delta" => {
                    if let Some(d) = msg["delta"].as_str() {
                        if !d.is_empty() {
                            self.streamed_partial = true;
                            out.push(AiEvent::Delta {
                                text: d.to_string(),
                            });
                        }
                    }
                }
                "agent_message" => {
                    let quiet = std::mem::take(&mut self.streamed_partial);
                    if let Some(s) = msg["message"].as_str() {
                        if quiet {
                            self.record_text(s);
                        } else if let Some(ev) = self.push_text(s) {
                            out.push(ev);
                        }
                    }
                }
                "exec_command_begin" => out.push(AiEvent::Action {
                    label: command_label(&msg["command"]),
                }),
                "patch_apply_begin" => out.push(AiEvent::Action {
                    label: "apply patch".into(),
                }),
                "task_complete" => {
                    self.final_text = msg["last_agent_message"].as_str().map(String::from);
                }
                "error" => {
                    self.error = Some(msg["message"].as_str().unwrap_or("agent error").into());
                }
                _ => {}
            }
            return out;
        }
        // Item schema: {"type":"item.completed","item":{…}}
        let item = &v["item"];
        if let Some(t) = item["type"].as_str().or_else(|| item["item_type"].as_str()) {
            match t {
                "agent_message" => {
                    if let Some(ev) = item["text"].as_str().and_then(|s| self.push_text(s)) {
                        out.push(ev);
                    }
                }
                "command_execution" => {
                    // started only — completed repeats the same item, and the
                    // persisted transcript would show every command twice
                    // (mirrors the legacy schema's exec_command_begin).
                    if v["type"] == "item.started" {
                        out.push(AiEvent::Action {
                            label: command_label(&item["command"]),
                        });
                    }
                }
                _ => {}
            }
            return out;
        }
        if v["type"] == "error" {
            self.error = Some(v["message"].as_str().unwrap_or("agent error").into());
        }
        out
    }
}

/// Harbly MCP calls render through the shared label builder so agent and BYOK
/// activity read identically; claude's built-in tools keep their own style.
fn tool_label(name: &str, input: &Value) -> String {
    if name.starts_with("mcp__harbly__") {
        return call_label(name, input);
    }
    let target = input["file_path"]
        .as_str()
        .map(|p| {
            p.rsplit(['/', '\\'])
                .next()
                .map(String::from)
                .unwrap_or_else(|| p.to_string())
        })
        .or_else(|| input["command"].as_str().map(|c| truncate(c, 48)))
        .or_else(|| input["pattern"].as_str().map(|p| truncate(p, 32)));
    match target {
        Some(t) => format!("{name} {t}"),
        None => name.to_string(),
    }
}

fn command_label(cmd: &Value) -> String {
    let joined = match cmd {
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        Value::String(s) => s.clone(),
        _ => "command".to_string(),
    };
    truncate(&joined, 60)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::task;
    use crate::ToolExecutor;

    #[test]
    fn claude_stream_captures_session_and_mcp_labels() {
        let mut p = StreamParser::new(AgentKind::ClaudeCode);
        p.feed(r#"{"type":"system","subtype":"init","session_id":"sess-42"}"#);
        assert_eq!(p.session_id.as_deref(), Some("sess-42"));
        let evs = p.feed(
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"mcp__harbly__read_asset","input":{"asset_id":"abcd1234-x"}}]}}"#,
        );
        assert!(matches!(&evs[0], AiEvent::Action { label } if label == "read_asset abcd1234"));
        p.feed(r#"{"type":"result","subtype":"success","result":"完成。","session_id":"sess-42"}"#);
        assert_eq!(p.final_text.as_deref(), Some("完成。"));
        assert!(p.error.is_none());
    }

    #[test]
    fn claude_error_result() {
        let mut p = StreamParser::new(AgentKind::ClaudeCode);
        p.feed(r#"{"type":"result","subtype":"error_max_turns"}"#);
        assert_eq!(p.error.as_deref(), Some("error_max_turns"));

        // The CLI hides detail in the `errors` array (observed on a stale
        // --resume id) — it must reach the error text callers match on
        let mut p = StreamParser::new(AgentKind::ClaudeCode);
        p.feed(
            r#"{"type":"result","subtype":"error_during_execution","errors":["No conversation found with session ID: abc"]}"#,
        );
        let err = p.error.as_deref().unwrap();
        assert!(err.contains("error_during_execution"));
        assert!(err.contains("No conversation found"));
    }

    #[test]
    fn claude_partial_deltas_stream_without_duplicate_on_complete_message() {
        let mut p = StreamParser::new(AgentKind::ClaudeCode);
        let d1 = p.feed(
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"你好"}}}"#,
        );
        assert!(matches!(&d1[0], AiEvent::Delta { text } if text == "你好"));
        let d2 = p.feed(
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"，世界"}}}"#,
        );
        assert_eq!(d2.len(), 1);
        // The complete message repeats the streamed text: record for the
        // transcript fallback, but never re-emit (double pop-in)
        let full = p.feed(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"你好，世界"}]}}"#,
        );
        assert!(full.is_empty());
        assert_eq!(p.texts, vec!["你好，世界".to_string()]);
        // A message with no preceding partials (older CLI) still emits whole
        let plain = p.feed(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"第二条"}]}}"#,
        );
        assert_eq!(plain.len(), 1);
    }

    #[test]
    fn codex_legacy_deltas_stream_without_duplicate() {
        let mut p = StreamParser::new(AgentKind::Codex);
        let d = p.feed(r#"{"id":"1","msg":{"type":"agent_message_delta","delta":"改好"}}"#);
        assert!(matches!(&d[0], AiEvent::Delta { text } if text == "改好"));
        let full = p.feed(r#"{"id":"1","msg":{"type":"agent_message","message":"改好了。"}}"#);
        assert!(full.is_empty());
        assert_eq!(p.texts, vec!["改好了。".to_string()]);
    }

    #[test]
    fn codex_item_schema_emits_one_action_per_command() {
        let mut p = StreamParser::new(AgentKind::Codex);
        let started = p.feed(
            r#"{"type":"item.started","item":{"type":"command_execution","command":"ls -la"}}"#,
        );
        assert_eq!(started.len(), 1);
        // completed repeats the same item — persisting it would show every
        // command twice in the reloaded transcript
        let completed = p.feed(
            r#"{"type":"item.completed","item":{"type":"command_execution","command":"ls -la"}}"#,
        );
        assert!(completed.is_empty());
    }

    #[test]
    fn codex_both_schemas_parse() {
        let mut p = StreamParser::new(AgentKind::Codex);
        let evs = p.feed(
            r#"{"id":"1","msg":{"type":"exec_command_begin","command":["bash","-lc","ls"]}}"#,
        );
        assert!(matches!(&evs[0], AiEvent::Action { label } if label == "bash -lc ls"));
        p.feed(r#"{"id":"3","msg":{"type":"task_complete","last_agent_message":"done"}}"#);
        assert_eq!(p.final_text.as_deref(), Some("done"));

        let mut p = StreamParser::new(AgentKind::Codex);
        let evs =
            p.feed(r#"{"type":"item.completed","item":{"type":"agent_message","text":"hi"}}"#);
        assert!(matches!(&evs[0], AiEvent::Delta { text } if text == "hi"));
    }

    #[test]
    fn garbage_lines_ignored() {
        let mut p = StreamParser::new(AgentKind::ClaudeCode);
        assert!(p.feed("not json at all").is_empty());
        assert!(p.error.is_none());
    }

    struct StubExec {
        content: String,
        writes: std::sync::Mutex<Vec<Value>>,
    }

    impl ToolExecutor for StubExec {
        fn execute(&self, name: &str, args: &Value) -> Result<Value, String> {
            match name {
                crate::tools::READ => Ok(serde_json::json!({
                    "id": args["asset_id"], "fileName": "a.html", "content": self.content,
                })),
                crate::tools::WRITE => {
                    self.writes.lock().unwrap().push(args.clone());
                    Ok(serde_json::json!({ "ver": 2 }))
                }
                _ => Err("unexpected tool".into()),
            }
        }
    }

    fn stub_supply(dir: &Path, script: &str) -> (PathBuf, Supply) {
        let bin = dir.join("codex");
        std::fs::write(&bin, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let supply = Supply::Agent {
            kind: AgentKind::Codex,
            program: bin.to_string_lossy().to_string(),
            model: None,
            workdir: dir.to_path_buf(),
            mcp_config_json: None,
        };
        (bin, supply)
    }

    #[tokio::test]
    async fn codex_scratch_edit_writes_back_through_executor() {
        let dir = tempfile::tempdir().unwrap();
        // Stub agent: rewrites the scratch copy and reports via legacy JSONL
        let (_bin, supply) = stub_supply(
            dir.path(),
            "#!/bin/sh\nprintf '<html>dark</html>' > a.html\necho '{\"msg\":{\"type\":\"task_complete\",\"last_agent_message\":\"改好了\"}}'\n",
        );
        let mut t = task();
        t.current_asset = Some(crate::AssetRef {
            id: "a1".into(),
            file_name: "a.html".into(),
            title: "A".into(),
        });
        let exec = StubExec {
            content: "<html>light</html>".into(),
            writes: std::sync::Mutex::new(vec![]),
        };
        let mut events = vec![];
        let out = run_codex_turn(&t, &supply, &exec, CancelFlag::new(), &mut |e| {
            events.push(e)
        })
        .await
        .unwrap();
        assert_eq!(out.reply, "改好了");
        let writes = exec.writes.lock().unwrap();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0]["asset_id"], "a1");
        assert_eq!(writes[0]["content"], "<html>dark</html>");
    }

    #[tokio::test]
    async fn codex_question_leaves_file_alone() {
        let dir = tempfile::tempdir().unwrap();
        let (_bin, supply) = stub_supply(
            dir.path(),
            "#!/bin/sh\necho '{\"msg\":{\"type\":\"agent_message\",\"message\":\"这个页面没有外部请求。\"}}'\n",
        );
        let mut t = task();
        t.current_asset = Some(crate::AssetRef {
            id: "a1".into(),
            file_name: "a.html".into(),
            title: "A".into(),
        });
        let exec = StubExec {
            content: "<html>light</html>".into(),
            writes: std::sync::Mutex::new(vec![]),
        };
        let out = run_codex_turn(&t, &supply, &exec, CancelFlag::new(), &mut |_| {})
            .await
            .unwrap();
        assert!(out.reply.contains("没有外部请求"));
        assert!(exec.writes.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancel_kills_a_hanging_agent() {
        let dir = tempfile::tempdir().unwrap();
        let (_bin, supply) = stub_supply(dir.path(), "#!/bin/sh\nsleep 60\n");
        let t = task();
        let exec = StubExec {
            content: String::new(),
            writes: std::sync::Mutex::new(vec![]),
        };
        let cancel = CancelFlag::new();
        let c2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            c2.cancel();
        });
        let started = Instant::now();
        let mut bare = t.clone();
        bare.current_asset = None;
        let err = run_codex_turn(&bare, &supply, &exec, cancel, &mut |_| {})
            .await
            .unwrap_err();
        assert!(matches!(err, AiError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn hang_after_output_is_killed_and_reply_kept() {
        let dir = tempfile::tempdir().unwrap();
        // Stub: report completion, close stdout, then linger far past the grace
        // window. The post-EOF wait must not block on the lingering process —
        // the reply is already in hand — so the turn returns promptly.
        let (_bin, supply) = stub_supply(
            dir.path(),
            "#!/bin/sh\necho '{\"msg\":{\"type\":\"task_complete\",\"last_agent_message\":\"改好了\"}}'\nexec 1>&-\nsleep 60\n",
        );
        let exec = StubExec {
            content: String::new(),
            writes: std::sync::Mutex::new(vec![]),
        };
        let mut bare = task();
        bare.current_asset = None;
        let started = Instant::now();
        let out = run_codex_turn(&bare, &supply, &exec, CancelFlag::new(), &mut |_| {})
            .await
            .unwrap();
        assert_eq!(out.reply, "改好了");
        // Killed after the grace window, nowhere near the stub's 60s sleep.
        assert!(started.elapsed() < Duration::from_secs(20));
    }

    #[tokio::test]
    async fn claude_turn_resumes_and_returns_session_id() {
        // Stub claude: prints the args file so the test can assert on flags,
        // then a result event carrying a session id.
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("claude");
        std::fs::write(
            &bin,
            "#!/bin/sh\necho \"$@\" > args.txt\necho '{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"ok\",\"session_id\":\"s-9\"}'\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let supply = Supply::Agent {
            kind: AgentKind::ClaudeCode,
            program: bin.to_string_lossy().to_string(),
            model: Some("sonnet".into()),
            workdir: dir.path().to_path_buf(),
            mcp_config_json: Some(r#"{"mcpServers":{"harbly":{"command":"x"}}}"#.into()),
        };
        let mut t = task();
        t.effort = "high".into();
        let out = run_claude_turn(&t, &supply, Some("prev-1"), CancelFlag::new(), &mut |_| {})
            .await
            .unwrap();
        assert_eq!(out.reply, "ok");
        assert_eq!(out.agent_session_id.as_deref(), Some("s-9"));
        let args = std::fs::read_to_string(dir.path().join("args.txt")).unwrap();
        assert!(args.contains("--resume prev-1"));
        assert!(args.contains("--mcp-config"));
        assert!(args.contains("--allowedTools mcp__harbly,mcp__harbly__*"));
        assert!(args.contains("--model sonnet"));
        // Effort reaches Claude Code as its session --effort flag
        assert!(args.contains("--effort high"));
    }
}
