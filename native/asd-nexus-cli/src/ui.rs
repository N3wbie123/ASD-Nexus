//! Monochrome ratatui rendering.

use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState, Wrap};
use ratatui::Frame;

use crate::app::{terminal_name, App, BoardPane, Focus, Overlay, HARNESSES, TERMINALS};
use crate::theme;
use crate::timeline::{Entry, EntryKind};

const SPINNER: [char; 4] = ['|', '/', '-', '\\'];

fn block(title: &str, focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(if focused { BorderType::Thick } else { BorderType::Plain })
        .border_style(theme::border(focused))
        .title(Span::styled(format!(" {title} "), if focused { theme::selected() } else { theme::title() }))
        .style(theme::base())
}

/// Wrap text to `width` display columns (char-level, unicode-width aware).
pub fn wrap(s: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for raw in s.split('\n') {
        let mut cur = String::new();
        let mut w = 0;
        for ch in raw.chars() {
            let cw = Span::raw(ch.to_string()).width();
            if w + cw > width && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
                w = 0;
            }
            cur.push(ch);
            w += cw;
        }
        out.push(cur);
    }
    out
}

fn first_line(s: &str, max: usize) -> String {
    let l = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    let mut out: String = l.chars().take(max).collect();
    if l.chars().count() > max || s.lines().filter(|l| !l.trim().is_empty()).count() > 1 {
        out.push('…');
    }
    out
}

fn fmt_num(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn short(id: &str, n: usize) -> String {
    if id.chars().count() <= n {
        id.to_string()
    } else {
        format!("{}…", id.chars().take(n).collect::<String>())
    }
}

fn centered(area: Rect, pct_x: u16, pct_y: u16) -> Rect {
    let [v] = Layout::vertical([Constraint::Percentage(pct_y)]).flex(Flex::Center).areas(area);
    let [h] = Layout::horizontal([Constraint::Percentage(pct_x)]).flex(Flex::Center).areas(v);
    h
}

fn popup(f: &mut Frame, area: Rect, title: &str) -> Rect {
    f.render_widget(Clear, area);
    let b = block(title, true);
    let inner = b.inner(area);
    f.render_widget(b, area);
    inner
}

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    f.render_widget(Block::default().style(theme::base()), area);
    let [head, body, foot] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(5), Constraint::Length(2)]).areas(area);
    draw_header(f, head, app);
    let [left, right] = Layout::horizontal([Constraint::Length(38), Constraint::Min(20)]).areas(body);
    let [proj, sess] = Layout::vertical([Constraint::Percentage(28), Constraint::Min(4)]).areas(left);
    draw_projects(f, proj, app);
    draw_sessions(f, sess, app);
    let [tl, input] = Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).areas(right);
    draw_timeline(f, tl, app);
    draw_input(f, input, app);
    draw_footer(f, foot, app);

    if let Some(o) = app.overlay {
        let a = centered(area, 86, 84);
        match o {
            Overlay::Board => draw_board(f, a, app),
            Overlay::Subagents => draw_subagents(f, a, app),
            Overlay::Diff => draw_diff(f, a, app),
            Overlay::Routing => draw_routing(f, a, app),
            Overlay::Terminal => draw_terminal(f, centered(area, 70, 50), app),
            Overlay::ModelPicker => draw_picker(f, centered(area, 60, 70), app),
            Overlay::Help => draw_help(f, a),
        }
    }
    if let Some(p) = &app.prompt {
        let a = centered(area, 60, 20);
        let inner = popup(f, a, p.purpose.label());
        let t = Paragraph::new(vec![
            Line::styled(format!("{}█", p.buffer), theme::bold()),
            Line::styled("Enter = submit · Esc = cancel", theme::faint()),
        ])
        .wrap(Wrap { trim: false });
        f.render_widget(t, inner);
    }
    if let Some((q, _)) = &app.confirm {
        let a = centered(area, 50, 20);
        let inner = popup(f, a, "CONFIRM");
        f.render_widget(
            Paragraph::new(vec![
                Line::styled(q.clone(), theme::bold()),
                Line::styled("y = yes · any other key = cancel", theme::faint()),
            ])
            .wrap(Wrap { trim: false }),
            inner,
        );
    }
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let sess = app.active_session_id().unwrap_or_else(|| "—".into());
    let state = if app.is_streaming() {
        format!(" {} STREAMING ", SPINNER[(app.timeline.entries.len()) % 4])
    } else {
        " IDLE ".into()
    };
    let line = Line::from(vec![
        Span::styled(" ASD-NEXUS ", theme::badge()),
        Span::styled(format!(" {} ", app.project_root), theme::header()),
        Span::styled(format!("│ {} ", app.harness), theme::header()),
        Span::styled(format!("│ {} ", app.model.as_deref().unwrap_or("(default)")), theme::header()),
        Span::styled(format!("│ session {} ", short(&sess, 24)), theme::header()),
        Span::styled(state, if app.is_streaming() { theme::badge() } else { theme::header() }),
    ]);
    f.render_widget(Paragraph::new(line).style(theme::header()), area);
}

