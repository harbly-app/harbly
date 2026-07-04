//! Local agent supply: spawn a `claude` / `codex` CLI against a scratch copy
//! of the asset. The agent edits the copy in place with its own tools; the
//! engine diffs the copy afterwards, so the library file is never exposed to
//! the agent process. Zero API cost — the user's existing CLI subscription
//! does the work.

use crate::{AgentKind, AiError, AiEvent, AiTask, CancelFlag, EventSink, TaskKind, TaskOutput};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;

const AGENT_TIMEOUT: Duration = Duration::from_secs(900);
/// Poll interval while waiting for output — lets cancellation land quickly
/// even when the agent is silent.
const POLL: Duration = Duration::from_millis(400);

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

fn agent_prompt(task: &AiTask) -> String {
    let kind = if task.is_markdown { "Markdown" } else { "HTML" };
    match task.kind {
        TaskKind::Revise => format!(
            "You are working in a scratch directory containing a single file `{name}` — a \
             self-contained {kind} asset from the user's library (title: {title}).\n\
             Apply this instruction to that file, editing it in place:\n{instruction}\n\n\
             Rules: modify only `{name}`; keep it self-contained; do not create, rename or \
             delete files; do not run package managers or network commands. \
             When finished, reply with one line in {lang} summarizing what changed.",
            name = task.file_name,
            title = task.title,
            instruction = task.instruction,
            lang = task.reply_lang,
        ),
        TaskKind::Review => format!(
            "Read the file `{name}` in the current directory — a self-contained {kind} asset \
             from the user's library (title: {title}). Do not modify anything.\n\
             Produce a concise, actionable review in compact markdown covering: security \
             (scripts, external requests, data collection), usability and accessibility, copy \
             quality, and a short prioritized fix list.{focus}\n\
             Respond entirely in {lang}.",
            name = task.file_name,
            title = task.title,
            focus = if task.instruction.trim().is_empty() {
                String::new()
            } else {
                format!(" Extra focus: {}.", task.instruction)
            },
            lang = task.reply_lang,
        ),
    }
}

fn build_command(task: &AiTask, kind: AgentKind, program: &str, workdir: &Path) -> Command {
    let prompt = agent_prompt(task);
    let mut cmd = Command::new(program);
    match kind {
        AgentKind::ClaudeCode => {
            cmd.arg("-p")
                .arg(&prompt)
                .args(["--output-format", "stream-json", "--verbose"])
                .args(["--max-turns", "40"]);
            // Auto-accept edits only when we actually want the file rewritten;
            // reviews run with default permissions (read-only tools need none).
            if task.kind == TaskKind::Revise {
                cmd.args(["--permission-mode", "acceptEdits"]);
            }
        }
        AgentKind::Codex => {
            cmd.args(["exec", "--json", "--full-auto", "--skip-git-repo-check"])
                .arg(&prompt);
        }
    }
    cmd.current_dir(workdir)
        .env("PATH", child_path_env())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    cmd
}

pub(crate) async fn run(
    task: &AiTask,
    kind: AgentKind,
    program: &str,
    cancel: CancelFlag,
    on_event: EventSink<'_>,
) -> Result<TaskOutput, AiError> {
    let scratch = tempfile::Builder::new().prefix("harbly-ai-").tempdir()?;
    let file_path = scratch.path().join(&task.file_name);
    std::fs::write(&file_path, &task.content)?;

    let mut child = build_command(task, kind, program, scratch.path())
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

    let assistant_text = parser.assistant_text();
    let mut out = TaskOutput {
        assistant_text: assistant_text.clone(),
        ..TaskOutput::default()
    };
    match task.kind {
        TaskKind::Review => {
            let report = parser.final_text.clone().unwrap_or(assistant_text);
            if report.trim().is_empty() {
                return Err(AiError::Agent("empty report".into()));
            }
            out.report = Some(report);
        }
        TaskKind::Revise => {
            let after = std::fs::read_to_string(&file_path)?;
            if after != task.content {
                out.new_content = Some(after);
            }
        }
    }
    Ok(out)
}

/// Tolerant JSONL reader for both CLIs (and both generations of the codex
/// event schema). Unknown lines are ignored rather than fatal — agent CLIs
/// add event types faster than we ship releases.
struct StreamParser {
    kind: AgentKind,
    texts: Vec<String>,
    final_text: Option<String>,
    error: Option<String>,
}

impl StreamParser {
    fn new(kind: AgentKind) -> Self {
        Self {
            kind,
            texts: Vec::new(),
            final_text: None,
            error: None,
        }
    }

