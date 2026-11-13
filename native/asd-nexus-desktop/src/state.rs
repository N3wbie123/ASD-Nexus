//! Pure (UI-agnostic) view state: tabs, model picker, PLAN.md editor with
//! conflict detection, routing editor, kanban grouping.

use std::collections::BTreeMap;

use asd_nexus_client::{BoardTaskItem, GetPlanResponse, SavePlanRequest, SavePlanResponse};
use serde_json::Value;

pub const HARNESSES: [&str; 5] = ["claude-code", "opencode", "codex", "antigravity", "cursor"];

// ---------------------------------------------------------------------------
// Tabs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tab {
    #[default]
    Chat,
    Board,
    Diff,
    Subagents,
    Routing,
    Terminal,
}

impl Tab {
    pub const ALL: [Tab; 6] = [Tab::Chat, Tab::Board, Tab::Diff, Tab::Subagents, Tab::Routing, Tab::Terminal];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Chat => "Chat",
            Tab::Board => "PLAN & Board",
            Tab::Diff => "Worktree Diff",
            Tab::Subagents => "Subagents",
            Tab::Routing => "Routing",
            Tab::Terminal => "Terminal & Resume",
        }
    }

    /// Keyboard shortcut index (Ctrl+1..6).
    pub fn from_index(i: usize) -> Option<Tab> {
        Tab::ALL.get(i).copied()
    }

    pub fn next(self) -> Tab {
        let i = Tab::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Tab::ALL[(i + 1) % Tab::ALL.len()]
    }

    pub fn prev(self) -> Tab {
        let i = Tab::ALL.iter().position(|t| *t == self).unwrap_or(0);
        Tab::ALL[(i + Tab::ALL.len() - 1) % Tab::ALL.len()]
    }
}

// ---------------------------------------------------------------------------
// Model picker
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ModelPicker {
    pub models: BTreeMap<String, Vec<String>>,
    pub filter: String,
    pub selected: Option<String>,
}

impl ModelPicker {
    pub fn set_models(&mut self, models: impl IntoIterator<Item = (String, Vec<String>)>) {
        self.models = models.into_iter().collect();
    }