fn draw_projects(f: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Focus::Projects && app.overlay.is_none();
    let items: Vec<ListItem> = app
        .projects
        .iter()
        .map(|p| {
            let mark = if p.project_root == app.project_root { "● " } else { "  " };
            let n = p.session_count.map(|c| format!(" ({c})")).unwrap_or_default();
            ListItem::new(format!("{mark}{}{n}", p.project_root))
        })
        .collect();
    let mut st = ListState::default().with_selected((!items.is_empty()).then_some(app.project_sel));
    let list = List::new(items)
        .block(block("PROJECTS", focused))
        .style(theme::base())
        .highlight_style(if focused { theme::selected() } else { theme::bold() });
    f.render_stateful_widget(list, area, &mut st);
}

fn draw_sessions(f: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Focus::Sessions && app.overlay.is_none();
    let filter = app.harness_filter.map(|i| HARNESSES[i]).unwrap_or("all");
    let mut title = format!("SESSIONS [{filter}]");
    if !app.search.is_empty() {
        title.push_str(&format!(" /{}", app.search));
    }
    let idx = app.filtered_sessions();
    let items: Vec<ListItem> = idx
        .iter()
        .map(|&i| {
            let s = &app.sessions[i];
            let fg = s.asd_session_id.is_some() && s.asd_session_id == app.foreground_id;
            let promoted = if fg { "★" } else if s.is_promoted_to_asd { "◆" } else { "◇" };
            let phase = s.asd_phase.as_deref().map(|p| format!(" [{p}]")).unwrap_or_default();
            let head = Line::from(vec![
                Span::styled(format!("{promoted} {:<11}", short(&s.harness, 11)), theme::bold()),
                Span::styled(format!(" {}{phase}", short(&s.harness_session_id, 12)), theme::muted()),
            ]);
            let sum = Line::styled(
                format!("   {}", first_line(s.summary.as_deref().unwrap_or("(no summary)"), 32)),
                theme::faint(),
            );
            ListItem::new(vec![head, sum])
        })
        .collect();
    let mut st = ListState::default().with_selected((!items.is_empty()).then_some(app.session_sel));
    let list = List::new(items)
        .block(block(&title, focused))
        .style(theme::base())
        .highlight_style(if focused { theme::selected() } else { theme::bold() });
    f.render_stateful_widget(list, area, &mut st);
}

/// Render timeline entries into wrapped lines; returns (lines, start line of each entry).
pub fn timeline_lines(app: &App, width: usize, focused: bool) -> (Vec<Line<'static>>, Vec<usize>) {
    let mut lines = Vec::new();
    let mut starts = Vec::new();
    for (i, e) in app.timeline.entries.iter().enumerate() {
        starts.push(lines.len());
        let sel = focused && i == app.timeline_sel;
        let (head, body_style) = entry_header(app, i, e);
        let head_style = if sel { theme::selected() } else { theme::bold() };
        let mut spans: Vec<Span> = Vec::new();
        for (txt, st) in head {
            spans.push(Span::styled(txt, if sel { head_style } else { st }));
        }
        lines.push(Line::from(spans));
        let body: Option<String> = if e.kind.collapsible() && e.collapsed {
            let p = first_line(&e.body, width.saturating_sub(6));
            (!p.is_empty()).then_some(p)
        } else if e.body.is_empty() {
            None
        } else {
            Some(e.body.clone())
        };
        if let Some(b) = body {
            for l in wrap(&b, width.saturating_sub(3)) {
                lines.push(Line::styled(format!("  │{l}"), body_style));
            }
        }
        lines.push(Line::raw(""));
    }
    (lines, starts)
}

