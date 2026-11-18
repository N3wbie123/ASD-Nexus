//! Pure (UI-agnostic) view state: primary modes, tabs, model picker, slash/@
//! autocomplete helpers, PLAN.md editor with conflict detection, routing
//! editor, kanban grouping.

use std::collections::BTreeMap;

use asd_nexus_client::{BoardTaskItem, FsListEntry, GetPlanResponse, HarnessCommandItem, SavePlanRequest, SavePlanResponse};
use serde_json::Value;

pub const HARNESSES: [&str; 5] = ["claude-code", "opencode", "codex", "antigravity", "cursor"];

// ---------------------------------------------------------------------------
// Primary Mode (Chat vs Dashboard) & Tabs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PrimaryMode {
    #[default]
    Chat,
    Dashboard,
}

impl PrimaryMode {
    pub fn label(self) -> &'static str {
        match self {
            PrimaryMode::Chat => "Chat",
            PrimaryMode::Dashboard => "Dashboard",
        }
    }
}

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
    pub const DASHBOARD: [Tab; 5] = [Tab::Board, Tab::Diff, Tab::Subagents, Tab::Routing, Tab::Terminal];

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

    pub fn is_dashboard(self) -> bool {
        !matches!(self, Tab::Chat)
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
// Slash (`/`) & File (`@`) Autocomplete Helpers
// ---------------------------------------------------------------------------

/// Built-in Nexus slash commands: `(command_without_slash, description)`.
pub const NEXUS_SLASH_COMMANDS: &[(&str, &str)] = &[
    ("cd", "Switch workspace directory (/cd <path> or open folder picker)"),
    ("sessions", "Browse or switch sessions"),
    ("new", "Start a new chat session in active harness"),
    ("models", "Refresh available models for harnesses"),
    ("harness", "Switch active harness (/harness <name>, new chat only)"),
    ("auto-approve", "Toggle auto-approve tool permissions (/auto-approve [on|off])"),
    ("dashboard", "Switch to Dashboard orchestration view"),
    ("theme", "Toggle Dark / Monochrome theme"),
    ("clear", "Clear current chat timeline"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashPopupItem {
    /// Name without leading `/` (e.g. `"cd"`, `"compact"`).
    pub name: String,
    /// Badge text (`"nexus"` or harness name like `"claude-code"`).
    pub badge: String,
    pub description: String,
    pub is_nexus: bool,
}

/// If the composer starts with a slash token currently being typed (no whitespace
/// after the `/token` and single-line), returns the query after `/`.
pub fn extract_slash_query(composer: &str) -> Option<&str> {
    let trimmed = composer.trim_start_matches(' ');
    let rest = trimmed.strip_prefix('/')?;
    if rest.contains(char::is_whitespace) {
        return None;
    }
    Some(rest)
}

/// Build filtered slash autocomplete items combining `[nexus]` built-ins and
/// `[<harness>]` specific commands from `GET /api/harness-commands`.
pub fn build_slash_items(
    query: &str,
    harness: &str,
    harness_cmds: &[HarnessCommandItem],
    allow_harness_switch: bool,
) -> Vec<SlashPopupItem> {
    let needle = query.trim().to_ascii_lowercase();
    let mut out = Vec::new();
    for (name, desc) in NEXUS_SLASH_COMMANDS {
        if *name == "harness" && !allow_harness_switch {
            continue;
        }
        if needle.is_empty()
            || name.to_ascii_lowercase().contains(&needle)
            || desc.to_ascii_lowercase().contains(&needle)
        {
            out.push(SlashPopupItem {
                name: (*name).to_string(),
                badge: "nexus".to_string(),
                description: (*desc).to_string(),
                is_nexus: true,
            });
        }
    }
    for cmd in harness_cmds {
        if !cmd.harness.is_empty() && cmd.harness != harness {
            continue;
        }
        let clean_name = cmd.name.trim_start_matches('/');
        if needle.is_empty()
            || clean_name.to_ascii_lowercase().contains(&needle)
            || cmd.description.to_ascii_lowercase().contains(&needle)
        {
            out.push(SlashPopupItem {
                name: clean_name.to_string(),
                badge: harness.to_string(),
                description: cmd.description.clone(),
                is_nexus: false,
            });
        }
    }
    out
}

/// If the trailing token of `composer` starts with `@` (at the start of input or
/// preceded by whitespace), returns `(byte_index_of_at, query_after_at)`.
pub fn extract_at_query(composer: &str) -> Option<(usize, &str)> {
    if composer.ends_with(char::is_whitespace) {
        return None;
    }
    let last_ws = composer.rfind(char::is_whitespace);
    let token_start = last_ws.map(|idx| idx + composer[idx..].chars().next().unwrap().len_utf8()).unwrap_or(0);
    let token = &composer[token_start..];
    let query = token.strip_prefix('@')?;
    Some((token_start, query))
}

/// Filter workspace file entries by `@query` substring match.
pub fn filter_fs_entries<'a>(entries: &'a [FsListEntry], query: &str, limit: usize) -> Vec<&'a FsListEntry> {
    let needle = query.trim().to_ascii_lowercase();
    entries
        .iter()
        .filter(|e| {
            needle.is_empty()
                || e.path.to_ascii_lowercase().contains(&needle)
                || e.name.to_ascii_lowercase().contains(&needle)
        })
        .take(limit)
        .collect()
}

/// Replace the trailing `@<query>` token in `composer` with `@<rel_path> `.
pub fn replace_at_token(composer: &str, rel_path: &str) -> String {
    if let Some((at_idx, _)) = extract_at_query(composer) {
        format!("{}@{} ", &composer[..at_idx], rel_path)
    } else if composer.is_empty() || composer.ends_with(char::is_whitespace) {
        format!("{composer}@{rel_path} ")
    } else {
        format!("{composer} @{rel_path} ")
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
        for (harness, incoming) in models {
            let existing = self.models.entry(harness).or_default();
            for model in incoming {
                if !existing.iter().any(|m| m == &model) {
                    existing.push(model);
                }
            }
        }
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
        self.filter.clear();
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
    use asd_nexus_client::{HarnessCommandKind, HarnessCommandSource};

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
        assert_eq!(Tab::DASHBOARD.len(), 5);
        assert!(!Tab::Chat.is_dashboard());
        assert!(Tab::Board.is_dashboard());
    }

    #[test]
    fn slash_and_at_autocomplete_parsing() {
        assert_eq!(extract_slash_query("/"), Some(""));
        assert_eq!(extract_slash_query("  /co"), Some("co"));
        assert_eq!(extract_slash_query("/co arg"), None);
        assert_eq!(extract_slash_query("hello /co"), None);

        let h_cmds = vec![HarnessCommandItem {
            name: "compact".into(),
            description: "Compact conversation".into(),
            harness: "claude-code".into(),
            source: HarnessCommandSource::Builtin,
            kind: HarnessCommandKind::Command,
            path: None,
        }];
        let all = build_slash_items("", "claude-code", &h_cmds, true);
        assert!(all.iter().any(|i| i.is_nexus && i.name == "cd" && i.badge == "nexus"));
        assert!(all.iter().any(|i| i.is_nexus && i.name == "harness" && i.badge == "nexus"));
        assert!(all.iter().any(|i| i.is_nexus && i.name == "auto-approve" && i.badge == "nexus"));
        assert!(all.iter().any(|i| !i.is_nexus && i.name == "compact" && i.badge == "claude-code"));

        let locked = build_slash_items("", "claude-code", &h_cmds, false);
        assert!(!locked.iter().any(|i| i.name == "harness"));
        assert!(locked.iter().any(|i| i.name == "auto-approve"));

        assert_eq!(extract_at_query("@src/ma"), Some((0, "src/ma")));
        assert_eq!(extract_at_query("check @Cargo.to"), Some((6, "Cargo.to")));
        assert_eq!(extract_at_query("check @Cargo.toml "), None);
        assert_eq!(replace_at_token("check @Car", "Cargo.toml"), "check @Cargo.toml ");
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
    fn model_picker_merges_models_and_clears_filter_on_harness_change() {
        let mut p = ModelPicker::default();
        p.set_models(vec![
            ("opencode".to_string(), vec!["a".to_string(), "b".to_string()]),
            ("codex".to_string(), vec!["g".to_string()]),
        ]);
        p.set_models(vec![
            ("opencode".to_string(), vec!["b".to_string(), "c".to_string()]),
            ("claude-code".to_string(), vec!["sonnet".to_string()]),
        ]);
        assert_eq!(
            p.models["opencode"],
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        assert_eq!(p.models["codex"], vec!["g".to_string()]);
        assert_eq!(p.models["claude-code"], vec!["sonnet".to_string()]);

        p.filter = "zzz".to_string();
        p.selected = Some("c".to_string());
        p.on_harness_changed("opencode");
        assert!(p.filter.is_empty());
        assert_eq!(p.selected.as_deref(), Some("c"));

        p.on_harness_changed("codex");
        assert!(p.selected.is_none());
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
