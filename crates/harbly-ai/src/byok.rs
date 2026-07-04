//! BYOK supplies: streaming tool-use loops straight against the provider,
//! key supplied by the caller (Harbly keeps it in the OS keychain, it never
//! touches disk here). Anthropic speaks its native Messages API; OpenAI and
//! OpenRouter share the chat-completions wire format. Tool calls are executed
//! between steps through the caller's [`ToolExecutor`] — the same library
//! surface the MCP server exposes to agent CLIs.

use crate::sse::SseParser;
use crate::tools::{call_label, tool_specs};
use crate::{
    system_prompt, AiError, AiEvent, ByokProvider, CancelFlag, EventSink, Role, SessionTask,
    ToolExecutor, TurnOutput,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// A whole turn may span several model requests (tool round-trips).
const TURN_TIMEOUT: Duration = Duration::from_secs(600);
/// Streams stall rather than fail when a proxy dies; give up after this long
/// without a single byte.
const CHUNK_TIMEOUT: Duration = Duration::from_secs(60);
/// Tool round-trips per turn — a loop guard, not a feature budget.
const MAX_STEPS: usize = 12;
const ANTHROPIC_MAX_TOKENS: u32 = 32_768;

fn thinking_budget(effort: &str) -> Option<u32> {
    match effort {
        "low" => Some(4_000),
        "medium" => Some(10_000),
        "high" => Some(24_000),
        _ => None,
    }
}

pub(crate) async fn run_turn(
    task: &SessionTask,
    provider: ByokProvider,
    api_key: &str,
    model: &str,
    executor: &dyn ToolExecutor,
    cancel: CancelFlag,
    on_event: EventSink<'_>,
) -> Result<TurnOutput, AiError> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| AiError::Http(e.to_string()))?;
    let deadline = Instant::now() + TURN_TIMEOUT;

    let mut reply = String::new();
    match provider {
        ByokProvider::Anthropic => {
            let mut messages = anthropic_history(task);
            for _ in 0..MAX_STEPS {
                let body = anthropic_body(task, model, &messages);
                let asm = stream_step(
                    &client,
                    provider,
                    api_key,
                    body,
                    deadline,
                    &cancel,
                    on_event,
                    StepAsm::new_anthropic(),
                )
                .await?;
                let (blocks, stop_reason, text) = asm.into_anthropic();
                if !text.is_empty() {
                    reply = text;
                }
                let tool_uses: Vec<Value> = blocks
                    .iter()
                    .filter(|b| b["type"] == "tool_use")
                    .cloned()
                    .collect();
                if stop_reason.as_deref() != Some("tool_use") || tool_uses.is_empty() {
                    break;
                }
                messages.push(json!({ "role": "assistant", "content": blocks }));
                let mut results = Vec::new();
                for tu in &tool_uses {
                    let name = tu["name"].as_str().unwrap_or_default();
                    let args = &tu["input"];
                    on_event(AiEvent::Action {
                        label: call_label(name, args),
                    });
                    let (content, is_error) = match executor.execute(name, args) {
                        Ok(v) => (v.to_string(), false),
                        Err(e) => (json!({ "error": e }).to_string(), true),
                    };
                    results.push(json!({
                        "type": "tool_result",
                        "tool_use_id": tu["id"],
                        "content": content,
                        "is_error": is_error,
                    }));
                }
                messages.push(json!({ "role": "user", "content": results }));
            }
        }
        ByokProvider::OpenAi | ByokProvider::OpenRouter => {
            let mut messages = openai_history(task);
            for _ in 0..MAX_STEPS {
                let body = openai_body(task, model, &messages);
                let asm = stream_step(
                    &client,
                    provider,
                    api_key,
                    body,
                    deadline,
                    &cancel,
                    on_event,
                    StepAsm::new_openai(),
                )
                .await?;
                let (text, calls, finish) = asm.into_openai();
                if !text.is_empty() {
                    reply = text.clone();
                }
                if finish.as_deref() != Some("tool_calls") || calls.is_empty() {
                    break;
                }
                let tool_calls: Vec<Value> = calls
                    .iter()
                    .map(|c| {
                        json!({
                            "id": c.id,
                            "type": "function",
                            "function": { "name": c.name, "arguments": c.arguments },
                        })
                    })
                    .collect();
                messages.push(json!({
                    "role": "assistant",
                    "content": if text.is_empty() { Value::Null } else { Value::String(text) },
                    "tool_calls": tool_calls,
                }));
                for c in &calls {
                    let args: Value =
                        serde_json::from_str(&c.arguments).unwrap_or_else(|_| json!({}));
                    on_event(AiEvent::Action {
                        label: call_label(&c.name, &args),
                    });
                    let content = match executor.execute(&c.name, &args) {
                        Ok(v) => v.to_string(),
                        Err(e) => json!({ "error": e }).to_string(),
                    };
                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": c.id,
                        "content": content,
                    }));
                }
            }
        }
    }

    if reply.trim().is_empty() {
        return Err(AiError::Provider("empty response".into()));
    }
    Ok(TurnOutput {
        reply,
        agent_session_id: None,
    })
}