fn entry_header(app: &App, i: usize, e: &Entry) -> (Vec<(String, ratatui::style::Style)>, ratatui::style::Style) {
    let fold = if e.collapsed { "[+]" } else { "[-]" };
    match e.kind {
        EntryKind::User => (vec![("▶ YOU".into(), theme::badge())], theme::bold()),
        EntryKind::Text => (vec![("● ASSISTANT".into(), theme::bold())], theme::base()),
        EntryKind::Thinking => (
            vec![(format!("◆ THINKING {fold} ({} chars)", e.body.chars().count()), theme::thinking())],
            theme::thinking(),
        ),
        EntryKind::ToolUse => {
            let spin = if app.timeline.tool_pending(i) { format!(" {}", SPINNER[i % 4]) } else { String::new() };
            (vec![(format!("⚙ TOOL {fold} {}{spin}", e.title), theme::tool())], theme::muted())
        }
        EntryKind::ToolResult => {
            let mut h = vec![(format!("↳ RESULT {fold} {}", e.title), theme::bold())];
            if e.is_error {
                h.push((" ".into(), theme::base()));
                h.push((" ERROR ".into(), theme::error()));
            }
            (h, if e.is_error { theme::bold() } else { theme::muted() })
        }
        EntryKind::Error => (vec![(" ERROR ".into(), theme::error())], theme::bold()),
        EntryKind::System => (vec![("— system —".into(), theme::faint())], theme::faint()),
    }
}

fn draw_timeline(f: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Focus::Timeline && app.overlay.is_none();
    let b = block("TIMELINE  thinking · tool_use · tool_result · text", focused);
    let inner = b.inner(area);
    f.render_widget(b, area);
    if app.timeline.entries.is_empty() {
        f.render_widget(
            Paragraph::new("Select a session (Enter) or type a prompt below (Tab to input).").style(theme::faint()),
            inner,
        );
        return;
    }
    let (lines, starts) = timeline_lines(app, inner.width as usize, focused);
    let h = inner.height as usize;
    let total = lines.len();
    let mut top = total.saturating_sub(h);
    if focused {
        if let Some(&s) = starts.get(app.timeline_sel) {
            let end = starts.get(app.timeline_sel + 1).copied().unwrap_or(total);
            if s < top {
                top = s;
            } else if end > top + h {
                top = end.saturating_sub(h).min(s);
            }
        }
        if !app.timeline.streaming {
            if let Some(&s) = starts.get(app.timeline_sel) {
                top = top.min(s);
            }
        }
    }
    let visible: Vec<Line> = lines.into_iter().skip(top).take(h).collect();
    f.render_widget(Paragraph::new(Text::from(visible)).style(theme::base()), inner);
}

fn draw_input(f: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Focus::Input && app.overlay.is_none() && app.prompt.is_none();
    let cursor = if focused { "█" } else { "" };
    let title = if app.is_streaming() { "PROMPT (Ctrl+X cancel)" } else { "PROMPT (Enter send)" };
    let p = Paragraph::new(format!("› {}{cursor}", app.input))
        .block(block(title, focused))
        .style(theme::base());
    f.render_widget(p, area);
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    let t = app.timeline.turn_usage;
    let s = app.timeline.total_usage;
    let usage = Line::from(vec![
        Span::styled(" TOKENS ", theme::badge()),
        Span::styled(
            format!(
                " turn in {} · out {} · total {} · ${:.4}  │  Σ in {} · out {} · total {} · ${:.4} ",
                fmt_num(t.input),
                fmt_num(t.output),
                fmt_num(t.total),
                t.cost,
                fmt_num(s.input),
                fmt_num(s.output),
                fmt_num(s.total),
                s.cost
            ),
            theme::footer(),
        ),
    ]);
    let status = Line::from(vec![
        Span::styled(format!(" {} ", app.status), theme::bold()),
        Span::styled(
            "│ Tab focus · Ctrl+B board · Ctrl+L subagents · Ctrl+D diff · Ctrl+R routing · Ctrl+T terminal · Ctrl+K model · F1 help · Ctrl+Q quit",
            theme::faint(),
        ),
    ]);
    f.render_widget(Paragraph::new(vec![usage, status]).style(theme::footer()), area);
}

