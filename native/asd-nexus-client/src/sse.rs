//! Server-Sent Events (SSE) streaming parser and message types.

use crate::models::{OrchestratorEvent, OrchestratorTurnStatus};

#[derive(Debug, Clone, PartialEq)]
pub enum StreamMessage {
    Event(Box<OrchestratorEvent>),
    Error(String),
    Done,
}

/// Incremental SSE byte chunk parser.
///
/// Handles torn buffers, CRLF / LF line endings, multi-line `data:`,
/// SSE comment ignore (`: ...`), and deserialization into `OrchestratorEvent`.
#[derive(Debug, Default)]
pub struct SseParser {
    buffer: Vec<u8>,
    current_event_type: Option<String>,
    current_data_lines: Vec<String>,
    emitted_done: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a byte chunk into the parser and return all parsed messages.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<StreamMessage> {
        self.buffer.extend_from_slice(chunk);
        let mut messages = Vec::new();

        while let Some(pos) = self.buffer.iter().position(|&b| b == b'\n') {
            let mut line_bytes = self.buffer.drain(..=pos).collect::<Vec<u8>>();
            // Remove trailing '\n'
            line_bytes.pop();
            // Remove trailing '\r' if present
            if line_bytes.last() == Some(&b'\r') {
                line_bytes.pop();
            }

            let line = String::from_utf8_lossy(&line_bytes).to_string();
            if let Some(msg) = self.process_line(&line) {
                messages.push(msg);
            }
        }

        messages
    }

    /// Process a single complete line.
    fn process_line(&mut self, line: &str) -> Option<StreamMessage> {
        // SSE comments start with ':'
        if line.starts_with(':') {
            return None;
        }

        // Empty line signals event boundary
        if line.is_empty() {
            return self.dispatch_current_event();
        }

        if let Some(rest) = line.strip_prefix("data:") {
            let val = rest.strip_prefix(' ').unwrap_or(rest);
            self.current_data_lines.push(val.to_string());
        } else if let Some(rest) = line.strip_prefix("event:") {
            let val = rest.strip_prefix(' ').unwrap_or(rest);
            self.current_event_type = Some(val.to_string());
        }

        None
    }

    /// Dispatch pending data as an event.
    fn dispatch_current_event(&mut self) -> Option<StreamMessage> {
        if self.current_data_lines.is_empty() {
            return None;
        }

        let full_data = self.current_data_lines.join("\n");
        self.current_data_lines.clear();
        let _event_type = self.current_event_type.take();

        match serde_json::from_str::<OrchestratorEvent>(&full_data) {
            Ok(event) => {
                let is_terminal = matches!(
                    event.status,
                    Some(OrchestratorTurnStatus::Done) | Some(OrchestratorTurnStatus::Interrupted)
                );
                if is_terminal {
                    self.emitted_done = true;
                }
                Some(StreamMessage::Event(Box::new(event)))
            }
            Err(e) => Some(StreamMessage::Error(format!(
                "Failed to deserialize SSE OrchestratorEvent: {e}; raw data: {full_data}"
            ))),
        }
    }

    /// Finalize parser stream at EOF.
    pub fn finish(&mut self) -> Vec<StreamMessage> {
        let mut messages = Vec::new();

        if !self.buffer.is_empty() {
            let line = String::from_utf8_lossy(&self.buffer).to_string();
            self.buffer.clear();
            if let Some(msg) = self.process_line(&line) {
                messages.push(msg);
            }
        }

        if let Some(msg) = self.dispatch_current_event() {
            messages.push(msg);
        }

        if !self.emitted_done {
            self.emitted_done = true;
            messages.push(StreamMessage::Done);
        }

        messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::OrchestratorEventKind;

    #[test]
    fn test_sse_single_event() {
        let mut parser = SseParser::new();
        let payload = r#"data: {"kind":"thinking","thinking":"Thinking deeply..."}

"#;
        let msgs = parser.feed(payload.as_bytes());
        assert_eq!(msgs.len(), 1);
        match &msgs[0] {
            StreamMessage::Event(evt) => {
                assert_eq!(evt.kind, OrchestratorEventKind::Thinking);
                assert_eq!(evt.thinking.as_deref(), Some("Thinking deeply..."));
            }
            _ => panic!("Expected Event"),
        }
    }

    #[test]
    fn test_sse_torn_chunks() {
        let mut parser = SseParser::new();
        let chunk1 = b"data: {\"kind\":\"te";
        let chunk2 = b"xt\",\"text\":\"Hel";
        let chunk3 = b"lo!\"}\n";
        let chunk4 = b"\n";

        assert!(parser.feed(chunk1).is_empty());
        assert!(parser.feed(chunk2).is_empty());
        assert!(parser.feed(chunk3).is_empty());
        let msgs = parser.feed(chunk4);
        assert_eq!(msgs.len(), 1);
        match &msgs[0] {
            StreamMessage::Event(evt) => {
                assert_eq!(evt.kind, OrchestratorEventKind::Text);
                assert_eq!(evt.text.as_deref(), Some("Hello!"));
            }
            _ => panic!("Expected Event"),
        }
    }

    #[test]
    fn test_sse_comments_and_keepalive() {
        let mut parser = SseParser::new();
        let chunk = b": keepalive ping\n\n: another comment\n\ndata: {\"kind\":\"status\",\"status\":\"running\"}\n\n";
        let msgs = parser.feed(chunk);
        assert_eq!(msgs.len(), 1);
        match &msgs[0] {
            StreamMessage::Event(evt) => {
                assert_eq!(evt.kind, OrchestratorEventKind::Status);
                assert_eq!(evt.status, Some(OrchestratorTurnStatus::Running));
            }
            _ => panic!("Expected Event"),
        }
    }

    #[test]
    fn test_sse_multi_line_data() {
        let mut parser = SseParser::new();
        let chunk = b"data: {\"kind\":\"text\",\ndata: \"text\":\"line 1\\nline 2\"}\n\n";
        let msgs = parser.feed(chunk);
        assert_eq!(msgs.len(), 1);
        match &msgs[0] {
            StreamMessage::Event(evt) => {
                assert_eq!(evt.kind, OrchestratorEventKind::Text);
                assert_eq!(evt.text.as_deref(), Some("line 1\nline 2"));
            }
            _ => panic!("Expected Event"),
        }
    }

    #[test]
    fn test_sse_invalid_json_produces_error() {
        let mut parser = SseParser::new();
        let chunk = b"data: not valid json\n\n";
        let msgs = parser.feed(chunk);
        assert_eq!(msgs.len(), 1);
        match &msgs[0] {
            StreamMessage::Error(err) => {
                assert!(err.contains("Failed to deserialize"));
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn test_sse_finish_emits_done() {
        let mut parser = SseParser::new();
        let chunk = b"data: {\"kind\":\"status\",\"status\":\"running\"}\n\n";
        let msgs = parser.feed(chunk);
        assert_eq!(msgs.len(), 1);
        let finish_msgs = parser.finish();
        assert_eq!(finish_msgs.len(), 1);
        assert_eq!(finish_msgs[0], StreamMessage::Done);
    }
}