// ---------- Request bodies ----------

fn anthropic_history(task: &SessionTask) -> Vec<Value> {
    let mut messages: Vec<Value> = task
        .history
        .iter()
        .map(|t| {
            json!({
                "role": match t.role { Role::User => "user", Role::Assistant => "assistant" },
                "content": t.text,
            })
        })
        .collect();
    messages.push(json!({ "role": "user", "content": task.instruction }));
    messages
}

fn anthropic_body(task: &SessionTask, model: &str, messages: &[Value]) -> Value {
    let tools: Vec<Value> = tool_specs()
        .iter()
        .map(|s| json!({ "name": s.name, "description": s.description, "input_schema": s.schema }))
        .collect();
    let mut body = json!({
        "model": model,
        "max_tokens": ANTHROPIC_MAX_TOKENS,
        "system": system_prompt(task),
        "messages": messages,
        "tools": tools,
        "stream": true,
    });
    if let Some(budget) = thinking_budget(&task.effort) {
        body["thinking"] = json!({ "type": "enabled", "budget_tokens": budget });
    }
    body
}

fn openai_history(task: &SessionTask) -> Vec<Value> {
    let mut messages = vec![json!({ "role": "system", "content": system_prompt(task) })];
    for t in &task.history {
        messages.push(json!({
            "role": match t.role { Role::User => "user", Role::Assistant => "assistant" },
            "content": t.text,
        }));
    }
    messages.push(json!({ "role": "user", "content": task.instruction }));
    messages
}

fn openai_body(task: &SessionTask, model: &str, messages: &[Value]) -> Value {
    let tools: Vec<Value> = tool_specs()
        .iter()
        .map(|s| {
            json!({
                "type": "function",
                "function": { "name": s.name, "description": s.description, "parameters": s.schema },
            })
        })
        .collect();
    let mut body = json!({
        "model": model,
        "messages": messages,
        "tools": tools,
        "stream": true,
    });
    if !task.effort.is_empty() {
        body["reasoning_effort"] = json!(task.effort);
    }
    body
}

// ---------- One streamed request ----------