fn draw_board(f: &mut Frame, area: Rect, app: &App) {
    let inner = popup(f, area, "PROGRESS BOARD + PLAN  (Ctrl+B)");
    let [info, main, keys] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)]).areas(inner);
    let (phase, progress, sid) = app
        .board
        .as_ref()
        .map(|b| {
            (
                b.phase.clone().unwrap_or_default(),
                b.progress.clone().unwrap_or_default(),
                b.session_id.clone().unwrap_or_default(),
            )
        })
        .unwrap_or_default();
    f.render_widget(
        Paragraph::new(format!(" session {sid} · phase {phase} · progress {progress}")).style(theme::bold()),
        info,
    );
    let [tasks_a, plan_a] = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(main);
    let rows: Vec<Row> = app
        .board_tasks()
        .iter()
        .map(|t| {
            Row::new(vec![
                format!("[{}]", t.status),
                t.id.clone(),
                t.subject.clone(),
                t.assignee_role.clone().unwrap_or_default(),
                t.turn.map(|n| n.to_string()).unwrap_or_default(),
            ])
        })
        .collect();
    let tf = app.board_pane == BoardPane::Tasks;
    let table = Table::new(
        rows,
        [Constraint::Length(12), Constraint::Length(8), Constraint::Min(10), Constraint::Length(10), Constraint::Length(4)],
    )
    .header(Row::new(vec!["STATUS", "ID", "SUBJECT", "ROLE", "TURN"]).style(theme::bold()))
    .block(block("TASKS", tf))
    .style(theme::base())
    .row_highlight_style(if tf { theme::selected() } else { theme::bold() });
    let mut st = TableState::default().with_selected((!app.board_tasks().is_empty()).then_some(app.board_sel));
    f.render_stateful_widget(table, tasks_a, &mut st);

    let plan_text = match &app.plan {
        Some(p) if p.exists => p.content.clone(),
        Some(p) => format!("PLAN.md not found: {}", p.plan_md_path),
        None => "Loading PLAN.md…".into(),
    };
    let title = app.plan.as_ref().map(|p| format!("PLAN {}", p.plan_md_path)).unwrap_or_else(|| "PLAN".into());
    f.render_widget(
        Paragraph::new(plan_text)
            .block(block(&title, !tf))
            .style(theme::base())
            .wrap(Wrap { trim: false })
            .scroll((app.plan_scroll, 0)),
        plan_a,
    );
    f.render_widget(
        Paragraph::new(" Tab pane · ↑↓ select/scroll · s sync PLAN · g dispatch · c claim-only · b debate · r refresh · Esc close")
            .style(theme::faint()),
        keys,
    );
}

fn draw_subagents(f: &mut Frame, area: Rect, app: &App) {
    let inner = popup(f, area, "SUBAGENTS + LIVE LOG  (Ctrl+L)");
    let [main, keys] = Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(inner);
    let [list_a, log_a] = Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(main);
    let items: Vec<ListItem> = app
        .subagents
        .iter()
        .map(|s| {
            ListItem::new(vec![
                Line::styled(
                    format!("[{}] {} {}", s.status, s.task_id.as_deref().unwrap_or("-"), s.role.as_deref().unwrap_or("")),
                    theme::bold(),
                ),
                Line::styled(
                    format!(
                        "  {} · {} · {}",
                        short(&s.sub_session_id, 16),
                        s.harness.as_deref().unwrap_or("-"),
                        s.model.as_deref().unwrap_or("-")
                    ),
                    theme::muted(),
                ),
            ])
        })
        .collect();
    let mut st = ListState::default().with_selected((!items.is_empty()).then_some(app.sub_sel));
    f.render_stateful_widget(
        List::new(items).block(block("SUBAGENTS", true)).style(theme::base()).highlight_style(theme::selected()),
        list_a,
        &mut st,
    );
    let h = log_a.height.saturating_sub(2) as usize;
    let start = app.logs.len().saturating_sub(h);
    let log: Vec<Line> = app.logs[start..].iter().map(|l| Line::styled(l.clone(), theme::muted())).collect();
    f.render_widget(Paragraph::new(log).block(block("LOG TAIL", false)).style(theme::base()), log_a);
    f.render_widget(
        Paragraph::new(" ↑↓ select · x kill · r retry · f fallback (current harness/model) · u resume · c custom · F5 refresh · Esc close")
            .style(theme::faint()),
        keys,
    );
}

