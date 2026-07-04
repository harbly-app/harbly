//! Manual end-to-end smoke of the local-agent supply: detects `claude`, has it
//! revise a tiny page in a scratch dir, and checks the edit landed. Spends a
//! real agent invocation — run explicitly, never in CI:
//!
//! ```sh
//! cargo run -p harbly-ai --example agent_smoke
//! ```

use harbly_ai::{detect_agent, run_task, AgentKind, AiEvent, AiTask, CancelFlag, Supply};

#[tokio::main]
async fn main() {
    let Some(info) = detect_agent(AgentKind::ClaudeCode).await else {
        println!("SKIP: claude not detected");
        return;
    };
    println!(
        "detected: {} ({})",
        info.path,
        info.version.as_deref().unwrap_or("?")
    );

    let task = AiTask {
        instruction: "Change the <h1> text to exactly 'Harbly Smoke OK' and give the body a \
                      #0b1021 background with white text."
            .into(),
        file_name: "smoke.html".into(),
        content: "<!doctype html><html><head><title>Smoke</title></head>\
                  <body><h1>Before</h1><p>test page</p></body></html>"
            .into(),
        is_markdown: false,
        title: "Smoke".into(),
        reply_lang: "zh-CN".into(),
    };
    let supply = Supply::Agent {
        kind: AgentKind::ClaudeCode,
        program: info.path,
    };

    let mut events = 0usize;
    let result = run_task(&task, &supply, CancelFlag::new(), &mut |e| {
        events += 1;
        match e {
            AiEvent::Delta { text } => {
                println!("delta: {}", text.chars().take(90).collect::<String>());
            }
            AiEvent::Action { label } => println!("action: {label}"),
        }
    })
    .await;

    match result {
        Ok(out) => {
            let content = out.new_content.unwrap_or_default();
            println!(
                "OK · events={events} · changed={} · marker={}",
                !content.is_empty(),
                content.contains("Harbly Smoke OK"),
            );
        }
        Err(e) => println!("FAILED: {e}"),
    }
}