    fn assistant_text(&self) -> String {
        self.texts.join("\n")
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

/// "Edit pricing.html" / "Bash: ls -la" — enough for a status line, no more.
fn tool_label(name: &str, input: &Value) -> String {
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

    fn revise_task() -> AiTask {
        AiTask {
            kind: TaskKind::Revise,
            instruction: "dark theme".into(),
            file_name: "a.html".into(),
            content: "<html></html>".into(),
            is_markdown: false,
            title: "A".into(),
            reply_lang: "en".into(),
        }
    }

    #[test]
    fn claude_stream_events() {
        let mut p = StreamParser::new(AgentKind::ClaudeCode);
        assert!(p.feed(r#"{"type":"system","subtype":"init"}"#).is_empty());
        let evs = p.feed(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Working"},{"type":"tool_use","name":"Edit","input":{"file_path":"/tmp/x/a.html"}}]}}"#,
        );
        assert_eq!(evs.len(), 2);
        assert!(matches!(&evs[0], AiEvent::Delta { text } if text == "Working"));
        assert!(matches!(&evs[1], AiEvent::Action { label } if label == "Edit a.html"));
        p.feed(r#"{"type":"result","subtype":"success","result":"Done."}"#);
        assert_eq!(p.final_text.as_deref(), Some("Done."));
        assert!(p.error.is_none());
    }

    #[test]
    fn claude_error_result() {
        let mut p = StreamParser::new(AgentKind::ClaudeCode);
        p.feed(r#"{"type":"result","subtype":"error_max_turns"}"#);
        assert_eq!(p.error.as_deref(), Some("error_max_turns"));
    }

    #[test]
    fn codex_legacy_events() {
        let mut p = StreamParser::new(AgentKind::Codex);
        let evs = p.feed(r#"{"id":"1","msg":{"type":"exec_command_begin","command":["bash","-lc","sed -i s/a/b/ a.html"]}}"#);
        assert!(matches!(&evs[0], AiEvent::Action { label } if label.starts_with("bash -lc")));
        p.feed(r#"{"id":"2","msg":{"type":"agent_message","message":"done"}}"#);
        p.feed(r#"{"id":"3","msg":{"type":"task_complete","last_agent_message":"done"}}"#);
        assert_eq!(p.final_text.as_deref(), Some("done"));
    }

    #[test]
    fn codex_item_events() {
        let mut p = StreamParser::new(AgentKind::Codex);
        let evs =
            p.feed(r#"{"type":"item.completed","item":{"type":"agent_message","text":"hi"}}"#);
        assert!(matches!(&evs[0], AiEvent::Delta { text } if text == "hi"));
        let evs = p.feed(
            r#"{"type":"item.started","item":{"type":"command_execution","command":"ls -la"}}"#,
        );
        assert!(matches!(&evs[0], AiEvent::Action { label } if label == "ls -la"));
    }

    #[test]
    fn garbage_lines_ignored() {
        let mut p = StreamParser::new(AgentKind::ClaudeCode);
        assert!(p.feed("not json at all").is_empty());
        assert!(p.error.is_none());
    }

    #[test]
    fn prompt_shapes() {
        let t = revise_task();
        let p = agent_prompt(&t);
        assert!(p.contains("`a.html`"));
        assert!(p.contains("dark theme"));
        let mut r = t.clone();
        r.kind = TaskKind::Review;
        r.instruction = String::new();
        let p = agent_prompt(&r);
        assert!(p.contains("Do not modify"));
        assert!(!p.contains("Extra focus"));
    }

    #[tokio::test]
    async fn revise_runs_a_fake_agent_and_reads_back_the_edit() {
        // A stub "agent": ignores its args, rewrites the file, prints one
        // stream-json line. Exercises spawn → parse → diff end to end.
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("claude");
        std::fs::write(
            &stub,
            "#!/bin/sh\nprintf '<html>dark</html>' > a.html\necho '{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"ok\"}'\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let task = revise_task();
        let mut events = vec![];
        let out = run(
            &task,
            AgentKind::ClaudeCode,
            stub.to_str().unwrap(),
            CancelFlag::new(),
            &mut |e| events.push(e),
        )
        .await
        .unwrap();
        assert_eq!(out.new_content.as_deref(), Some("<html>dark</html>"));
    }

    #[tokio::test]
    async fn cancel_kills_a_hanging_agent() {
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("claude");
        std::fs::write(&stub, "#!/bin/sh\nsleep 60\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let task = revise_task();
        let cancel = CancelFlag::new();
        let c2 = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            c2.cancel();
        });
        let started = Instant::now();
        let err = run(
            &task,
            AgentKind::ClaudeCode,
            stub.to_str().unwrap(),
            cancel,
            &mut |_| {},
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AiError::Cancelled));
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