fn draw_diff(f: &mut Frame, area: Rect, app: &App) {
    let inner = popup(f, area, "WORKTREE DIFF  (Ctrl+D)");
    let [main, keys] = Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(inner);
    let [list_a, diff_a] = Layout::horizontal([Constraint::Percentage(30), Constraint::Percentage(70)]).areas(main);
    let items: Vec<ListItem> = app
        .board_tasks()
        .iter()
        .map(|t| ListItem::new(format!("[{}] {} {}", t.status, t.id, t.subject)))
        .collect();
    let mut st = ListState::default().with_selected((!items.is_empty()).then_some(app.diff_sel));
    f.render_stateful_widget(
        List::new(items).block(block("TASKS", true)).style(theme::base()).highlight_style(theme::selected()),
        list_a,
        &mut st,
    );
    let mut lines: Vec<Line> = Vec::new();
    match &app.diff {
        None => lines.push(Line::styled("Enter = load diff for selected task", theme::faint())),
        Some(d) => {
            lines.push(Line::styled(
                format!(
                    "{} · mode {} · base {}",
                    d.worktree_path.as_deref().unwrap_or("-"),
                    d.mode.as_deref().unwrap_or("-"),
                    d.base_ref.as_deref().unwrap_or("-")
                ),
                theme::bold(),
            ));
            for file in &d.files {
                lines.push(Line::styled(
                    format!(
                        " {} {}  +{} -{}",
                        file.status.as_deref().unwrap_or("M"),
                        file.path,
                        file.additions.unwrap_or(0),
                        file.deletions.unwrap_or(0)
                    ),
                    theme::base(),
                ));
            }
            if let Some(e) = &d.error {
                lines.push(Line::styled(format!(" ERROR {e}"), theme::error()));
            }
            lines.push(Line::raw(""));
            for l in d.diff_text.as_deref().unwrap_or("").lines() {
                let st = if l.starts_with('+') && !l.starts_with("+++") {
                    theme::diff_add()
                } else if l.starts_with('-') && !l.starts_with("---") {
                    theme::diff_del()
                } else if l.starts_with("@@") || l.starts_with("diff ") {
                    theme::selected()
                } else {
                    theme::muted()
                };
                lines.push(Line::styled(l.to_string(), st));
            }
        }
    }
    f.render_widget(
        Paragraph::new(lines).block(block("DIFF", false)).style(theme::base()).scroll((app.diff_scroll, 0)),
        diff_a,
    );
    f.render_widget(
        Paragraph::new(" ↑↓ task · Enter load diff · PgUp/PgDn scroll · a approve · x reject · F5 refresh · Esc close")
            .style(theme::faint()),
        keys,
    );
}

fn draw_routing(f: &mut Frame, area: Rect, app: &App) {
    let editing = app.routing_editor.is_some();
    let inner = popup(f, area, if editing { "ROUTING CONFIG — EDITING  (Ctrl+R)" } else { "ROUTING CONFIG  (Ctrl+R)" });
    let [main, keys] = Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(inner);
    if let Some(ed) = &app.routing_editor {
        let h = main.height as usize;
        let top = ed.row.saturating_sub(h.saturating_sub(1));
        let lines: Vec<Line> = ed
            .lines
            .iter()
            .enumerate()
            .skip(top)
            .take(h)
            .map(|(r, l)| {
                if r == ed.row {
                    let (a, b): (String, String) = (l.chars().take(ed.col).collect(), l.chars().skip(ed.col).collect());
                    let mut bc = b.chars();
                    let cur = bc.next().map(|c| c.to_string()).unwrap_or_else(|| " ".into());
                    Line::from(vec![
                        Span::styled(a, theme::base()),
                        Span::styled(cur, theme::selected()),
                        Span::styled(bc.collect::<String>(), theme::base()),
                    ])
                } else {
                    Line::styled(l.clone(), theme::base())
                }
            })
            .collect();
        f.render_widget(Paragraph::new(lines).style(theme::base()), main);
        f.render_widget(Paragraph::new(" Ctrl+S save (POST /api/routing) · Esc cancel edit").style(theme::faint()), keys);
    } else {
        let text = app
            .routing
            .as_ref()
            .map(|v| serde_json::to_string_pretty(v).unwrap_or_default())
            .unwrap_or_else(|| "Loading GET /api/routing…".into());
        f.render_widget(Paragraph::new(text).style(theme::base()).scroll((app.routing_scroll, 0)), main);
        f.render_widget(
            Paragraph::new(" ↑↓ scroll · e edit · x reset to defaults · r reload · Esc close").style(theme::faint()),
            keys,
        );
    }
}

