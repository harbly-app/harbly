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
    history_block, system_prompt, AgentKind, AiError, AiEvent, CancelFlag, EventSink, SessionTask,
    Supply, ToolExecutor, TurnOutput,
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

    // The MCP config goes through a temp file: robust across CLI versions
    // (inline-JSON support varies) and keeps the command line short.
    let mut config_file = tempfile::Builder::new()
        .prefix("harbly-mcp-")
        .suffix(".json")
        .tempfile()?;
    let config_path = match mcp_config {
        Some(json) => {
            config_file.write_all(json.as_bytes())?;
            config_file.flush()?;
            Some(config_file.path().to_path_buf())
        }
        None => None,
    };

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
        .args(["--max-turns", "40"])
        .args(["--append-system-prompt", &system_prompt(task)]);
    if let Some(cfg) = &config_path {
        cmd.arg("--mcp-config").arg(cfg);
        // Pre-approve ONLY the Harbly tools (both server- and tool-level
        // patterns, for CLI-version tolerance). Everything else — raw file
        // tools, Bash — stays unapproved and is denied in print mode.
        cmd.args(["--allowedTools", "mcp__harbly,mcp__harbly__*"]);
    }
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

    let mut prompt = system_prompt(task);
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
    let mut reply = parsed
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
        reply = task.instruction.clone();
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
            let start = buf.len().saturating_sub(2000);
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
            let _ = child.start_kill();
            return Err(AiError::Cancelled);
        }
        if started.elapsed() > AGENT_TIMEOUT {
            let _ = child.start_kill();
            return Err(AiError::Timeout);
        }
        match tokio::time::timeout(POLL, lines.next_line()).await {
            Err(_) => continue,
            Ok(Err(e)) => {
                let _ = child.start_kill();
                return Err(AiError::Agent(e.to_string()));
            }
            Ok(Ok(None)) => break,
            Ok(Ok(Some(line))) => {
                for ev in parser.feed(&line) {
                    on_event(ev);
                }
                if let Some(err) = parser.error.take() {
                    let _ = child.start_kill();
                    return Err(AiError::Agent(err));
                }
            }
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| AiError::Agent(e.to_string()))?;
    if !status.success() {
        let tail = stderr_tail.lock().unwrap().clone();
        let tail = tail.trim();
        return Err(AiError::Agent(if tail.is_empty() {
            format!("exit code {}", status.code().unwrap_or(-1))
        } else {
            tail.chars().take(300).collect()
        }));
    }

    Ok(ParsedRun {
        texts: parser.texts,
        final_text: parser.final_text,
        session_id: parser.session_id,
    })
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
}

impl StreamParser {
    fn new(kind: AgentKind) -> Self {
        Self {
            kind,
            texts: Vec::new(),
            final_text: None,
            session_id: None,
            error: None,
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

    fn feed_claude(&mut self, v: &Value) -> Vec<AiEvent> {
        let mut out = vec![];
        if let Some(id) = v["session_id"].as_str() {
            self.session_id = Some(id.to_string());
        }
        match v["type"].as_str() {
            Some("assistant") => {
                if let Some(blocks) = v["message"]["content"].as_array() {
                    for b in blocks {
                        match b["type"].as_str() {
                            Some("text") => {
                                if let Some(ev) = b["text"].as_str().and_then(|t| self.push_text(t))
                                {
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
                    self.error = Some(
                        v["result"]
                            .as_str()
                            .map(String::from)
                            .unwrap_or_else(|| sub.to_string()),
                    );
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
                "agent_message" => {
                    if let Some(ev) = msg["message"].as_str().and_then(|s| self.push_text(s)) {
                        out.push(ev);
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
                    if v["type"] == "item.started" || v["type"] == "item.completed" {
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