    /// Models for a harness filtered by case-insensitive substring match.
    pub fn filtered(&self, harness: &str) -> Vec<&str> {
        let needle = self.filter.trim().to_lowercase();
        self.models
            .get(harness)
            .map(|v| {
                v.iter()
                    .filter(|m| needle.is_empty() || m.to_lowercase().contains(&needle))
                    .map(String::as_str)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Drop a selection that is not offered by the new harness.
    pub fn on_harness_changed(&mut self, harness: &str) {
        if let Some(sel) = &self.selected {
            let known = self.models.get(harness).map(|v| v.contains(sel)).unwrap_or(false);
            if !known {
                self.selected = None;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PLAN.md editor with conflict detection (expectedSha256 / expectedMtimeMs)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct PlanConflict {
    /// Latest on-disk content if known (from a reload), else `None`
    /// (server rejected a save with `conflict: true`).
    pub remote_content: Option<String>,
    pub remote_sha256: Option<String>,
    pub remote_mtime_ms: Option<f64>,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct PlanEditor {
    pub path: String,
    pub exists: bool,
    pub session_id: Option<String>,
    /// Content as last loaded/saved (the base for dirty tracking).
    pub base: String,
    /// Editable buffer.
    pub buffer: String,
    pub sha256: Option<String>,
    pub mtime_ms: Option<f64>,
    pub conflict: Option<PlanConflict>,
    pub loaded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanLoadOutcome {
    Loaded,
    Unchanged,
    Conflict,
}

impl PlanEditor {
    pub fn is_dirty(&self) -> bool {
        self.buffer != self.base
    }

    /// Has the file on disk changed relative to our base?
    /// Compares `sha256` first, then `mtimeMs` when no hash is available.
    pub fn remote_changed(&self, resp: &GetPlanResponse) -> bool {
        if !self.loaded {
            return true;
        }
        match (&self.sha256, &resp.sha256) {
            (Some(a), Some(b)) => a != b,
            _ => match (self.mtime_ms, resp.mtime_ms) {
                (Some(a), Some(b)) => (a - b).abs() > 0.5,
                _ => self.base != resp.content,
            },
        }
    }

    /// Apply a GET /api/plan response. Local unsaved edits are never
    /// overwritten silently: a remote change while dirty becomes a conflict.
    pub fn apply_loaded(&mut self, resp: GetPlanResponse, force: bool) -> PlanLoadOutcome {
        let changed = self.remote_changed(&resp);
        if !force && self.loaded && self.is_dirty() {
            if changed {
                self.conflict = Some(PlanConflict {
                    remote_content: Some(resp.content),
                    remote_sha256: resp.sha256,
                    remote_mtime_ms: resp.mtime_ms,
                    reason: "PLAN.md changed on disk while you have unsaved edits".into(),
                });
                return PlanLoadOutcome::Conflict;
            }
            return PlanLoadOutcome::Unchanged;
        }
        if self.loaded && !changed && !force {
            return PlanLoadOutcome::Unchanged;
        }
        self.path = resp.plan_md_path;
        self.exists = resp.exists;
        self.session_id = resp.session_id;
        self.base = resp.content.clone();
        self.buffer = resp.content;
        self.sha256 = resp.sha256;
        self.mtime_ms = resp.mtime_ms;
        self.conflict = None;
        self.loaded = true;
        PlanLoadOutcome::Loaded
    }

    /// Build a save request carrying `expectedSha256` for optimistic concurrency.
    pub fn save_request(&self, project_root: Option<String>, session_id: Option<String>) -> SavePlanRequest {
        SavePlanRequest {
            project_root,
            session_id: session_id.or_else(|| self.session_id.clone()),
            content: self.buffer.clone(),
            expected_sha256: self.sha256.clone(),
            sync_to_db: Some(true),
        }
    }

    /// Pre-flight check before saving: given a fresh GET response, detect a
    /// sha/mtime drift (expectedSha256 / expectedMtimeMs) and raise a conflict.
    pub fn preflight(&mut self, fresh: &GetPlanResponse) -> bool {
        if self.loaded && self.remote_changed(fresh) {
            self.conflict = Some(PlanConflict {
                remote_content: Some(fresh.content.clone()),
                remote_sha256: fresh.sha256.clone(),
                remote_mtime_ms: fresh.mtime_ms,
                reason: format!(
                    "expected sha256 {} / mtimeMs {} but disk has {} / {}",
                    short(self.sha256.as_deref()),
                    fmt_mtime(self.mtime_ms),
                    short(fresh.sha256.as_deref()),
                    fmt_mtime(fresh.mtime_ms)
                ),
            });
            return false;
        }
        true
    }

    pub fn apply_save_result(&mut self, resp: &SavePlanResponse) {
        if resp.success {
            self.base = self.buffer.clone();
            if resp.sha256.is_some() {
                self.sha256 = resp.sha256.clone();
            }
            if resp.mtime_ms.is_some() {
                self.mtime_ms = resp.mtime_ms;
            }
            self.exists = true;
            self.conflict = None;
        } else if resp.conflict == Some(true) {
            self.conflict = Some(PlanConflict {
                remote_content: None,
                remote_sha256: resp.sha256.clone(),
                remote_mtime_ms: resp.mtime_ms,
                reason: resp.error.clone().unwrap_or_else(|| "server rejected save: sha256 mismatch".into()),
            });
        }
    }

    /// Resolve: discard local edits and take the on-disk version.
    pub fn resolve_take_theirs(&mut self) -> bool {
        match self.conflict.take() {
            Some(PlanConflict { remote_content: Some(c), remote_sha256, remote_mtime_ms, .. }) => {
                self.base = c.clone();
                self.buffer = c;
                self.sha256 = remote_sha256;
                self.mtime_ms = remote_mtime_ms;
                true
            }
            other => {
                // Remote content unknown: caller must reload.
                self.conflict = other;
                false
            }
        }
    }

    /// Resolve: keep local edits and adopt the remote hash as the new
    /// expectation so the next save intentionally overwrites.
    pub fn resolve_keep_mine(&mut self) {
        if let Some(c) = self.conflict.take() {
            if c.remote_sha256.is_some() {
                self.sha256 = c.remote_sha256;
            }
            if c.remote_mtime_ms.is_some() {
                self.mtime_ms = c.remote_mtime_ms;
            }
            if let Some(rc) = c.remote_content {
                self.base = rc;
            }
        }
    }
}

fn short(s: Option<&str>) -> String {
    s.map(|s| s.chars().take(10).collect()).unwrap_or_else(|| "-".into())
}

fn fmt_mtime(m: Option<f64>) -> String {
    m.map(|m| format!("{m:.0}")).unwrap_or_else(|| "-".into())
}

// ---------------------------------------------------------------------------
// Routing editor: lightSubagents / agents / categories
// ---------------------------------------------------------------------------

pub const ROUTING_SECTIONS: [&str; 3] = ["lightSubagents", "agents", "categories"];

#[derive(Debug, Clone, Default)]
pub struct RoutingEditor {
    pub raw: Value,
    /// Pretty JSON text per section, in ROUTING_SECTIONS order.
    pub sections: [String; 3],
    pub loaded: bool,
    pub error: Option<String>,
}

impl RoutingEditor {
    pub fn load(&mut self, routing: Value) {
        for (i, key) in ROUTING_SECTIONS.iter().enumerate() {
            let v = routing.get(*key).cloned().unwrap_or(Value::Null);
            self.sections[i] = if v.is_null() {
                String::new()
            } else {
                serde_json::to_string_pretty(&v).unwrap_or_default()
            };
        }
        self.raw = routing;
        self.loaded = true;
        self.error = None;
    }

    /// Merge edited sections back into the full routing document.
    /// Empty text removes the key; invalid JSON yields an error naming the section.
    pub fn build(&self) -> Result<Value, String> {
        let mut root = match &self.raw {
            Value::Object(m) => m.clone(),
            _ => serde_json::Map::new(),
        };
        for (i, key) in ROUTING_SECTIONS.iter().enumerate() {
            let text = self.sections[i].trim();
            if text.is_empty() {
                root.remove(*key);
                continue;
            }
            let v: Value = serde_json::from_str(text).map_err(|e| format!("{key}: invalid JSON: {e}"))?;
            root.insert((*key).to_string(), v);
        }
        Ok(Value::Object(root))
    }

    pub fn is_dirty(&self) -> bool {
        self.build().map(|v| v != self.raw).unwrap_or(true)
    }

    /// Flatten a section into `(key, summary)` rows for a read-only grid.
    pub fn summary(&self, key: &str) -> Vec<(String, String)> {
        match self.raw.get(key) {
            Some(Value::Object(m)) => m.iter().map(|(k, v)| (k.clone(), compact(v))).collect(),
            Some(Value::Array(a)) => a.iter().enumerate().map(|(i, v)| (i.to_string(), compact(v))).collect(),
            Some(v) if !v.is_null() => vec![("value".into(), compact(v))],
            _ => Vec::new(),
        }
    }
}

fn compact(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(m) => {
            let parts: Vec<String> = ["harness", "model", "provider"]
                .iter()
                .filter_map(|k| m.get(*k).and_then(Value::as_str).map(|s| format!("{k}={s}")))
                .collect();
            if parts.is_empty() {
                let s = v.to_string();
                if s.chars().count() > 80 { s.chars().take(80).collect::<String>() + "…" } else { s }
            } else {
                parts.join(" ")
            }
        }
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Kanban grouping
// ---------------------------------------------------------------------------

pub const KANBAN_COLUMNS: [&str; 5] = ["Pending", "In Progress", "Review", "Done", "Failed"];

pub fn kanban_column(status: &str) -> usize {
    match status.to_ascii_lowercase().as_str() {
        "in_progress" | "in-progress" | "running" | "claimed" | "active" => 1,
        "review" | "pending_review" | "awaiting_review" | "needs_review" => 2,
        "done" | "completed" | "complete" | "merged" | "approved" => 3,
        "failed" | "error" | "rejected" | "blocked" | "cancelled" | "killed" => 4,
        _ => 0,
    }
}

pub fn group_kanban(tasks: &[BoardTaskItem]) -> [Vec<&BoardTaskItem>; 5] {
    let mut cols: [Vec<&BoardTaskItem>; 5] = Default::default();
    for t in tasks {
        cols[kanban_column(&t.status)].push(t);
    }
    cols
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(content: &str, sha: &str, mtime: f64) -> GetPlanResponse {
        GetPlanResponse {
            session_id: Some("s".into()),
            plan_md_path: "PLAN.md".into(),
            exists: true,
            content: content.into(),
            mtime_ms: Some(mtime),
            sha256: Some(sha.into()),
        }
    }

    #[test]
    fn tab_cycle() {
        assert_eq!(Tab::Chat.next(), Tab::Board);
        assert_eq!(Tab::Chat.prev(), Tab::Terminal);
        assert_eq!(Tab::from_index(5), Some(Tab::Terminal));
        assert_eq!(Tab::from_index(6), None);
    }

    #[test]
    fn plan_conflict_on_reload_while_dirty() {
        let mut ed = PlanEditor::default();
        assert_eq!(ed.apply_loaded(plan("a", "h1", 1.0), false), PlanLoadOutcome::Loaded);
        ed.buffer.push('!');
        assert_eq!(ed.apply_loaded(plan("a", "h1", 1.0), false), PlanLoadOutcome::Unchanged);
        assert_eq!(ed.apply_loaded(plan("b", "h2", 2.0), false), PlanLoadOutcome::Conflict);
        assert_eq!(ed.buffer, "a!");
        assert!(ed.resolve_take_theirs());
        assert_eq!(ed.buffer, "b");
        assert_eq!(ed.sha256.as_deref(), Some("h2"));
        assert!(!ed.is_dirty());
    }

    #[test]
    fn routing_roundtrip() {
        let mut r = RoutingEditor::default();
        r.load(serde_json::json!({"agents": {"worker": {"harness": "codex"}}, "other": 1}));
        assert!(!r.is_dirty());
        assert_eq!(r.summary("agents"), vec![("worker".to_string(), "harness=codex".to_string())]);
        r.sections[0] = "[\"scout\"]".into();
        let v = r.build().unwrap();
        assert_eq!(v["lightSubagents"][0], "scout");
        assert_eq!(v["other"], 1);
        r.sections[1] = "{bad".into();
        assert!(r.build().unwrap_err().starts_with("agents"));
    }
}
