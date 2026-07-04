//! BYOK supplies: streaming HTTP straight to the provider, key supplied by the
//! caller (Harbly keeps it in the OS keychain, it never touches disk here).
//! Anthropic speaks its native Messages API; OpenAI and OpenRouter share the
//! chat-completions wire format (that IS OpenRouter's native protocol).

use crate::sse::SseParser;
use crate::{
    extract_file_from_reply, prose_before_fence, unified_system, user_message, AiError, AiEvent,
    AiTask, ByokProvider, CancelFlag, EventSink, TaskOutput,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::time::Duration;

const BYOK_TIMEOUT: Duration = Duration::from_secs(300);
/// Streams stall rather than fail when a proxy dies; give up after this long
/// without a single byte.
const CHUNK_TIMEOUT: Duration = Duration::from_secs(60);
const ANTHROPIC_MAX_TOKENS: u32 = 32_768;

pub(crate) async fn run(
    task: &AiTask,
    provider: ByokProvider,
    api_key: &str,
    model: &str,
    cancel: CancelFlag,
    on_event: EventSink<'_>,
) -> Result<TaskOutput, AiError> {
    let text = tokio::time::timeout(
        BYOK_TIMEOUT,
        stream_completion(task, provider, api_key, model, cancel, on_event),
    )
    .await
    .map_err(|_| AiError::Timeout)??;

    // Outcome classification: sentinel fence with different content = rewrite;
    // everything else (prose, or a fenced no-op) = textual reply.
    let mut out = TaskOutput {
        assistant_text: text.clone(),
        ..TaskOutput::default()
    };
    match extract_file_from_reply(&text) {
        Some(content) if content.trim() != task.content.trim() => {
            out.new_content = Some(content);
        }
        Some(_) => out.reply = Some(prose_before_fence(&text)),
        None => out.reply = Some(text),
    }
    Ok(out)
}

async fn stream_completion(
    task: &AiTask,
    provider: ByokProvider,
    api_key: &str,
    model: &str,
    cancel: CancelFlag,
    on_event: EventSink<'_>,
) -> Result<String, AiError> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| AiError::Http(e.to_string()))?;

    let req = match provider {
        ByokProvider::Anthropic => client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": model,
                "max_tokens": ANTHROPIC_MAX_TOKENS,
                "system": unified_system(task),
                "messages": [{ "role": "user", "content": user_message(task) }],
                "stream": true,
            })),
        ByokProvider::OpenAi | ByokProvider::OpenRouter => {
            let url = match provider {
                ByokProvider::OpenAi => "https://api.openai.com/v1/chat/completions",
                _ => "https://openrouter.ai/api/v1/chat/completions",
            };
            let mut r = client.post(url).bearer_auth(api_key).json(&json!({
                "model": model,
                "messages": [
                    { "role": "system", "content": unified_system(task) },
                    { "role": "user", "content": user_message(task) },
                ],
                "stream": true,
            }));
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
    let mut text = String::new();
    loop {
        if cancel.is_cancelled() {
            return Err(AiError::Cancelled);
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
            if let Some(delta) = extract_delta(provider, &v) {
                if !delta.is_empty() {
                    text.push_str(&delta);
                    on_event(AiEvent::Delta { text: delta });
                }
            }
        }
    }
    if text.trim().is_empty() {
        return Err(AiError::Provider("empty response".into()));
    }
    Ok(text)
}

/// Text increment per SSE payload, per wire format.
fn extract_delta(provider: ByokProvider, v: &Value) -> Option<String> {
    match provider {
        // {"type":"content_block_delta","delta":{"type":"text_delta","text":"…"}}
        ByokProvider::Anthropic => {
            if v["type"] != "content_block_delta" {
                return None;
            }
            v["delta"]["text"].as_str().map(|s| s.to_string())
        }
        ByokProvider::OpenAi | ByokProvider::OpenRouter => v["choices"][0]["delta"]["content"]
            .as_str()
            .map(|s| s.to_string()),
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

    #[test]
    fn anthropic_delta_shape() {
        let v: Value = serde_json::from_str(
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#,
        )
        .unwrap();
        assert_eq!(
            extract_delta(ByokProvider::Anthropic, &v),
            Some("hi".into())
        );
        assert_eq!(extract_delta(ByokProvider::OpenAi, &v), None);
    }

    #[test]
    fn openai_delta_shape() {
        let v: Value =
            serde_json::from_str(r#"{"choices":[{"delta":{"content":"ok"},"index":0}]}"#).unwrap();
        assert_eq!(extract_delta(ByokProvider::OpenAi, &v), Some("ok".into()));
        assert_eq!(
            extract_delta(ByokProvider::OpenRouter, &v),
            Some("ok".into())
        );
    }

    #[test]
    fn error_events_surface() {
        let v: Value = serde_json::from_str(
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
        )
        .unwrap();
        assert_eq!(stream_error(&v), Some("Overloaded".into()));
        let v2: Value =
            serde_json::from_str(r#"{"error":{"message":"bad key","code":401}}"#).unwrap();
        assert_eq!(stream_error(&v2), Some("bad key".into()));
        let ok: Value = serde_json::from_str(r#"{"choices":[]}"#).unwrap();
        assert_eq!(stream_error(&ok), None);
    }

    #[test]
    fn http_error_condenses() {
        let s = provider_error(401, r#"{"error":{"message":"invalid x-api-key"}}"#);
        assert_eq!(s, "HTTP 401: invalid x-api-key");
    }
}