fn draw_terminal(f: &mut Frame, area: Rect, app: &App) {
    let inner = popup(f, area, "INTERACTIVE RESUME / OS TERMINAL  (Ctrl+T)");
    let mut lines = vec![];
    match &app.resume {
        None => lines.push(Line::styled("No resume command loaded (select an ASD session).", theme::faint())),
        Some(r) => {
            lines.push(Line::from(vec![Span::styled(" HARNESS ", theme::badge()), Span::styled(format!(" {}", r.harness), theme::bold())]));
            lines.push(Line::styled(format!("session  {}", r.harness_session_id.as_deref().unwrap_or("-")), theme::muted()));
            lines.push(Line::styled(format!("cwd      {}", r.cwd), theme::muted()));
            lines.push(Line::raw(""));
            lines.push(Line::styled("command:", theme::bold()));
            lines.push(Line::styled(format!("  {}", r.cli_string), theme::selected()));
            if let Some(e) = &r.error {
                lines.push(Line::styled(format!(" ERROR {e}"), theme::error()));
            }
        }
    }
    lines.push(Line::raw(""));
    let kinds: Vec<Span> = TERMINALS
        .iter()
        .enumerate()
        .map(|(i, k)| {
            Span::styled(
                format!(" {} ", terminal_name(*k)),
                if i == app.terminal_kind { theme::selected() } else { theme::muted() },
            )
        })
        .collect();
    lines.push(Line::from(kinds));
    lines.push(Line::raw(""));
    lines.push(Line::styled("Enter/o launch in OS terminal · k/Tab terminal kind · r reload · Esc close", theme::faint()));
    f.render_widget(Paragraph::new(lines).style(theme::base()).wrap(Wrap { trim: false }), inner);
}

fn draw_picker(f: &mut Frame, area: Rect, app: &App) {
    let inner = popup(f, area, "HARNESS & MODEL  (Ctrl+K)");
    let [tabs, q, list_a, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(inner);
    let spans: Vec<Span> = HARNESSES
        .iter()
        .enumerate()
        .map(|(i, h)| Span::styled(format!(" {h} "), if i == app.picker_harness { theme::selected() } else { theme::muted() }))
        .collect();
    f.render_widget(Paragraph::new(Line::from(spans)), tabs);
    f.render_widget(Paragraph::new(format!(" search: {}█", app.picker_query)).style(theme::bold()), q);
    let models = app.picker_models();
    let items: Vec<ListItem> = models
        .iter()
        .map(|m| {
            let cur = app.harness == HARNESSES[app.picker_harness]
                && (app.model.as_deref() == Some(m.as_str()) || (app.model.is_none() && m == "(default)"));
            ListItem::new(format!("{} {m}", if cur { "●" } else { " " }))
        })
        .collect();
    let mut st = ListState::default().with_selected(Some(app.picker_sel));
    f.render_stateful_widget(
        List::new(items).style(theme::base()).highlight_style(theme::selected()),
        list_a,
        &mut st,
    );
    f.render_widget(
        Paragraph::new(" type to filter · ↑↓ select · Tab/←→ harness · F5 refresh models · Enter apply · Esc close")
            .style(theme::faint()),
        keys,
    );
}

fn draw_help(f: &mut Frame, area: Rect) {
    let inner = popup(f, area, "HELP");
    let rows = [
        ("Global", "Tab/Shift+Tab focus · Ctrl+Q/Ctrl+C quit · Ctrl+X cancel turn · Ctrl+E fold/unfold all · F1 help"),
        ("Overlays", "Ctrl+B board+PLAN · Ctrl+L subagents · Ctrl+D diff · Ctrl+R routing · Ctrl+T terminal · Ctrl+K model"),
        ("Projects", "↑↓/jk select · Enter switch project · r refresh"),
        ("Sessions", "Enter open · n new · p promote · d demote · u unlink · x delete · e memo · f foreground"),
        ("", "t toggle phase · / search · h harness filter · H cycle harness · m model picker · r refresh"),
        ("Timeline", "↑↓/jk select · Enter/Space fold thinking/tool blocks · i input"),
        ("Prompt", "type · Enter send (SSE stream) · Esc back"),
    ];
    let lines: Vec<Line> = rows
        .iter()
        .map(|(a, b)| Line::from(vec![Span::styled(format!(" {a:<10}"), theme::bold()), Span::styled(*b, theme::muted())]))
        .collect();
    f.render_widget(Paragraph::new(lines).style(theme::base()).wrap(Wrap { trim: false }), inner);
}
