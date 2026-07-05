//! Harbly MCP server: the library tool surface over stdio, for agent CLIs.
//!
//! Spawned by Claude Code (via the app's --mcp-config) or hooked up manually
//! (`claude mcp add harbly -- harbly-mcp --library ~/Harbly`). A deliberately
//! minimal, dependency-free MCP implementation: newline-delimited JSON-RPC,
//! three methods that matter (initialize / tools/list / tools/call). Tool
//! names and schemas come from harbly-ai so every supply sees the identical
//! surface; execution goes through harbly-core, so every write lands as an
//! attributed version. SQLite is shared with the running app via WAL + busy
//! timeout.

use harbly_core::{AiToolCtx, Library};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

fn main() {
    let args = Args::parse(std::env::args().skip(1));
    let Some(library) = args.library else {
        eprintln!("usage: harbly-mcp --library <path> [--session <id>] [--supply <name>] [--model <name>]");
        std::process::exit(2);
    };
    let lib = match Library::open_or_create(&library) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("harbly-mcp: cannot open library {library}: {e}");
            std::process::exit(1);
        }
    };
    // Incremental catch-up (unchanged files are stat-only): standalone use
    // may point at a folder the app hasn't indexed yet.
    let _ = lib.scan(|_| {});
    let ctx = AiToolCtx {
        supply: args.supply.unwrap_or_else(|| "claude".into()),
        model: args.model.unwrap_or_default(),
        session_id: args.session,
    };

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(req) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(resp) = handle(&req, &lib, &ctx) {
            let _ = writeln!(stdout, "{resp}");
            let _ = stdout.flush();
        }
    }
}

#[derive(Default)]
struct Args {
    library: Option<String>,
    session: Option<String>,
    supply: Option<String>,
    model: Option<String>,
}

impl Args {
    fn parse(mut it: impl Iterator<Item = String>) -> Self {
        let mut a = Args::default();
        while let Some(flag) = it.next() {
            let slot = match flag.as_str() {
                "--library" => &mut a.library,
                "--session" => &mut a.session,
                "--supply" => &mut a.supply,
                "--model" => &mut a.model,
                _ => continue,
            };
            *slot = it.next();
        }
        a
    }
}

/// One JSON-RPC message in, at most one response out (notifications get none).
fn handle(req: &Value, lib: &Library, ctx: &AiToolCtx) -> Option<Value> {
    let method = req["method"].as_str().unwrap_or_default();
    let id = req.get("id").filter(|v| !v.is_null())?.clone();

    let result = match method {
        "initialize" => Ok(json!({
            // Echo the client's protocol version — this server's surface is
            // small enough to be compatible across revisions.
            "protocolVersion": req["params"]["protocolVersion"]
                .as_str()
                .unwrap_or("2024-11-05"),
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "harbly",
                "version": env!("CARGO_PKG_VERSION"),
            },
        })),
        "ping" => Ok(json!({})),
        "tools/list" => {
            let tools: Vec<Value> = harbly_ai::tools::tool_specs()
                .iter()
                .map(|s| {
                    json!({
                        "name": s.name,
                        "description": s.description,
                        "inputSchema": s.schema,
                    })
                })
                .collect();
            Ok(json!({ "tools": tools }))
        }
        "tools/call" => {
            let name = req["params"]["name"].as_str().unwrap_or_default();
            let empty = json!({});
            let args = req["params"].get("arguments").unwrap_or(&empty);
            // Tool failures are results, not protocol errors: the model reads
            // them and adapts.
            let (text, is_error) = match lib.execute_ai_tool(name, args, ctx) {
                Ok((v, _)) => (v.to_string(), false),
                Err(e) => (json!({ "error": e }).to_string(), true),
            };
            Ok(json!({
                "content": [{ "type": "text", "text": text }],
                "isError": is_error,
            }))
        }
        _ => Err(json!({ "code": -32601, "message": format!("method not found: {method}") })),
    };

    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, Library, AiToolCtx) {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open_or_create(tmp.path().join("Harbly")).unwrap();
        let ctx = AiToolCtx {
            supply: "claude".into(),
            model: String::new(),
            session_id: Some("sess-1".into()),
        };
        (tmp, lib, ctx)
    }

    #[test]
    fn initialize_lists_and_calls() {
        let (_tmp, lib, ctx) = setup();
        std::fs::write(
            lib.root().join("a.html"),
            "<!doctype html><html><title>定价页</title><body>方案甲</body></html>",
        )
        .unwrap();
        lib.scan(|_| {}).unwrap();

        let resp = handle(
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}),
            &lib,
            &ctx,
        )
        .unwrap();
        assert_eq!(resp["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(resp["result"]["serverInfo"]["name"], "harbly");

        // Notification → no response
        assert!(handle(
            &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            &lib,
            &ctx
        )
        .is_none());

        let resp = handle(
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            &lib,
            &ctx,
        )
        .unwrap();
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 6);
        assert!(tools.iter().any(|t| t["name"] == "write_asset"));

        let resp = handle(
            &json!({"jsonrpc":"2.0","id":3,"method":"tools/call",
                    "params":{"name":"search_library","arguments":{"query":"定价"}}}),
            &lib,
            &ctx,
        )
        .unwrap();
        assert_eq!(resp["result"]["isError"], false);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        let v: Value = serde_json::from_str(text).unwrap();
        let asset_id = v["results"][0]["asset_id"].as_str().unwrap().to_string();

        let resp = handle(
            &json!({"jsonrpc":"2.0","id":4,"method":"tools/call",
                    "params":{"name":"write_asset","arguments":{
                        "asset_id": asset_id,
                        "content": "<!doctype html><html><title>定价页</title><body>方案乙</body></html>",
                        "summary": "改成方案乙"}}}),
            &lib,
            &ctx,
        )
        .unwrap();
        assert_eq!(resp["result"]["isError"], false);
        assert!(lib.read_asset_text(&asset_id).unwrap().contains("方案乙"));
        let runs = lib.list_ai_runs(&asset_id, 5).unwrap();
        assert_eq!(runs[0].session_id.as_deref(), Some("sess-1"));

        // Tool failure = result with isError, never a protocol error
        let resp = handle(
            &json!({"jsonrpc":"2.0","id":5,"method":"tools/call",
                    "params":{"name":"read_asset","arguments":{"asset_id":"nope"}}}),
            &lib,
            &ctx,
        )
        .unwrap();
        assert_eq!(resp["result"]["isError"], true);

        // Unknown method = JSON-RPC error
        let resp = handle(
            &json!({"jsonrpc":"2.0","id":6,"method":"resources/list"}),
            &lib,
            &ctx,
        )
        .unwrap();
        assert_eq!(resp["error"]["code"], -32601);
    }

    #[test]
    fn args_parse() {
        let a = Args::parse(
            [
                "--library",
                "/tmp/L",
                "--session",
                "s1",
                "--supply",
                "claude",
            ]
            .iter()
            .map(|s| s.to_string()),
        );
        assert_eq!(a.library.as_deref(), Some("/tmp/L"));
        assert_eq!(a.session.as_deref(), Some("s1"));
        assert_eq!(a.supply.as_deref(), Some("claude"));
        assert!(a.model.is_none());
    }
}
