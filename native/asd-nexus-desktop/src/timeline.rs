//! 4-category streaming chat timeline (thinking / tool_use / tool_result / text,
//! plus user / error / system rows) and the SSE `StreamMessage` reducer.

use asd_nexus_client::{
    NormalizedTranscriptMessage, OrchestratorEvent, OrchestratorEventKind, OrchestratorTurnStatus,
    StreamMessage, TokenUsagePayload, TranscriptCategory,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    User,
    Thinking,
    ToolUse,
    ToolResult,
    Text,
    Error,
    System,
}

impl EntryKind {
    /// The four streamed categories render as collapsible sections
    /// (text is collapsible too, but open by default).
    pub fn collapsible(self) -> bool {
        matches!(self, EntryKind::Thinking | EntryKind::ToolUse | EntryKind::ToolResult | EntryKind::Text)
    }

    pub fn default_open(self) -> bool {
        !matches!(self, EntryKind::Thinking | EntryKind::ToolUse | EntryKind::ToolResult)
    }

    pub fn label(self) -> &'static str {
        match self {
            EntryKind::User => "YOU",
            EntryKind::Thinking => "THINKING",
            EntryKind::ToolUse => "TOOL USE",
            EntryKind::ToolResult => "TOOL RESULT",
            EntryKind::Text => "ASSISTANT",
            EntryKind::Error => "ERROR",
            EntryKind::System => "SYSTEM",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Stable id (monotonic) used as egui id salt so collapse state survives appends.
    pub id: u64,
    pub kind: EntryKind,
    pub title: String,
    pub body: String,
    pub is_error: bool,
    pub tool_id: Option<String>,
    /// tool_use awaiting its tool_result (spinner).
    pub pending: bool,
}

impl Entry {
    /// Single-line compact summary of `body` for inline tool/thinking headers.
    pub fn compact_summary(&self, max_chars: usize) -> String {
        let mut s = String::new();
        for tok in self.body.split_whitespace() {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str(tok);
            if s.chars().count() >= max_chars {
                break;
            }
        }
        if s.chars().count() > max_chars {
            let mut out: String = s.chars().take(max_chars).collect();
            out.push('…');
            out
        } else {
            s
        }
    }

