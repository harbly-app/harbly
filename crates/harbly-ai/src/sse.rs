//! Minimal incremental SSE decoder: byte chunks in, `data:` payloads out.
//! Both Anthropic and OpenAI emit one JSON document per `data:` line, so no
//! multi-line data accumulation is needed; `event:`/`id:`/comment lines are
//! ignored (Anthropic repeats the event type inside the JSON).

#[derive(Default)]
pub(crate) struct SseParser {
    // Bytes, not String: network chunks split at arbitrary byte boundaries,
    // so a multi-byte UTF-8 character can straddle two chunks. Decoding per
    // chunk would turn both halves into U+FFFD. A line never splits a
    // character ('\n' is a single byte that cannot occur inside a multi-byte
    // sequence), so decoding per complete line is safe.
    buf: Vec<u8>,
    scanned: usize,
}

impl SseParser {
    /// Feed one network chunk; returns the complete `data:` payloads it closed.
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(rel) = self.buf[self.scanned..].iter().position(|&b| b == b'\n') {
            let pos = self.scanned + rel;
            let line_bytes: Vec<u8> = self.buf.drain(..=pos).collect();
            self.scanned = 0;
            let line = String::from_utf8_lossy(&line_bytes);
            let line = line.trim_end_matches(['\n', '\r']);
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.strip_prefix(' ').unwrap_or(data);
                if !data.is_empty() {
                    out.push(data.to_string());
                }
            }
        }
        // No newline in the remainder: skip re-scanning it on the next chunk
        self.scanned = self.buf.len();
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

    #[test]
    fn multibyte_char_split_across_chunks_stays_intact() {
        let line = "data: {\"text\":\"深色模式\"}\n".as_bytes();
        // Split inside the 3-byte encoding of 色 (bytes 8..11 of the payload)
        for cut in 1..line.len() {
            let mut p = SseParser::default();
            let mut got = p.push(&line[..cut]);
            got.extend(p.push(&line[cut..]));
            assert_eq!(
                got,
                vec!["{\"text\":\"深色模式\"}".to_string()],
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn utf8_split_at_chunk_boundary_no_replacement_chars() {
        let mut p = SseParser::default();
        let bytes = "data: 定\n".as_bytes();
        assert!(p.push(&bytes[..8]).is_empty()); // ends mid-定 (E5 AE 9A)
        let got = p.push(&bytes[8..]);
        assert_eq!(got, vec!["定".to_string()]);
        assert!(!got[0].contains('\u{FFFD}'));
    }
}
