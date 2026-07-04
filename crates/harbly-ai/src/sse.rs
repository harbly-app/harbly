//! Minimal incremental SSE decoder: byte chunks in, `data:` payloads out.
//! Both Anthropic and OpenAI emit one JSON document per `data:` line, so no
//! multi-line data accumulation is needed; `event:`/`id:`/comment lines are
//! ignored (Anthropic repeats the event type inside the JSON).

#[derive(Default)]
pub(crate) struct SseParser {
    buf: String,
}

impl SseParser {
    /// Feed one network chunk; returns the complete `data:` payloads it closed.
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buf.push_str(&String::from_utf8_lossy(chunk));
        let mut out = Vec::new();
        while let Some(pos) = self.buf.find('\n') {
            let line: String = self.buf.drain(..=pos).collect();
            let line = line.trim_end_matches(['\n', '\r']);
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.strip_prefix(' ').unwrap_or(data);
                if !data.is_empty() {
                    out.push(data.to_string());
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_payloads_across_chunks() {
        let mut p = SseParser::default();
        assert!(p.push(b"event: message_start\ndata: {\"a\":").is_empty());
        let got = p.push(b"1}\n\ndata: [DONE]\n");
        assert_eq!(got, vec!["{\"a\":1}".to_string(), "[DONE]".to_string()]);
    }

    #[test]
    fn handles_crlf_and_comments() {
        let mut p = SseParser::default();
        let got = p.push(b": keepalive\r\ndata: {\"x\":2}\r\n\r\n");
        assert_eq!(got, vec!["{\"x\":2}".to_string()]);
    }
}