    /// Antigravity-style header label for a Thinking entry (`"Thought for …"`).
    pub fn thought_header(&self) -> String {
        let chars = self.body.chars().count();
        let preview = self.compact_summary(56);
        if preview.is_empty() {
            format!("Thought for {chars} chars")
        } else {
            format!("Thought for {chars} chars · {preview}")
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub total: u64,
    pub cost: f64,
}

impl Usage {
    pub fn merge(&mut self, p: &TokenUsagePayload) {
        if let Some(v) = p.input_tokens {
            self.input = v;
        }
        if let Some(v) = p.output_tokens {
            self.output = v;
        }
        self.total = p.total_tokens.unwrap_or(self.input + self.output);
        if let Some(v) = p.cost_usd {
            self.cost = v;
        }
    }

    pub fn add(&mut self, o: &Usage) {
        self.input += o.input;
        self.output += o.output;
        self.total += o.total;
        self.cost += o.cost;
    }

    pub fn readout(&self) -> String {
        format!(
            "in {} · out {} · total {} · ${:.4}",
            self.input, self.output, self.total, self.cost
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct Timeline {
    pub entries: Vec<Entry>,
    pub streaming: bool,
    pub turn_id: Option<String>,
    pub harness_session_id: Option<String>,
    pub turn_usage: Usage,
    pub total_usage: Usage,
    pub turns: u32,
    next_id: u64,
}

impl Timeline {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.streaming = false;
        self.turn_id = None;
        self.turn_usage = Usage::default();
        self.total_usage = Usage::default();
        self.turns = 0;
    }

    fn make(&mut self, kind: EntryKind, title: impl Into<String>, body: impl Into<String>) -> Entry {
        self.next_id += 1;
        Entry {
            id: self.next_id,
            kind,
            title: title.into(),
            body: body.into(),
            is_error: kind == EntryKind::Error,
            tool_id: None,
            pending: false,
        }
    }

    pub fn push(&mut self, kind: EntryKind, title: impl Into<String>, body: impl Into<String>) -> &mut Entry {
        let e = self.make(kind, title, body);
        self.entries.push(e);
        self.entries.last_mut().unwrap()
    }

    pub fn count(&self, kind: EntryKind) -> usize {
        self.entries.iter().filter(|e| e.kind == kind).count()
    }

    pub fn begin_turn(&mut self, prompt: &str) {
        self.push(EntryKind::User, "", prompt);
        self.streaming = true;
        self.turn_id = None;
        self.turn_usage = Usage::default();
    }

    /// Finalize the current turn (idempotent).
    pub fn end_turn(&mut self, note: Option<&str>) {
        if !self.streaming {
            return;
        }
        self.streaming = false;
        self.turns += 1;
        let u = self.turn_usage;
        self.total_usage.add(&u);
        for e in self.entries.iter_mut().filter(|e| e.pending) {
            e.pending = false;
        }
        if let Some(n) = note {
            self.push(EntryKind::System, "", n);
        }
    }

    /// Append streaming text to the last entry of the same kind if it is the
    /// tail of the timeline, otherwise start a new entry.
    fn append(&mut self, kind: EntryKind, chunk: &str) {
        if chunk.is_empty() {
            return;
        }
        if let Some(last) = self.entries.last_mut() {
            if last.kind == kind && self.streaming {
                last.body.push_str(chunk);
                return;
            }
        }
        self.push(kind, "", chunk);
    }

    /// Reduce one SSE stream message into the timeline.
    pub fn apply(&mut self, msg: &StreamMessage) {
        match msg {
            StreamMessage::Event(evt) => self.apply_event(evt),
            StreamMessage::Error(e) => {
                self.push(EntryKind::Error, "stream", e.clone());
            }
            StreamMessage::Done => self.end_turn(None),
        }
    }

    pub fn apply_event(&mut self, evt: &OrchestratorEvent) {
        if self.turn_id.is_none() {
            self.turn_id = evt.turn_id.clone();
        }
        if let Some(sid) = &evt.session_id {
            self.harness_session_id = Some(sid.clone());
        }
        match evt.kind {
            OrchestratorEventKind::Thinking => {
                let t = evt.thinking.as_deref().or(evt.text.as_deref()).unwrap_or("");
                self.append(EntryKind::Thinking, t);
            }
            OrchestratorEventKind::Text => {
                self.append(EntryKind::Text, evt.text.as_deref().unwrap_or(""));
            }
            OrchestratorEventKind::ToolUse => {
                let (name, id, input) = match &evt.tool_use {
                    Some(t) => (t.name.clone(), t.id.clone(), t.input.clone().unwrap_or_default()),
                    None => ("tool".into(), None, evt.text.clone().unwrap_or_default()),
                };
                let e = self.push(EntryKind::ToolUse, name, input);
                e.tool_id = id;
                e.pending = true;
            }
            OrchestratorEventKind::ToolResult => {
                let (name, id, out, err) = match &evt.tool_result {
                    Some(r) => (r.name.clone(), r.tool_use_id.clone(), r.output.clone(), r.is_error.unwrap_or(false)),
                    None => (None, None, evt.text.clone().unwrap_or_default(), false),
                };
                // Resolve the matching tool_use (by id, else the most recent pending one).
                let idx = self
                    .entries
                    .iter()
                    .rposition(|e| e.kind == EntryKind::ToolUse && e.pending && (id.is_none() || e.tool_id == id));
                let mut title = name.unwrap_or_default();
                if let Some(i) = idx {
                    self.entries[i].pending = false;
                    if title.is_empty() {
                        title = self.entries[i].title.clone();
                    }
                }
                let e = self.push(EntryKind::ToolResult, title, out);
                e.tool_id = id;
                e.is_error = err;
            }
            OrchestratorEventKind::Usage => {
                if let Some(u) = &evt.usage {
                    self.turn_usage.merge(u);
                }
            }
            OrchestratorEventKind::Error => {
                let msg = evt.error.clone().or_else(|| evt.text.clone()).unwrap_or_else(|| "unknown error".into());
                self.push(EntryKind::Error, evt.harness.clone().unwrap_or_default(), msg);
            }
            OrchestratorEventKind::Status => {}
            OrchestratorEventKind::PermissionRequest => {
                if let Some(perm) = &evt.permission_request {
                    self.push(
                        EntryKind::System,
                        "permission",
                        format!("Permission requested: {}", perm.tool_name),
                    );
                }
            }
        }
        if let Some(u) = &evt.usage {
            if evt.kind != OrchestratorEventKind::Usage {
                self.turn_usage.merge(u);
            }
        }
        match evt.status {
            Some(OrchestratorTurnStatus::Done) => self.end_turn(None),
            Some(OrchestratorTurnStatus::Interrupted) => self.end_turn(Some("turn interrupted")),
            _ => {}
        }
    }

    /// Replace contents with a restored 4-category transcript.
    pub fn load_transcript(&mut self, messages: &[NormalizedTranscriptMessage]) {
        self.clear();
        for m in messages {
            let cat = m.category.unwrap_or(match m.role.as_str() {
                "user" => TranscriptCategory::User,
                "system" => TranscriptCategory::System,
                _ => TranscriptCategory::Assistant,
            });
            match cat {
                TranscriptCategory::User => {
                    self.push(EntryKind::User, "", m.text.clone());
                }
                TranscriptCategory::Assistant => {
                    if let Some(t) = m.thinking.as_ref().filter(|t| !t.is_empty()) {
                        self.push(EntryKind::Thinking, "", t.clone());
                    }
                    if !m.text.is_empty() {
                        self.push(EntryKind::Text, "", m.text.clone());
                    }
                }
                TranscriptCategory::Thinking => {
                    let body = m.thinking.clone().unwrap_or_else(|| m.text.clone());
                    self.push(EntryKind::Thinking, "", body);
                }
                TranscriptCategory::ToolUse => {
                    let name = m
                        .tool_use
                        .as_ref()
                        .map(|t| t.name.clone())
                        .or_else(|| m.tool_name.clone())
                        .unwrap_or_else(|| "tool".into());
                    let input = m.tool_use.as_ref().and_then(|t| t.input.clone()).unwrap_or_else(|| m.text.clone());
                    let id = m.tool_use.as_ref().and_then(|t| t.id.clone());
                    let e = self.push(EntryKind::ToolUse, name, input);
                    e.tool_id = id;
                }
                TranscriptCategory::ToolResult => {
                    let (out, err, id) = match &m.tool_result {
                        Some(r) => (r.output.clone(), r.is_error.unwrap_or(false), r.tool_use_id.clone()),
                        None => (m.text.clone(), false, None),
                    };
                    let name = m.tool_name.clone().unwrap_or_default();
                    let e = self.push(EntryKind::ToolResult, name, out);
                    e.is_error = err;
                    e.tool_id = id;
                }
                TranscriptCategory::System => {
                    self.push(EntryKind::System, "", m.text.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use asd_nexus_client::SseParser;

    #[test]
    fn reduces_sse_bytes_into_timeline() {
        let mut p = SseParser::new();
        let mut tl = Timeline::default();
        tl.begin_turn("hi");
        let raw = concat!(
            "data: {\"kind\":\"status\",\"status\":\"running\",\"turnId\":\"t1\"}\n\n",
            "data: {\"kind\":\"thinking\",\"thinking\":\"a\"}\n\n",
            "data: {\"kind\":\"thinking\",\"thinking\":\"b\"}\n\n",
            "data: {\"kind\":\"tool_use\",\"toolUse\":{\"name\":\"read\",\"id\":\"c1\",\"input\":\"{}\"}}\n\n",
            "data: {\"kind\":\"tool_result\",\"toolResult\":{\"toolUseId\":\"c1\",\"output\":\"ok\",\"isError\":true}}\n\n",
            "data: {\"kind\":\"text\",\"text\":\"Hel\"}\n\n",
            "data: {\"kind\":\"text\",\"text\":\"lo\"}\n\n",
            "data: {\"kind\":\"usage\",\"usage\":{\"inputTokens\":3,\"outputTokens\":4,\"costUsd\":0.5}}\n\n",
            "data: {\"kind\":\"status\",\"status\":\"done\"}\n\n",
        );
        for m in p.feed(raw.as_bytes()) {
            tl.apply(&m);
        }
        assert_eq!(tl.turn_id.as_deref(), Some("t1"));
        assert_eq!(tl.count(EntryKind::Thinking), 1);
        assert_eq!(tl.entries[1].body, "ab");
        assert!(tl.entries[1].thought_header().starts_with("Thought for 2 chars"));
        assert!(!tl.entries[2].pending);
        assert_eq!(tl.entries[3].title, "read");
        assert!(tl.entries[3].is_error);
        assert_eq!(tl.entries[4].body, "Hello");
        assert!(!tl.streaming);
        assert_eq!(tl.total_usage.total, 7);
        assert!((tl.total_usage.cost - 0.5).abs() < 1e-9);
        // Done after terminal status is idempotent.
        tl.apply(&StreamMessage::Done);
        assert_eq!(tl.turns, 1);
    }
}