#[expect(
    clippy::too_many_arguments,
    reason = "internal plumbing shared by both providers; a struct would just mirror the locals"
)]
async fn stream_step(
    client: &reqwest::Client,
    provider: ByokProvider,
    api_key: &str,
    body: Value,
    deadline: Instant,
    cancel: &CancelFlag,
    on_event: EventSink<'_>,
    mut asm: StepAsm,
) -> Result<StepAsm, AiError> {
    let req = match provider {
        ByokProvider::Anthropic => client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body),
        ByokProvider::OpenAi | ByokProvider::OpenRouter => {
            let url = match provider {
                ByokProvider::OpenAi => "https://api.openai.com/v1/chat/completions",
                _ => "https://openrouter.ai/api/v1/chat/completions",
            };
            let mut r = client.post(url).bearer_auth(api_key).json(&body);
            if provider == ByokProvider::OpenRouter {
                // OpenRouter attribution headers (optional but recommended)
                r = r
                    .header("HTTP-Referer", "https://harbly.app")
                    .header("X-Title", "Harbly");
            }
            r
        }
    };

    let resp = req.send().await.map_err(|e| AiError::Http(e.to_string()))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(AiError::Provider(provider_error(status.as_u16(), &body)));
    }

    let mut stream = resp.bytes_stream();
    let mut parser = SseParser::default();
    loop {
        if cancel.is_cancelled() {
            return Err(AiError::Cancelled);
        }
        if Instant::now() > deadline {
            return Err(AiError::Timeout);
        }
        let chunk = match tokio::time::timeout(CHUNK_TIMEOUT, stream.next()).await {
            Err(_) => return Err(AiError::Timeout),
            Ok(None) => break,
            Ok(Some(Err(e))) => return Err(AiError::Http(e.to_string())),
            Ok(Some(Ok(c))) => c,
        };
        for payload in parser.push(&chunk) {
            if payload == "[DONE]" {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(&payload) else {
                continue;
            };
            if let Some(err) = stream_error(&v) {
                return Err(AiError::Provider(err));
            }
            if let Some(text) = asm.feed(&v) {
                on_event(AiEvent::Delta { text });
            }
        }
    }
    Ok(asm)
}

// ---------- Stream assemblers ----------

/// Rebuilds complete messages from stream deltas. Anthropic: content blocks
/// (text / tool_use / thinking with signature — replayed verbatim on the next
/// step, as extended thinking requires). OpenAI: text + tool_calls by index.
pub(crate) enum StepAsm {
    Anthropic {
        blocks: Vec<AnthropicBlock>,
        stop_reason: Option<String>,
    },
    OpenAi {
        text: String,
        calls: Vec<OpenAiCall>,
        finish: Option<String>,
    },
}

pub(crate) struct AnthropicBlock {
    base: Value,
    input_buf: String,
}

#[derive(Clone)]
pub(crate) struct OpenAiCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

impl StepAsm {
    fn new_anthropic() -> Self {
        StepAsm::Anthropic {
            blocks: Vec::new(),
            stop_reason: None,
        }
    }

    fn new_openai() -> Self {
        StepAsm::OpenAi {
            text: String::new(),
            calls: Vec::new(),
            finish: None,
        }
    }

