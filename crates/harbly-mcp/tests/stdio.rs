//! Full stdio round-trip against the real binary — what Claude Code actually
//! does when it spawns the server.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn stdio_roundtrip_initialize_list_call() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("Harbly");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("page.html"),
        "<!doctype html><html><title>Alpha</title><body>hello alpha</body></html>",
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_harbly-mcp"))
        .args(["--library", root.to_str().unwrap(), "--supply", "claude"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut rpc = |req: &str| -> serde_json::Value {
        writeln!(stdin, "{req}").unwrap();
        stdin.flush().unwrap();
        serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap()
    };

    let resp = rpc(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#,
    );
    assert_eq!(resp["result"]["serverInfo"]["name"], "harbly");

    let resp = rpc(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    assert_eq!(resp["result"]["tools"].as_array().unwrap().len(), 4);

    let resp = rpc(
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"search_library","arguments":{"query":"alpha"}}}"#,
    );
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let v: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(v["results"][0]["file_name"], "page.html");

    drop(stdin);
    let _ = child.wait();
}
