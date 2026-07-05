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
/// How often the cancel flag is polled while awaiting the network.
const CANCEL_TICK: Duration = Duration::from_millis(250);
/// Tool round-trips per turn — a loop guard, not a feature budget.
const MAX_STEPS: usize = 12;
const ANTHROPIC_MAX_TOKENS: u32 = 32_768;

use crate::thinking_budget;

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
    // stop_reason/finish_reason of the last step — turns a blank turn into an
    // actionable error (truncation vs refusal) instead of "empty response".
    let mut last_stop: Option<String> = None;
    // The model still wanted tools when the step budget ran out. Its pending
    // calls were NOT executed: performing writes whose results can never be
    // reported back would leave silent side effects.
    let mut steps_exhausted = false;

    match provider {
        ByokProvider::Anthropic => {
            let mut messages = anthropic_history(task);
            // Effort knobs vary by model generation and the id heuristic can
            // misread custom models; a shape 400 on the FIRST request (nothing
            // streamed, no replay in flight) walks the fallback chain.
            let mut shapes = shape_fallbacks(detect_shape(model, &task.effort)).into_iter();
            let mut shape = shapes.next().unwrap_or(EffortShape::Bare);
            let mut step = 0;
            while step < MAX_STEPS {
                let body = anthropic_body(task, model, &messages, shape);
                let asm = match stream_step(
                    &client,
                    provider,
                    api_key,
                    body,
                    deadline,
                    &cancel,
                    on_event,
                    StepAsm::new_anthropic(),
                )
                .await
                {
                    Ok(a) => a,
                    Err(AiError::Provider(msg)) if step == 0 && is_effort_shape_error(&msg) => {
                        match shapes.next() {
                            Some(next) => {
                                shape = next;
                                continue;
                            }
                            None => return Err(AiError::Provider(msg)),
                        }
                    }
                    Err(e) => return Err(e),
                };
                step += 1;
                let (blocks, stop_reason, text) = asm.into_anthropic();
                last_stop = stop_reason.clone();
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
                if step == MAX_STEPS {
                    steps_exhausted = true;
                    break;
                }
                // The API rejects empty text blocks on replay (a text block
                // can open and close with zero deltas ahead of tool_use).
                let replay: Vec<Value> = blocks
                    .into_iter()
                    .filter(|b| {
                        !(b["type"] == "text" && b["text"].as_str().unwrap_or("").is_empty())
                    })
                    .collect();
                messages.push(json!({ "role": "assistant", "content": replay }));
                let mut results = Vec::new();
                for tu in &tool_uses {
                    if cancel.is_cancelled() {
                        return Err(AiError::Cancelled);
                    }
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
            let mut step = 0;
            while step < MAX_STEPS {
                let body = openai_body(task, provider, model, &messages);
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
                step += 1;
                let (text, calls, finish, reasoning) = asm.into_openai();
                last_stop = finish.clone();
                if !text.is_empty() {
                    reply = text.clone();
                }
                if finish.as_deref() != Some("tool_calls") || calls.is_empty() {
                    break;
                }
                if step == MAX_STEPS {
                    steps_exhausted = true;
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
                let mut assistant = json!({
                    "role": "assistant",
                    "content": if text.is_empty() { Value::Null } else { Value::String(text) },
                    "tool_calls": tool_calls,
                });
                // OpenRouter requires reasoning_details to be echoed back
                // verbatim on tool round-trips — Anthropic/Gemini upstreams
                // reject the follow-up without their reasoning blocks.
                if provider == ByokProvider::OpenRouter && !reasoning.is_empty() {
                    assistant["reasoning_details"] = Value::Array(reasoning);
                }
                messages.push(assistant);
                for c in &calls {
                    if cancel.is_cancelled() {
                        return Err(AiError::Cancelled);
                    }
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

    if steps_exhausted {
        if reply.trim().is_empty() {
            return Err(AiError::StepLimit);
        }
        let note = step_limit_note(&task.reply_lang);
        on_event(AiEvent::Delta { text: note.clone() });
        reply.push_str(&note);
    }
    if reply.trim().is_empty() {
        return Err(empty_reply_error(last_stop.as_deref()));
    }
    Ok(TurnOutput {
        reply,
        agent_session_id: None,
    })
}

// ---------- Anthropic effort shapes ----------

/// Effort knobs by model generation (verified 2026-07 against the
/// extended-thinking docs):
/// - Sonnet 5 / Opus 4.8+: adaptive thinking + output_config effort;
/// - Fable/Mythos 5: thinking is always-on — the field must be OMITTED
///   (sending enabled OR disabled is a 400), effort goes via output_config;
/// - Haiku 4.5 / Claude ≤4.5 era: legacy thinking {enabled, budget_tokens}.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffortShape {
    Adaptive,
    OutputOnly,
    Legacy,
    Bare,
}

fn detect_shape(model: &str, effort: &str) -> EffortShape {
    if !crate::is_anthropic_effort(effort) {
        return EffortShape::Bare;
    }
    if model.contains("fable") || model.contains("mythos") {
        return EffortShape::OutputOnly;
    }
    if model.contains("haiku") || model.contains("-4-5") || model.contains("claude-3") {
        return EffortShape::Legacy;
    }
    EffortShape::Adaptive
}

/// Shapes to try in order: the detected one, the plausible alternate, then no
/// knobs at all. Custom model ids the heuristic misreads (claude-opus-4-1
/// matches no legacy marker but only speaks the legacy shape) still get a
/// working turn instead of a hard 400.
fn shape_fallbacks(first: EffortShape) -> Vec<EffortShape> {
    match first {
        EffortShape::Adaptive => vec![
            EffortShape::Adaptive,
            EffortShape::Legacy,
            EffortShape::Bare,
        ],
        EffortShape::Legacy => vec![
            EffortShape::Legacy,
            EffortShape::Adaptive,
            EffortShape::Bare,
        ],
        EffortShape::OutputOnly => vec![
            EffortShape::OutputOnly,
            EffortShape::Adaptive,
            EffortShape::Bare,
        ],
        EffortShape::Bare => vec![EffortShape::Bare],
    }
}

/// A 400 whose message names the effort/thinking knobs. Anything else (auth,
/// overload, content) must NOT trigger a re-send.
fn is_effort_shape_error(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.starts_with("http 400")
        && [
            "thinking",
            "output_config",
            "budget_tokens",
            "adaptive",
            "effort",
        ]
        .iter()
        .any(|k| lower.contains(k))
}

fn empty_reply_error(stop: Option<&str>) -> AiError {
    AiError::Provider(match stop {
        Some("max_tokens") | Some("length") => {
            "output truncated by max_tokens before any reply".into()
        }
        Some("refusal") | Some("content_filter") => "the model declined to answer".into(),
        _ => "empty response".into(),
    })
}

/// Appended to the visible reply when the step budget ran out mid-task —
/// transcript content, so it follows the reply language (errors stay
/// canonical-Chinese and are localized at the UI edge instead).
fn step_limit_note(lang: &str) -> String {
    let text = if lang.starts_with("zh-TW") {
        "已達單回合工具步數上限，任務可能未全部完成——回覆「繼續」可接著做。"
    } else if lang.starts_with("zh") {
        "已达单回合工具步数上限，任务可能未全部完成——回复「继续」可接着做。"
    } else if lang.starts_with("ja") {
        "1ターンのツール実行上限に達しました。タスクは未完了の可能性があります。「続けて」と送ると続行します。"
    } else if lang.starts_with("ko") {
        "이번 턴의 도구 실행 한도에 도달했습니다. 작업이 완료되지 않았을 수 있습니다. \"계속\"이라고 보내면 이어서 진행합니다."
    } else if lang.starts_with("es") {
        "Se alcanzó el límite de pasos de herramientas de este turno; la tarea puede quedar incompleta. Responde «continúa» para seguir."
    } else {
        "Tool-step limit for this turn reached; the task may be incomplete. Reply \"continue\" to keep going."
    };
    format!("\n\n> {text}")
}

// ---------- Request bodies ----------

fn anthropic_history(task: &SessionTask) -> Vec<Value> {
    let mut messages: Vec<Value> = task
        .history
        .iter()
        // A failed turn persists its user message with no assistant reply, so
        // a fixed-size window can open on an assistant turn — Anthropic
        // requires the first message to be user-role.
        .skip_while(|t| t.role == Role::Assistant)
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

fn anthropic_body(
    task: &SessionTask,
    model: &str,
    messages: &[Value],
    shape: EffortShape,
) -> Value {
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
    match shape {
        EffortShape::Bare => {}
        EffortShape::Legacy => {
            if let Some(budget) = thinking_budget(&task.effort) {
                body["thinking"] = json!({ "type": "enabled", "budget_tokens": budget });
            }
        }
        EffortShape::OutputOnly => {
            body["output_config"] = json!({ "effort": task.effort });
        }
        EffortShape::Adaptive => {
            body["output_config"] = json!({ "effort": task.effort });
            body["thinking"] = json!({ "type": "adaptive" });
        }
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

fn openai_body(
    task: &SessionTask,
    provider: ByokProvider,
    model: &str,
    messages: &[Value],
) -> Value {
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
        if provider == ByokProvider::OpenRouter {
            // OpenRouter's canonical cross-provider knob; it normalizes to each
            // upstream's native parameter (reasoning_effort passthrough is
            // inconsistent there).
            body["reasoning"] = json!({ "effort": task.effort });
        } else {
            body["reasoning_effort"] = json!(task.effort);
        }
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

    // Await in short ticks so Stop is honored within ~250ms even while the
    // request is connecting or the stream is silent (long thinking phases),
    // not only when the next byte happens to arrive.
    let send_fut = req.send();
    tokio::pin!(send_fut);
    let resp = loop {
        if cancel.is_cancelled() {
            return Err(AiError::Cancelled);
        }
        if Instant::now() > deadline {
            return Err(AiError::Timeout);
        }
        match tokio::time::timeout(CANCEL_TICK, &mut send_fut).await {
            Err(_) => continue,
            Ok(r) => break r.map_err(|e| AiError::Http(e.to_string()))?,
        }
    };
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(AiError::Provider(provider_error(status.as_u16(), &body)));
    }

    let mut stream = resp.bytes_stream();
    let mut parser = SseParser::default();
    let mut last_byte = Instant::now();
    loop {
        if cancel.is_cancelled() {
            return Err(AiError::Cancelled);
        }
        if Instant::now() > deadline {
            return Err(AiError::Timeout);
        }
        if last_byte.elapsed() > CHUNK_TIMEOUT {
            return Err(AiError::Timeout);
        }
        let chunk = match tokio::time::timeout(CANCEL_TICK, stream.next()).await {
            Err(_) => continue,
            Ok(None) => break,
            Ok(Some(Err(e))) => return Err(AiError::Http(e.to_string())),
            Ok(Some(Ok(c))) => {
                last_byte = Instant::now();
                c
            }
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
        /// OpenRouter reasoning_details entries, kept verbatim for replay.
        reasoning: Vec<Value>,
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
            reasoning: Vec::new(),
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
                reasoning,
            } => {
                let choice = &v["choices"][0];
                if let Some(r) = choice["finish_reason"].as_str() {
                    *finish = Some(r.to_string());
                }
                // OpenRouter streams reasoning_details for reasoning models;
                // collected verbatim, replayed on the next step.
                if let Some(rd) = choice["delta"]["reasoning_details"].as_array() {
                    reasoning.extend(rd.iter().cloned());
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
                            // Strict OpenAI streams the name once; some
                            // OpenRouter upstreams (Gemini) resend the FULL
                            // name on every delta — appending would yield
                            // "search_librarysearch_library". Append only
                            // genuine fragments.
                            let cur = &mut calls[idx].name;
                            if cur.is_empty() || cur.as_str() != n {
                                cur.push_str(n);
                            }
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

    fn into_openai(self) -> (String, Vec<OpenAiCall>, Option<String>, Vec<Value>) {
        match self {
            StepAsm::OpenAi {
                text,
                mut calls,
                finish,
                reasoning,
            } => {
                // A provider that omits call ids would otherwise be replayed
                // with "" and rejected; synthesize stable ones.
                for (i, c) in calls.iter_mut().enumerate() {
                    if c.id.is_empty() {
                        c.id = format!("call_{i}");
                    }
                }
                (text, calls, finish, reasoning)
            }
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
        let (text, calls, finish, _) = asm.into_openai();
        assert_eq!(text, "好的");
        assert_eq!(finish.as_deref(), Some("tool_calls"));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "search_library");
        let args: Value = serde_json::from_str(&calls[0].arguments).unwrap();
        assert_eq!(args["query"], "定价");
    }

    #[test]
    fn openai_tolerates_full_name_resends_and_missing_ids() {
        // Gemini-via-OpenRouter resends the complete name on every delta and
        // can omit the call id entirely.
        let mut asm = StepAsm::new_openai();
        feed_all(
            &mut asm,
            &[
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"name":"search_library","arguments":"{\"que"}}]}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"name":"search_library","arguments":"ry\":\"a\"}"}}]}}]}"#,
                r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
            ],
        );
        let (_, calls, _, _) = asm.into_openai();
        assert_eq!(calls[0].name, "search_library");
        assert_eq!(calls[0].id, "call_0");
    }

    #[test]
    fn openrouter_reasoning_details_are_collected_verbatim() {
        let mut asm = StepAsm::new_openai();
        feed_all(
            &mut asm,
            &[
                r#"{"choices":[{"index":0,"delta":{"reasoning_details":[{"type":"reasoning.text","text":"想一想","index":0}]}}]}"#,
                r#"{"choices":[{"index":0,"delta":{"reasoning_details":[{"type":"reasoning.signature","signature":"sig","index":0}]}}]}"#,
                r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
            ],
        );
        let (_, _, _, reasoning) = asm.into_openai();
        assert_eq!(reasoning.len(), 2);
        assert_eq!(reasoning[0]["type"], "reasoning.text");
        assert_eq!(reasoning[1]["signature"], "sig");
    }

    fn body_for(t: &crate::SessionTask, model: &str) -> Value {
        anthropic_body(
            t,
            model,
            &anthropic_history(t),
            detect_shape(model, &t.effort),
        )
    }

    #[test]
    fn bodies_carry_tools_and_effort() {
        let mut t = task();
        t.effort = "high".into();

        // Current generation: adaptive thinking + output_config effort
        let body = body_for(&t, "claude-sonnet-5");
        assert_eq!(body["tools"].as_array().unwrap().len(), 6);
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(body["output_config"]["effort"], "high");
        assert_eq!(
            body["messages"].as_array().unwrap().last().unwrap()["content"],
            "make it dark"
        );
        // Fable: always-on thinking — the field must be omitted entirely
        let body = body_for(&t, "claude-fable-5");
        assert!(body.get("thinking").is_none());
        assert_eq!(body["output_config"]["effort"], "high");
        // Haiku 4.5: legacy budget shape, no output_config
        let body = body_for(&t, "claude-haiku-4-5");
        assert_eq!(body["thinking"]["budget_tokens"], 24_000);
        assert!(body.get("output_config").is_none());

        let body = openai_body(&t, ByokProvider::OpenAi, "gpt-5.5", &openai_history(&t));
        assert_eq!(body["reasoning_effort"], "high");
        assert_eq!(body["tools"][0]["function"]["name"], "search_library");
        assert_eq!(body["messages"][0]["role"], "system");
        // OpenRouter uses its canonical reasoning object instead
        let body = openai_body(
            &t,
            ByokProvider::OpenRouter,
            "anthropic/claude-sonnet-5",
            &openai_history(&t),
        );
        assert!(body.get("reasoning_effort").is_none());
        assert_eq!(body["reasoning"]["effort"], "high");

        // No effort → no knobs at all (model defaults apply)
        t.effort = String::new();
        let body = openai_body(&t, ByokProvider::OpenAi, "gpt-5.5", &openai_history(&t));
        assert!(body.get("reasoning_effort").is_none());
        let body = body_for(&t, "claude-sonnet-5");
        assert!(body.get("thinking").is_none());
        assert!(body.get("output_config").is_none());
    }

    #[test]
    fn shape_fallback_chain_recovers_custom_models() {
        // claude-opus-4-1 matches no marker → detected Adaptive, but it only
        // speaks the legacy shape; the chain must offer Legacy before Bare.
        assert_eq!(
            detect_shape("claude-opus-4-1", "high"),
            EffortShape::Adaptive
        );
        assert_eq!(
            shape_fallbacks(EffortShape::Adaptive),
            vec![
                EffortShape::Adaptive,
                EffortShape::Legacy,
                EffortShape::Bare
            ]
        );
        assert_eq!(detect_shape("claude-sonnet-5", ""), EffortShape::Bare);
        assert_eq!(shape_fallbacks(EffortShape::Bare), vec![EffortShape::Bare]);

        assert!(is_effort_shape_error(
            "HTTP 400: Unexpected value(s) for the `thinking` parameter"
        ));
        assert!(is_effort_shape_error(
            "HTTP 400: output_config: Extra inputs are not permitted"
        ));
        // Non-shape failures must not trigger a re-send
        assert!(!is_effort_shape_error("HTTP 401: invalid x-api-key"));
        assert!(!is_effort_shape_error("HTTP 400: max_tokens: too large"));
    }

    #[test]
    fn anthropic_history_never_opens_on_assistant() {
        let mut t = task();
        t.history = vec![
            crate::ChatTurn {
                role: Role::Assistant,
                text: "上一回合的孤儿回复".into(),
            },
            crate::ChatTurn {
                role: Role::User,
                text: "继续".into(),
            },
        ];
        let msgs = anthropic_history(&t);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "继续");
    }

    #[test]
    fn empty_reply_errors_carry_stop_reason() {
        assert!(matches!(
            empty_reply_error(Some("max_tokens")),
            AiError::Provider(m) if m.contains("max_tokens")
        ));
        assert!(matches!(
            empty_reply_error(Some("refusal")),
            AiError::Provider(m) if m.contains("declined")
        ));
        assert!(matches!(
            empty_reply_error(None),
            AiError::Provider(m) if m == "empty response"
        ));
        assert!(step_limit_note("zh-CN").contains("步数上限"));
        assert!(step_limit_note("en").contains("limit"));
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