    /// Feed one SSE JSON payload; returns visible text to stream to the UI.
    pub(crate) fn feed(&mut self, v: &Value) -> Option<String> {
        match self {
            StepAsm::Anthropic {
                blocks,
                stop_reason,
            } => match v["type"].as_str()? {
                "content_block_start" => {
                    let idx = v["index"].as_u64()? as usize;
                    while blocks.len() <= idx {
                        blocks.push(AnthropicBlock {
                            base: json!({}),
                            input_buf: String::new(),
                        });
                    }
                    blocks[idx].base = v["content_block"].clone();
                    // Streamed tool input arrives via input_json_delta
                    if blocks[idx].base["type"] == "tool_use" {
                        blocks[idx].base["input"] = json!({});
                    }
                    None
                }
                "content_block_delta" => {
                    let idx = v["index"].as_u64()? as usize;
                    let b = blocks.get_mut(idx)?;
                    match v["delta"]["type"].as_str()? {
                        "text_delta" => {
                            let t = v["delta"]["text"].as_str()?.to_string();
                            if let Some(s) = b.base["text"].as_str() {
                                b.base["text"] = json!(format!("{s}{t}"));
                            }
                            Some(t)
                        }
                        "input_json_delta" => {
                            b.input_buf
                                .push_str(v["delta"]["partial_json"].as_str().unwrap_or(""));
                            None
                        }
                        "thinking_delta" => {
                            let t = v["delta"]["thinking"].as_str().unwrap_or("");
                            if let Some(s) = b.base["thinking"].as_str() {
                                b.base["thinking"] = json!(format!("{s}{t}"));
                            }
                            None
                        }
                        "signature_delta" => {
                            b.base["signature"] = v["delta"]["signature"].clone();
                            None
                        }
                        _ => None,
                    }
                }
                "content_block_stop" => {
                    let idx = v["index"].as_u64()? as usize;
                    let b = blocks.get_mut(idx)?;
                    if b.base["type"] == "tool_use" && !b.input_buf.is_empty() {
                        b.base["input"] =
                            serde_json::from_str(&b.input_buf).unwrap_or_else(|_| json!({}));
                    }
                    None
                }
                "message_delta" => {
                    if let Some(r) = v["delta"]["stop_reason"].as_str() {
                        *stop_reason = Some(r.to_string());
                    }
                    None
                }
                _ => None,
            },
            StepAsm::OpenAi {
                text,
                calls,
                finish,
            } => {
                let choice = &v["choices"][0];
                if let Some(r) = choice["finish_reason"].as_str() {
                    *finish = Some(r.to_string());
                }
                if let Some(tcs) = choice["delta"]["tool_calls"].as_array() {
                    for tc in tcs {
                        let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                        while calls.len() <= idx {
                            calls.push(OpenAiCall {
                                id: String::new(),
                                name: String::new(),
                                arguments: String::new(),
                            });
                        }
                        if let Some(id) = tc["id"].as_str() {
                            calls[idx].id = id.to_string();
                        }
                        if let Some(n) = tc["function"]["name"].as_str() {
                            calls[idx].name.push_str(n);
                        }
                        if let Some(a) = tc["function"]["arguments"].as_str() {
                            calls[idx].arguments.push_str(a);
                        }
                    }
                }
                let t = choice["delta"]["content"].as_str()?;
                if t.is_empty() {
                    return None;
                }
                text.push_str(t);
                Some(t.to_string())
            }
        }
    }

    fn into_anthropic(self) -> (Vec<Value>, Option<String>, String) {
        match self {
            StepAsm::Anthropic {
                blocks,
                stop_reason,
            } => {
                let vals: Vec<Value> = blocks.into_iter().map(|b| b.base).collect();
                let text = vals
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("");
                (vals, stop_reason, text)
            }
            StepAsm::OpenAi { .. } => unreachable!("provider mismatch"),
        }
    }

    fn into_openai(self) -> (String, Vec<OpenAiCall>, Option<String>) {
        match self {
            StepAsm::OpenAi {
                text,
                calls,
                finish,
            } => (text, calls, finish),
            StepAsm::Anthropic { .. } => unreachable!("provider mismatch"),
        }
    }
}

/// Mid-stream error events (Anthropic {"type":"error"}, OpenRouter {"error":…}).
fn stream_error(v: &Value) -> Option<String> {
    if v["type"] == "error" {
        return Some(
            v["error"]["message"]
                .as_str()
                .unwrap_or("provider error")
                .to_string(),
        );
    }
    v.get("error").filter(|e| !e.is_null()).map(|e| {
        e["message"]
            .as_str()
            .unwrap_or("provider error")
            .to_string()
    })
}

