//! Chat timeline (thinking / tool_use / tool_result / text / shell / user / error / system)
//! and the SSE event reducer.

use asd_nexus_client::{
    NormalizedTranscriptMessage, OrchestratorEventKind, OrchestratorTurnStatus, StreamMessage,
    TokenUsagePayload, TranscriptCategory,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    User,
    Thinking,
    ToolUse,
    ToolResult,
    Text,
    Shell,
    Error,
    System,
}

impl EntryKind {
    pub fn collapsible(self) -> bool {
        matches!(self, EntryKind::Thinking | EntryKind::ToolUse | EntryKind::ToolResult | EntryKind::Shell)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub kind: EntryKind,
    /// Tool name for tool_use / tool_result, or command for shell.
    pub title: String,
    pub body: String,
    pub collapsed: bool,
    pub is_error: bool,
    pub tool_id: Option<String>,
}

impl Entry {
    pub fn new(kind: EntryKind, body: impl Into<String>) -> Self {
        Self {
            kind,
            title: String::new(),
            body: body.into(),
            collapsed: matches!(kind, EntryKind::Thinking | EntryKind::ToolUse | EntryKind::ToolResult),
            is_error: kind == EntryKind::Error,
            tool_id: None,
        }
    }

    pub fn titled(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn shell(cmd: impl Into<String>, output: impl Into<String>, exit_code: i32) -> Self {
        let cmd = cmd.into();
        Self {
            kind: EntryKind::Shell,
            title: format!("!{cmd} [exit {exit_code}]"),
            body: output.into(),
            collapsed: false,
            is_error: exit_code != 0,
            tool_id: None,
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
}

#[derive(Debug, Clone, Default)]
pub struct Timeline {
    pub entries: Vec<Entry>,
    pub streaming: bool,
    pub turn_id: Option<String>,
    pub turn_usage: Usage,
    pub total_usage: Usage,
}

impl Timeline {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.streaming = false;
        self.turn_id = None;
        self.turn_usage = Usage::default();
    }

    pub fn push(&mut self, e: Entry) {
        self.entries.push(e);
    }

    pub fn begin_turn(&mut self, prompt: &str) {
        self.push(Entry::new(EntryKind::User, prompt));
        self.streaming = true;
        self.turn_id = None;
        self.turn_usage = Usage::default();
    }

    /// Finalize the turn (idempotent).
    pub fn end_turn(&mut self, note: Option<&str>) {
        if !self.streaming {
            return;
        }
        self.streaming = false;
        let u = self.turn_usage;
        self.total_usage.add(&u);
        if let Some(n) = note {
            self.push(Entry::new(EntryKind::System, n));
        }
    }

    fn append_or_push(&mut self, kind: EntryKind, chunk: &str) {
        if let Some(last) = self.entries.last_mut() {
            if last.kind == kind {
                last.body.push_str(chunk);
                return;
            }
        }
        self.push(Entry::new(kind, chunk));
    }

    /// Reduce one SSE stream message into the timeline.
    pub fn apply(&mut self, msg: &StreamMessage) {
        match msg {
            StreamMessage::Done => self.end_turn(None),
            StreamMessage::Error(e) => self.push(Entry::new(EntryKind::Error, e.clone())),
            StreamMessage::Event(evt) => {
                if self.turn_id.is_none() {
                    self.turn_id = evt.turn_id.clone();
                }
                match evt.kind {
                    OrchestratorEventKind::Thinking => {
                        let t = evt.thinking.as_deref().or(evt.text.as_deref()).unwrap_or("");
                        self.append_or_push(EntryKind::Thinking, t);
                    }
                    OrchestratorEventKind::Text => {
                        self.append_or_push(EntryKind::Text, evt.text.as_deref().unwrap_or(""));
                    }
                    OrchestratorEventKind::ToolUse => {
                        let (name, input, id) = evt
                            .tool_use
                            .as_ref()
                            .map(|t| (t.name.clone(), t.input.clone().unwrap_or_default(), t.id.clone()))
                            .unwrap_or_else(|| ("tool".into(), String::new(), None));
                        let mut e = Entry::new(EntryKind::ToolUse, input).titled(name);
                        e.tool_id = id;
                        self.push(e);
                    }
                    OrchestratorEventKind::ToolResult => {
                        let r = evt.tool_result.clone().unwrap_or_default();
                        let name = r
                            .name
                            .clone()
                            .or_else(|| self.tool_name_for(r.tool_use_id.as_deref()))
                            .unwrap_or_else(|| "result".into());
                        let mut e = Entry::new(EntryKind::ToolResult, r.output).titled(name);
                        e.is_error = r.is_error.unwrap_or(false);
                        e.tool_id = r.tool_use_id;
                        self.push(e);
                    }
                    OrchestratorEventKind::Usage => {
                        if let Some(u) = &evt.usage {
                            self.turn_usage.merge(u);
                        }
                    }
                    OrchestratorEventKind::Error => {
                        let m = evt.error.clone().or(evt.text.clone()).unwrap_or_else(|| "error".into());
                        self.push(Entry::new(EntryKind::Error, m));
                    }
                    OrchestratorEventKind::Status => {}
                    OrchestratorEventKind::PermissionRequest => {
                        if let Some(p) = &evt.permission_request {
                            self.push(Entry::new(
                                EntryKind::System,
                                format!("permission requested: {}", p.tool_name),
                            ));
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
        }
    }

    fn tool_name_for(&self, id: Option<&str>) -> Option<String> {
        let id = id?;
        self.entries
            .iter()
            .rev()
            .find(|e| e.kind == EntryKind::ToolUse && e.tool_id.as_deref() == Some(id))
            .map(|e| e.title.clone())
    }

    /// True if a tool_use has no matching tool_result yet (spinner).
    pub fn tool_pending(&self, idx: usize) -> bool {
        let e = &self.entries[idx];
        if e.kind != EntryKind::ToolUse || !self.streaming {
            return false;
        }
        !self.entries[idx + 1..].iter().any(|r| {
            r.kind == EntryKind::ToolResult && (e.tool_id.is_none() || r.tool_id == e.tool_id)
        })
    }

    pub fn toggle(&mut self, idx: usize) {
        if let Some(e) = self.entries.get_mut(idx) {
            if e.kind.collapsible() {
                e.collapsed = !e.collapsed;
            }
        }
    }

    /// Collapse all if any is expanded, otherwise expand all.
    pub fn toggle_all(&mut self) {
        let any_open = self.entries.iter().any(|e| e.kind.collapsible() && !e.collapsed);
        for e in self.entries.iter_mut().filter(|e| e.kind.collapsible()) {
            e.collapsed = any_open;
        }
    }

    /// Replace the timeline with a restored transcript.
    pub fn load_transcript(&mut self, msgs: &[NormalizedTranscriptMessage]) {
        self.clear();
        for m in msgs {
            let cat = m.category.unwrap_or(match m.role.as_str() {
                "user" => TranscriptCategory::User,
                "system" => TranscriptCategory::System,
                _ => TranscriptCategory::Assistant,
            });
            match cat {
                TranscriptCategory::User => self.push(Entry::new(EntryKind::User, m.text.clone())),
                TranscriptCategory::Assistant => {
                    if let Some(t) = m.thinking.as_deref().filter(|t| !t.is_empty()) {
                        self.push(Entry::new(EntryKind::Thinking, t));
                    }
                    if !m.text.is_empty() {
                        self.push(Entry::new(EntryKind::Text, m.text.clone()));
                    }
                }
                TranscriptCategory::Thinking => {
                    let t = m.thinking.clone().unwrap_or_else(|| m.text.clone());
                    self.push(Entry::new(EntryKind::Thinking, t));
                }
                TranscriptCategory::ToolUse => {
                    let name = m
                        .tool_use
                        .as_ref()
                        .map(|t| t.name.clone())
                        .or(m.tool_name.clone())
                        .unwrap_or_else(|| "tool".into());
                    let input = m
                        .tool_use
                        .as_ref()
                        .and_then(|t| t.input.clone())
                        .unwrap_or_else(|| m.text.clone());
                    let mut e = Entry::new(EntryKind::ToolUse, input).titled(name);
                    e.tool_id = m.tool_use.as_ref().and_then(|t| t.id.clone());
                    self.push(e);
                }
                TranscriptCategory::ToolResult => {
                    let r = m.tool_result.clone();
                    let name = r
                        .as_ref()
                        .and_then(|r| r.name.clone())
                        .or(m.tool_name.clone())
                        .unwrap_or_else(|| "result".into());
                    let out = r.as_ref().map(|r| r.output.clone()).unwrap_or_else(|| m.text.clone());
                    let mut e = Entry::new(EntryKind::ToolResult, out).titled(name);
                    e.is_error = r.as_ref().and_then(|r| r.is_error).unwrap_or(false);
                    e.tool_id = r.and_then(|r| r.tool_use_id);
                    self.push(e);
                }
                TranscriptCategory::System => self.push(Entry::new(EntryKind::System, m.text.clone())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use asd_nexus_client::SseParser;

    fn feed(t: &mut Timeline, raw: &str) {
        let mut p = SseParser::new();
        let mut msgs = p.feed(raw.as_bytes());
        msgs.extend(p.finish());
        for m in &msgs {
            t.apply(m);
        }
    }

    #[test]
    fn sse_reduction_four_categories() {
        let mut t = Timeline::default();
        t.begin_turn("hi");
        feed(
            &mut t,
            concat!(
                "data: {\"kind\":\"status\",\"status\":\"running\",\"turnId\":\"t1\"}\n\n",
                "data: {\"kind\":\"thinking\",\"thinking\":\"plan \"}\n\n",
                "data: {\"kind\":\"thinking\",\"thinking\":\"more\"}\n\n",
                "data: {\"kind\":\"tool_use\",\"toolUse\":{\"name\":\"read\",\"id\":\"c1\",\"input\":\"{}\"}}\n\n",
                "data: {\"kind\":\"tool_result\",\"toolResult\":{\"toolUseId\":\"c1\",\"output\":\"boom\",\"isError\":true}}\n\n",
                "data: {\"kind\":\"text\",\"text\":\"Hel\"}\n\n",
                "data: {\"kind\":\"text\",\"text\":\"lo\"}\n\n",
                "data: {\"kind\":\"usage\",\"usage\":{\"inputTokens\":10,\"outputTokens\":5,\"costUsd\":0.5}}\n\n",
                "data: {\"kind\":\"status\",\"status\":\"done\"}\n\n",
            ),
        );
        let kinds: Vec<_> = t.entries.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![
                EntryKind::User,
                EntryKind::Thinking,
                EntryKind::ToolUse,
                EntryKind::ToolResult,
                EntryKind::Text,
            ]
        );
        assert_eq!(t.entries[1].body, "plan more");
        assert_eq!(t.entries[3].title, "read");
        assert!(t.entries[3].is_error);
        assert_eq!(t.entries[4].body, "Hello");
        assert!(!t.streaming);
        assert_eq!(t.turn_id.as_deref(), Some("t1"));
        assert_eq!(t.total_usage.total, 15);
        assert!((t.total_usage.cost - 0.5).abs() < 1e-9);
    }

    #[test]
    fn error_and_done_are_idempotent() {
        let mut t = Timeline::default();
        t.begin_turn("x");
        t.apply(&StreamMessage::Error("bad".into()));
        t.apply(&StreamMessage::Done);
        t.apply(&StreamMessage::Done);
        assert!(!t.streaming);
        assert_eq!(t.entries.len(), 2);
        assert_eq!(t.entries[1].kind, EntryKind::Error);
    }

    #[test]
    fn collapse_toggles() {
        let mut t = Timeline::default();
        t.push(Entry::new(EntryKind::Thinking, "a"));
        t.push(Entry::new(EntryKind::Text, "b"));
        assert!(t.entries[0].collapsed);
        t.toggle(0);
        assert!(!t.entries[0].collapsed);
        t.toggle(1);
        assert!(!t.entries[1].collapsed);
        t.toggle_all();
        assert!(t.entries[0].collapsed);
        t.toggle_all();
        assert!(!t.entries[0].collapsed);
    }
}
