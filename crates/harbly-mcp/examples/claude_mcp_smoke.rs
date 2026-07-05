//! Manual end-to-end smoke of the claude + MCP pipeline: a real Claude Code
//! process gets the Harbly MCP server, must DISCOVER an asset by search, read
//! it, and write a revision — no current-file context, pure tool use. Spends
//! a real agent invocation — run explicitly, never in CI:
//!
//! ```sh
//! cargo build -p harbly-mcp && cargo run -p harbly-mcp --example claude_mcp_smoke
//! ```

use harbly_ai::{detect_agent, run_turn, AgentKind, AiEvent, SessionTask, Supply, ToolExecutor};
use harbly_core::Library;

struct NoExec;
impl ToolExecutor for NoExec {
    fn execute(&self, _n: &str, _a: &serde_json::Value) -> Result<serde_json::Value, String> {
        Err("claude path must go through MCP, not the in-process executor".into())
    }
}

#[tokio::main]
async fn main() {
    let Some(info) = detect_agent(AgentKind::ClaudeCode).await else {
        println!("SKIP: claude not detected");
        return;
    };
    let mcp_bin = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("harbly-mcp");
    if !mcp_bin.is_file() {
        println!("SKIP: build harbly-mcp first (cargo build -p harbly-mcp)");
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("Harbly");
    let lib = Library::open_or_create(&root).unwrap();
    std::fs::write(
        root.join("pricing.html"),
        "<!doctype html><html><head><title>Pricing</title></head>\
         <body style=\"background:#ffffff\"><h1>Pricing</h1></body></html>",
    )
    .unwrap();
    lib.scan(|_| {}).unwrap();
    let session = lib.create_ai_session("claude", "", "").unwrap();

    let mcp_config = serde_json::json!({
        "mcpServers": { "harbly": {
            "command": mcp_bin,
            "args": ["--library", root, "--session", session.id, "--supply", "claude"],
        }}
    })
    .to_string();

    let task = SessionTask {
        instruction: "Find the page titled 'Pricing' in the library (search for it), then \
                      change its body background to #0b1021 with white text, keeping \
                      everything else."
            .into(),
        history: vec![],
        current_asset: None,
        reply_lang: "zh-CN".into(),
        effort: String::new(),
    };
    let supply = Supply::Agent {
        kind: AgentKind::ClaudeCode,
        program: info.path,
        model: None,
        workdir: root.join(".harbly").join("ai-workspace"),
        mcp_config_json: Some(mcp_config),
    };

    let out = run_turn(
        &task,
        &supply,
        &NoExec,
        None,
        harbly_ai::CancelFlag::new(),
        &mut |e| match e {
            AiEvent::Action { label } => println!("action: {label}"),
            AiEvent::Delta { text } => {
                println!("delta: {}", text.chars().take(90).collect::<String>());
            }
        },
    )
    .await;

    match out {
        Ok(o) => {
            let a = lib.asset_by_rel("pricing.html").unwrap();
            let content = lib.read_asset_text(&a.id).unwrap();
            let runs = lib.list_ai_runs(&a.id, 5).unwrap();
            println!(
                "OK · reply={} · session={} · dark={} · vers={} · run_supply={} · run_session_linked={}",
                o.reply.chars().take(60).collect::<String>(),
                o.agent_session_id.as_deref().unwrap_or("-"),
                content.contains("#0b1021"),
                a.ver_count,
                runs.first().map(|r| r.supply.as_str()).unwrap_or("-"),
                runs.first()
                    .map(|r| r.session_id.as_deref() == Some(session.id.as_str()))
                    .unwrap_or(false),
            );
        }
        Err(e) => println!("FAILED: {e}"),
    }
}