/// Condense an HTTP error body to something a toast can show.
fn provider_error(status: u16, body: &str) -> String {
    let msg = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v["error"]["message"]
                .as_str()
                .map(String::from)
                .or_else(|| v["error"].as_str().map(String::from))
                .or_else(|| v["message"].as_str().map(String::from))
        })
        .unwrap_or_else(|| body.chars().take(200).collect());
    format!("HTTP {status}: {msg}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::task;

    fn feed_all(asm: &mut StepAsm, payloads: &[&str]) -> String {
        let mut streamed = String::new();
        for p in payloads {
            let v: Value = serde_json::from_str(p).unwrap();
            if let Some(t) = asm.feed(&v) {
                streamed.push_str(&t);
            }
        }
        streamed
    }

    #[test]
    fn anthropic_assembles_text_and_tool_use() {
        let mut asm = StepAsm::new_anthropic();
        let streamed = feed_all(
            &mut asm,
            &[
                r#"{"type":"message_start","message":{}}"#,
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"读一下文件。"}}"#,
                r#"{"type":"content_block_stop","index":0}"#,
                r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tu_1","name":"read_asset","input":{}}}"#,
                r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"asset_"}}"#,
                r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"id\":\"a1\"}"}}"#,
                r#"{"type":"content_block_stop","index":1}"#,
                r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{}}"#,
            ],
        );
        assert_eq!(streamed, "读一下文件。");
        let (blocks, stop, text) = asm.into_anthropic();
        assert_eq!(stop.as_deref(), Some("tool_use"));
        assert_eq!(text, "读一下文件。");
        assert_eq!(blocks[1]["type"], "tool_use");
        assert_eq!(blocks[1]["input"]["asset_id"], "a1");
    }

    #[test]
    fn anthropic_replays_thinking_blocks_with_signature() {
        let mut asm = StepAsm::new_anthropic();
        feed_all(
            &mut asm,
            &[
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#,
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"考虑中"}}"#,
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig=="}}"#,
                r#"{"type":"content_block_stop","index":0}"#,
            ],
        );
        let (blocks, _, _) = asm.into_anthropic();
        assert_eq!(blocks[0]["type"], "thinking");
        assert_eq!(blocks[0]["thinking"], "考虑中");
        assert_eq!(blocks[0]["signature"], "sig==");
    }

    #[test]
    fn openai_assembles_split_tool_calls() {
        let mut asm = StepAsm::new_openai();
        let streamed = feed_all(
            &mut asm,
            &[
                r#"{"choices":[{"index":0,"delta":{"content":"好的"}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"search_library","arguments":""}}]}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"que"}}]}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"ry\":\"定价\"}"}}]}}]}"#,
                r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
            ],
        );
        assert_eq!(streamed, "好的");
        let (text, calls, finish) = asm.into_openai();
        assert_eq!(text, "好的");
        assert_eq!(finish.as_deref(), Some("tool_calls"));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "search_library");
        let args: Value = serde_json::from_str(&calls[0].arguments).unwrap();
        assert_eq!(args["query"], "定价");
    }

    #[test]
    fn bodies_carry_tools_and_effort() {
        let mut t = task();
        t.effort = "high".into();
        let body = anthropic_body(&t, "claude-sonnet-5", &anthropic_history(&t));
        assert_eq!(body["tools"].as_array().unwrap().len(), 4);
        assert_eq!(body["thinking"]["budget_tokens"], 24_000);
        assert_eq!(
            body["messages"].as_array().unwrap().last().unwrap()["content"],
            "make it dark"
        );

        let body = openai_body(&t, "gpt-5.1", &openai_history(&t));
        assert_eq!(body["reasoning_effort"], "high");
        assert_eq!(body["tools"][0]["function"]["name"], "search_library");
        assert_eq!(body["messages"][0]["role"], "system");

        t.effort = String::new();
        let body = openai_body(&t, "gpt-5.1", &openai_history(&t));
        assert!(body.get("reasoning_effort").is_none());
        let body = anthropic_body(&t, "m", &anthropic_history(&t));
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn history_maps_into_message_arrays() {
        let mut t = task();
        t.history = vec![
            crate::ChatTurn {
                role: Role::User,
                text: "改成深色".into(),
            },
            crate::ChatTurn {
                role: Role::Assistant,
                text: "已改。".into(),
            },
        ];
        let msgs = anthropic_history(&t);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[1]["role"], "assistant");
        let msgs = openai_history(&t);
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[1]["content"], "改成深色");
    }

    #[test]
    fn http_error_condenses() {
        let s = provider_error(401, r#"{"error":{"message":"invalid x-api-key"}}"#);
        assert_eq!(s, "HTTP 401: invalid x-api-key");
        let ok: Value = serde_json::from_str(r#"{"choices":[]}"#).unwrap();
        assert_eq!(stream_error(&ok), None);
    }
}
