//! OpenCode-inspired borderless TUI layout and modal rendering.
//! Supports both `ThemeMode::DarkColor` (default) and `ThemeMode::Monochrome` (`/theme`).

use ratatui::layout::{Alignment, Constraint, Flex, Layout, Rect};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState, Wrap,
};
use ratatui::Frame;

use crate::app::{
    terminal_name, App, BoardPane, CommandBadge, Focus, Overlay, UiRegions, HARNESSES, TERMINALS,
};
use crate::theme;
use crate::timeline::EntryKind;

const SPINNER: [char; 4] = ['⠋', '⠙', '⠹', '⠸'];

fn block(title: &str, focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(if focused { BorderType::Rounded } else { BorderType::Plain })
        .border_style(theme::border(focused))
        .title(Span::styled(format!(" {title} "), if focused { theme::selected() } else { theme::title() }))
        .style(theme::panel())
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
    theme::set_mode(app.theme_mode);
    *app.ui_regions.borrow_mut() = UiRegions::default();

    let area = f.area();
    f.render_widget(Block::default().style(theme::base()), area);

    let [head, _top_gap, body, foot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .areas(area);
    draw_header(f, head, app);

    let main_area = if app.sidebar_open {
        let [left, right] = Layout::horizontal([Constraint::Length(36), Constraint::Min(24)]).areas(body);
        let [proj, sess] = Layout::vertical([Constraint::Percentage(30), Constraint::Min(4)]).areas(left);
        draw_projects(f, proj, app);
        draw_sessions_panel(f, sess, app);
        right
    } else {
        body
    };

    if app.is_home() {
        draw_home(f, main_area, app);
    } else {
        let input_lines = app.input_area.lines.len().clamp(1, 6) as u16;
        let input_h = (input_lines + 3).min(main_area.height.saturating_sub(3)).max(3);
        let [tl_area, _gap, input_area] = Layout::vertical([
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(input_h),
        ])
        .areas(main_area);
        draw_timeline(f, tl_area, app);
        draw_input_card(f, input_area, app);
        draw_input_popups(f, main_area, input_area, app);
    }

    draw_footer(f, foot, app);

    if let Some(o) = app.overlay {
        let a = centered(area, 86, 84);
        match o {
            Overlay::Board => draw_board(f, a, app),
            Overlay::Subagents => draw_subagents(f, a, app),
            Overlay::Diff => draw_diff(f, a, app),
            Overlay::Routing => draw_routing(f, a, app),
            Overlay::Terminal => draw_terminal(f, centered(area, 72, 54), app),
            Overlay::ModelPicker => draw_picker(f, centered(area, 64, 72), app),
            Overlay::Sessions => draw_sessions_modal(f, centered(area, 82, 78), app),
            Overlay::DirPicker => draw_dir_picker(f, centered(area, 70, 68), app),
            Overlay::CommandPalette => draw_command_palette(f, centered(area, 68, 72), app),
            Overlay::Help => draw_help(f, a),
        }
    }
    if let Some(p) = &app.prompt {
        let a = centered(area, 60, 22);
        let inner = popup(f, a, p.purpose.label());
        let t = Paragraph::new(vec![
            Line::styled(format!("{}█", p.buffer), theme::bold()),
            Line::styled("Enter = submit · Esc = cancel", theme::faint()),
        ])
        .style(theme::panel())
        .wrap(Wrap { trim: false });
        f.render_widget(t, inner);
    }
    if let Some((q, _)) = &app.confirm {
        let a = centered(area, 52, 22);
        let inner = popup(f, a, "CONFIRM");
        f.render_widget(
            Paragraph::new(vec![
                Line::styled(q.clone(), theme::bold()),
                Line::styled("y = yes · any other key = cancel", theme::faint()),
            ])
            .style(theme::panel())
            .wrap(Wrap { trim: false }),
            inner,
        );
    }
    if !app.pending_permissions.is_empty() {
        draw_permission_modal(f, area, app);
    }
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let t = app.timeline.turn_usage;
    let s = app.timeline.total_usage;
    let tok_total = if s.total > 0 { s.total } else { t.input + t.output };
    let cost_total = if s.cost > 0.0 { s.cost } else { t.cost };

    let mut spans = vec![
        Span::styled(" ASD-NEXUS ", theme::badge()),
        Span::styled(format!("  {}", app.project_root), theme::header()),
    ];

    if app.is_session_started() {
        let sess_summary = app
            .current
            .as_ref()
            .and_then(|s| s.summary.as_deref())
            .map(|s| first_line(s, 28))
            .or_else(|| app.active_session_id().map(|id| short(&id, 20)));
        if let Some(summary) = sess_summary {
            spans.push(Span::styled(format!("  ({summary})"), theme::header_muted()));
        }
    }

    if tok_total > 0 {
        spans.push(Span::styled(
            format!("  ·  {} tok (${:.4})", fmt_num(tok_total), cost_total),
            theme::header_muted(),
        ));
    }

    if app.is_streaming() {
        let spin = SPINNER[(app.timeline.entries.len()) % 4];
        spans.push(Span::styled("  ", theme::header()));
        spans.push(Span::styled(format!(" {spin} STREAMING "), theme::badge()));
    }

    f.render_widget(Paragraph::new(Line::from(spans)).style(theme::header()), area);
}

fn draw_home(f: &mut Frame, area: Rect, app: &App) {
    let card_w = area.width.saturating_sub(8).clamp(36, 88);
    let pad_x = area.width.saturating_sub(card_w) / 2;
    let center_col = Rect::new(area.x + pad_x, area.y, card_w, area.height);

    let input_lines = app.input_area.lines.len().clamp(1, 5) as u16;
    let input_h = (input_lines + 3).min(area.height.saturating_sub(4)).max(3);

    let [_top_sp, hero_area, card_area, _bot_sp] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(3),
        Constraint::Length(input_h),
        Constraint::Min(1),
    ])
    .areas(center_col);

    let hero = vec![
        Line::from(vec![
            Span::styled("▄▀█ █▀ █▀▄   ", theme::accent()),
            Span::styled("█▄░█ █▀▀ ▀▄▀ █░█ █▀", theme::bold()),
        ]),
        Line::from(vec![
            Span::styled("█▀█ ▄█ █▄▀   ", theme::accent()),
            Span::styled("█░▀█ ██▄ █░█ █▄█ ▄█", theme::bold()),
        ]),
        Line::raw(""),
    ];
    f.render_widget(
        Paragraph::new(hero).alignment(Alignment::Center).style(theme::base()),
        hero_area,
    );

    draw_input_card(f, card_area, app);
    draw_input_popups(f, area, card_area, app);
}

fn draw_input_card(f: &mut Frame, area: Rect, app: &App) {
    let pad = if !app.is_home() && area.width > 44 { 2 } else { 0 };
    let card = Rect::new(
        area.x + pad,
        area.y,
        area.width.saturating_sub(pad * 2).max(10),
        area.height,
    );
    app.ui_regions.borrow_mut().input_rect = Some(card);

    f.render_widget(Block::default().style(theme::surface()), card);

    let focused = app.focus == Focus::Input
        && app.pending_permissions.is_empty()
        && app.overlay.is_none()
        && app.prompt.is_none()
        && app.confirm.is_none();
    let bar = if focused { "┃ " } else { "│ " };
    let bar_style = if focused { theme::surface_accent() } else { theme::surface_faint() };

    let h = card.height as usize;
    let has_vpad = h >= 4;
    let text_rows = if has_vpad {
        h - 3
    } else {
        h.saturating_sub(1).max(1)
    };

    let total_lines = app.input_area.lines.len();
    let start_row = app.input_area.row.saturating_sub(text_rows.saturating_sub(1));

    let mut rendered_lines: Vec<Line> = Vec::with_capacity(h);
    if has_vpad {
        rendered_lines.push(Line::from(vec![Span::styled(bar, bar_style)]));
    }

    let is_empty = app.input.is_empty() && total_lines == 1 && app.input_area.lines[0].is_empty();
    let mut pushed_text = 0usize;

    if is_empty {
        let placeholder = if app.is_streaming() { "Streaming…" } else { "Ask anything…" };
        let mut spans = vec![Span::styled(bar, bar_style), Span::styled("› ", theme::surface_accent())];
        if focused {
            spans.push(Span::styled("█", theme::surface_accent()));
            spans.push(Span::styled(format!(" {placeholder}"), theme::surface_faint()));
        } else {
            spans.push(Span::styled(placeholder, theme::surface_faint()));
        }
        rendered_lines.push(Line::from(spans));
        pushed_text += 1;
    } else {
        for (r, line_str) in app.input_area.lines.iter().enumerate().skip(start_row).take(text_rows) {
            let prefix = if r == 0 { "› " } else { "  " };
            let mut spans = vec![
                Span::styled(bar, bar_style),
                Span::styled(prefix, theme::surface_accent()),
            ];
            if focused && r == app.input_area.row {
                let before: String = line_str.chars().take(app.input_area.col).collect();
                let after: String = line_str.chars().skip(app.input_area.col).collect();
                spans.push(Span::styled(before, theme::surface()));
                spans.push(Span::styled("█", theme::surface_accent()));
                spans.push(Span::styled(after, theme::surface()));
            } else {
                spans.push(Span::styled(line_str.clone(), theme::surface()));
            }
            rendered_lines.push(Line::from(spans));
            pushed_text += 1;
        }
    }

    while pushed_text < text_rows {
        rendered_lines.push(Line::from(vec![Span::styled(bar, bar_style)]));
        pushed_text += 1;
    }

    if h >= 2 {
        let auto_label = if app.auto_approve { "[auto:on]" } else { "[auto:off]" };
        let auto_style = if app.auto_approve { theme::surface_faint() } else { theme::badge() };
        let pill = Line::from(vec![
            Span::styled(bar, bar_style),
            Span::styled(format!(" {} ", app.harness), theme::harness_badge()),
            Span::styled(
                format!(" · {} ", app.model.as_deref().unwrap_or("(default)")),
                theme::surface_muted(),
            ),
            Span::styled(format!(" {auto_label}"), auto_style),
        ]);
        rendered_lines.push(pill);
    }

    if has_vpad {
        rendered_lines.push(Line::from(vec![Span::styled(bar, bar_style)]));
    }

    f.render_widget(Paragraph::new(rendered_lines).style(theme::surface()), card);
}

fn draw_input_popups(f: &mut Frame, container: Rect, input_rect: Rect, app: &App) {
    if app.is_slash_popup_active() {
        let cmds = app.filtered_slash_commands();
        let max_vis = 8usize.min(cmds.len()).min(input_rect.y.saturating_sub(container.y) as usize);
        if max_vis == 0 {
            return;
        }
        let h = (max_vis as u16) + 2;
        let w = input_rect.width.clamp(40, 86).min(container.width);
        let y = input_rect.y.saturating_sub(h);
        let rect = Rect::new(input_rect.x, y, w, h);
        app.ui_regions.borrow_mut().popup_rect = Some(rect);

        let inner = popup(f, rect, "SLASH COMMANDS  (↑↓ select · Tab complete · Enter run · Esc close)");
        let start = app.slash_sel.saturating_sub(max_vis.saturating_sub(1));
        let mut items: Vec<ListItem> = Vec::new();
        let mut rows_map = Vec::new();

        for (vis_idx, (cmd_idx, cmd)) in cmds.iter().enumerate().skip(start).take(max_vis).enumerate() {
            let row_y = inner.y + vis_idx as u16;
            rows_map.push((row_y, cmd_idx));
            let badge_style = match &cmd.badge {
                CommandBadge::Nexus => theme::badge(),
                CommandBadge::Harness(_) => theme::harness_badge(),
            };
            let is_sel = cmd_idx == app.slash_sel;
            let row_style = if is_sel { theme::selected() } else { theme::panel() };
            let desc_style = if is_sel { theme::selected() } else { theme::muted() };
            items.push(ListItem::new(Line::from(vec![
                Span::styled(format!(" {:<16} ", cmd.usage), row_style),
                Span::styled(format!(" {} ", cmd.badge.label()), badge_style),
                Span::styled(format!(" {}", first_line(&cmd.description, 44)), desc_style),
            ])));
        }
        app.ui_regions.borrow_mut().popup_rows = rows_map;
        let mut st = ListState::default().with_selected(Some(app.slash_sel.saturating_sub(start)));
        f.render_stateful_widget(
            List::new(items).style(theme::panel()).highlight_style(theme::selected()),
            inner,
            &mut st,
        );
    } else if app.is_file_popup_active() {
        let files = app.filtered_files();
        let max_vis = 8usize.min(files.len()).min(input_rect.y.saturating_sub(container.y) as usize);
        if max_vis == 0 {
            return;
        }
        let h = (max_vis as u16) + 2;
        let w = input_rect.width.clamp(40, 80).min(container.width);
        let y = input_rect.y.saturating_sub(h);
        let rect = Rect::new(input_rect.x, y, w, h);
        app.ui_regions.borrow_mut().popup_rect = Some(rect);

        let inner = popup(f, rect, "@FILE AUTOCOMPLETE  (↑↓ select · Tab/Enter insert · Esc close)");
        let start = app.file_sel.saturating_sub(max_vis.saturating_sub(1));
        let mut items: Vec<ListItem> = Vec::new();
        let mut rows_map = Vec::new();

        for (vis_idx, (file_idx, entry)) in files.iter().enumerate().skip(start).take(max_vis).enumerate() {
            let row_y = inner.y + vis_idx as u16;
            rows_map.push((row_y, file_idx));
            let is_sel = file_idx == app.file_sel;
            let st = if is_sel { theme::selected() } else { theme::panel() };
            items.push(ListItem::new(Line::from(vec![
                Span::styled(format!(" @{} ", entry.path), st),
            ])));
        }
        app.ui_regions.borrow_mut().popup_rows = rows_map;
        let mut st = ListState::default().with_selected(Some(app.file_sel.saturating_sub(start)));
        f.render_stateful_widget(
            List::new(items).style(theme::panel()).highlight_style(theme::selected()),
            inner,
            &mut st,
        );
    }
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
        .style(theme::panel())
        .highlight_style(if focused { theme::selected() } else { theme::bold() });
    f.render_stateful_widget(list, area, &mut st);
}

fn draw_sessions_panel(f: &mut Frame, area: Rect, app: &App) {
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
                format!("   {}", first_line(s.summary.as_deref().unwrap_or("(no summary)"), 30)),
                theme::faint(),
            );
            ListItem::new(vec![head, sum])
        })
        .collect();
    let mut st = ListState::default().with_selected((!items.is_empty()).then_some(app.session_sel));
    let list = List::new(items)
        .block(block(&title, focused))
        .style(theme::panel())
        .highlight_style(if focused { theme::selected() } else { theme::bold() });
    f.render_stateful_widget(list, area, &mut st);
}

/// Render timeline entries into wrapped lines; returns `(lines, start_line_of_each_entry)`.
pub fn timeline_lines(app: &App, width: usize, focused: bool) -> (Vec<Line<'static>>, Vec<usize>) {
    let mut lines = Vec::new();
    let mut starts = Vec::new();
    for (i, e) in app.timeline.entries.iter().enumerate() {
        starts.push(lines.len());
        let sel = focused && i == app.timeline_sel;

        match e.kind {
            EntryKind::User => {
                // Only User messages have the left vertical bar; no "YOU" header label.
                let bar_st = if sel { theme::selected() } else { theme::accent() };
                let txt_st = if sel { theme::selected() } else { theme::bold() };
                let wrapped = if e.body.is_empty() {
                    vec![String::new()]
                } else {
                    wrap(&e.body, width.saturating_sub(3))
                };
                for l in wrapped {
                    lines.push(Line::from(vec![
                        Span::styled("┃ ", bar_st),
                        Span::styled(l, txt_st),
                    ]));
                }
            }
            EntryKind::Thinking => {
                // Gray text, no left vertical bar.
                let head_st = if sel { theme::selected() } else { theme::thinking() };
                if e.collapsed {
                    let preview = first_line(&e.body, width.saturating_sub(16));
                    let label = if preview.is_empty() {
                        "▸ Thinking".to_string()
                    } else {
                        format!("▸ Thinking · {preview}")
                    };
                    lines.push(Line::styled(label, head_st));
                } else {
                    lines.push(Line::styled("▾ Thinking", head_st));
                    if !e.body.is_empty() {
                        for l in wrap(&e.body, width.saturating_sub(3)) {
                            lines.push(Line::styled(format!("  {l}"), theme::thinking()));
                        }
                    }
                }
            }
            EntryKind::Text => {
                // Pure white response text, no left vertical bar, no "ASSISTANT" header.
                let txt_st = if sel { theme::selected() } else { theme::base() };
                if !e.body.is_empty() {
                    for l in wrap(&e.body, width.saturating_sub(1)) {
                        lines.push(Line::styled(l, txt_st));
                    }
                }
            }
            EntryKind::ToolUse => {
                let arrow = if e.collapsed { "▸" } else { "▾" };
                let spin = if app.timeline.tool_pending(i) {
                    format!(" {}", SPINNER[i % 4])
                } else {
                    String::new()
                };
                let head_st = if sel { theme::selected() } else { theme::muted() };
                lines.push(Line::styled(format!("{arrow} tool · {}{spin}", e.title), head_st));
                if !e.collapsed && !e.body.is_empty() {
                    for l in wrap(&e.body, width.saturating_sub(3)) {
                        lines.push(Line::styled(format!("  {l}"), theme::faint()));
                    }
                }
            }
            EntryKind::ToolResult => {
                let arrow = if e.collapsed { "▸" } else { "▾" };
                let head_st = if sel { theme::selected() } else { theme::muted() };
                let mut spans = vec![Span::styled(format!("{arrow} result · {}", e.title), head_st)];
                if e.is_error {
                    spans.push(Span::styled(" ", theme::base()));
                    spans.push(Span::styled(" ERROR ", theme::error()));
                }
                lines.push(Line::from(spans));
                if !e.collapsed && !e.body.is_empty() {
                    let body_st = if e.is_error { theme::bold() } else { theme::faint() };
                    for l in wrap(&e.body, width.saturating_sub(3)) {
                        lines.push(Line::styled(format!("  {l}"), body_st));
                    }
                }
            }
            EntryKind::Shell => {
                let arrow = if e.collapsed { "▸" } else { "▾" };
                let head_st = if sel { theme::selected() } else { theme::shell() };
                let mut spans = vec![Span::styled(format!("{arrow} $ {}", e.title), head_st)];
                if e.is_error {
                    spans.push(Span::styled(" ", theme::base()));
                    spans.push(Span::styled(" ERROR ", theme::error()));
                }
                lines.push(Line::from(spans));
                if !e.collapsed && !e.body.is_empty() {
                    for l in wrap(&e.body, width.saturating_sub(3)) {
                        lines.push(Line::styled(format!("  {l}"), theme::muted()));
                    }
                }
            }
            EntryKind::Error => {
                let head_st = if sel { theme::selected() } else { theme::error() };
                lines.push(Line::from(vec![Span::styled(" ERROR ", head_st)]));
                if !e.body.is_empty() {
                    for l in wrap(&e.body, width.saturating_sub(3)) {
                        lines.push(Line::styled(format!("  {l}"), theme::bold()));
                    }
                }
            }
            EntryKind::System => {
                if !e.body.is_empty() {
                    let st = if sel { theme::selected() } else { theme::faint() };
                    for l in wrap(&e.body, width.saturating_sub(1)) {
                        lines.push(Line::styled(l, st));
                    }
                }
            }
        }

        lines.push(Line::styled("", theme::base()));
    }
    (lines, starts)
}

fn draw_timeline(f: &mut Frame, area: Rect, app: &App) {
    let pad = if area.width > 44 { 2 } else { 1 };
    let inner = Rect::new(
        area.x + pad,
        area.y,
        area.width.saturating_sub(pad * 2).max(10),
        area.height,
    );
    app.ui_regions.borrow_mut().timeline_rect = Some(inner);

    let focused = app.focus == Focus::Timeline && app.overlay.is_none();
    let (lines, starts) = timeline_lines(app, inner.width as usize, focused);
    let h = inner.height as usize;
    let total = lines.len();
    let mut top = app.timeline_scroll.unwrap_or_else(|| total.saturating_sub(h));
    if app.timeline_scroll.is_none() && (focused || !app.timeline.streaming) {
        if let Some(&s) = starts.get(app.timeline_sel) {
            let end = starts.get(app.timeline_sel + 1).copied().unwrap_or(total);
            if s < top {
                top = s;
            } else if end > top + h {
                top = end.saturating_sub(h).min(s);
            }
        }
    }
    top = top.min(total.saturating_sub(1));

    // Record screen Y ranges for each visible timeline entry for mouse click hit-testing.
    let mut entry_regions = Vec::new();
    for (idx, &s) in starts.iter().enumerate() {
        let e_end = starts.get(idx + 1).copied().unwrap_or(total);
        if e_end > top && s < top + h {
            let vis_start = s.saturating_sub(top) as u16 + inner.y;
            let vis_end = (e_end.min(top + h) - top) as u16 + inner.y;
            entry_regions.push((vis_start, vis_end, idx));
        }
    }
    app.ui_regions.borrow_mut().timeline_entries = entry_regions;

    let visible: Vec<Line> = lines.into_iter().skip(top).take(h).collect();
    f.render_widget(Paragraph::new(Text::from(visible)).style(theme::base()), inner);
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    let line = if app.status.is_empty() {
        Line::raw("")
    } else {
        Line::styled(format!("  {}", app.status), theme::faint())
    };
    f.render_widget(Paragraph::new(line).style(theme::footer()), area);
}

fn draw_sessions_modal(f: &mut Frame, area: Rect, app: &App) {
    let filter = app.harness_filter.map(|i| HARNESSES[i]).unwrap_or("all");
    let mut title = format!("SESSIONS [{filter}]  (/sessions)");
    if !app.search.is_empty() {
        title.push_str(&format!("  search: {}", app.search));
    }
    let inner = popup(f, area, &title);
    let [list_a, keys] = Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(inner);

    let idx = app.filtered_sessions();
    let items: Vec<ListItem> = idx
        .iter()
        .map(|&i| {
            let s = &app.sessions[i];
            let fg = s.asd_session_id.is_some() && s.asd_session_id == app.foreground_id;
            let promoted = if fg { "★" } else if s.is_promoted_to_asd { "◆" } else { "◇" };
            let phase = s.asd_phase.as_deref().map(|p| format!(" [{p}]")).unwrap_or_default();
            let asd_id = s.asd_session_id.as_deref().unwrap_or("-");
            let head = Line::from(vec![
                Span::styled(format!(" {promoted} {:<12}", s.harness), theme::bold()),
                Span::styled(format!(" {:<18}", short(&s.harness_session_id, 18)), theme::muted()),
                Span::styled(format!(" asd:{:<14}{phase}", short(asd_id, 14)), theme::faint()),
            ]);
            let sum = Line::styled(
                format!("    {}", first_line(s.summary.as_deref().unwrap_or("(no summary)"), 68)),
                theme::muted(),
            );
            ListItem::new(vec![head, sum])
        })
        .collect();
    let mut st = ListState::default().with_selected((!items.is_empty()).then_some(app.session_sel));
    f.render_stateful_widget(
        List::new(items).style(theme::panel()).highlight_style(theme::selected()),
        list_a,
        &mut st,
    );
    f.render_widget(
        Paragraph::new(
            " Enter open · / search · n new · p promote · d demote · u unlink · x del · e memo · f fg · t phase · h filter · Esc close",
        )
        .style(theme::faint()),
        keys,
    );
}

fn draw_dir_picker(f: &mut Frame, area: Rect, app: &App) {
    let inner = popup(f, area, "SWITCH DIRECTORY  (/cd)");
    let [cur_a, q_a, list_a, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(inner);

    f.render_widget(
        Paragraph::new(format!(" current: {}", app.project_root)).style(theme::muted()),
        cur_a,
    );
    f.render_widget(
        Paragraph::new(format!(" path / filter: {}█", app.dir_query)).style(theme::bold()),
        q_a,
    );

    let items_data = app.dir_picker_items();
    let items: Vec<ListItem> = items_data
        .iter()
        .map(|(path, badge)| {
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {:<18} ", badge), theme::muted()),
                Span::styled(path.clone(), theme::panel()),
            ]))
        })
        .collect();
    let mut st = ListState::default().with_selected((!items.is_empty()).then_some(app.dir_sel));
    f.render_stateful_widget(
        List::new(items).style(theme::panel()).highlight_style(theme::selected()),
        list_a,
        &mut st,
    );
    f.render_widget(
        Paragraph::new(" type filter/path · ↑↓ select · Tab drill into dir · Enter switch project · Esc close")
            .style(theme::faint()),
        keys,
    );
}

fn draw_command_palette(f: &mut Frame, area: Rect, app: &App) {
    let inner = popup(f, area, "COMMAND PALETTE  (Ctrl+P)");
    let [q_a, list_a, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(inner);

    f.render_widget(
        Paragraph::new(format!(" › {}█", app.palette_query)).style(theme::bold()),
        q_a,
    );

    let cmds = app.palette_items();
    let items: Vec<ListItem> = cmds
        .iter()
        .map(|c| {
            let badge_st = match &c.badge {
                CommandBadge::Nexus => theme::badge(),
                CommandBadge::Harness(_) => theme::harness_badge(),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {:<18} ", c.usage), theme::bold()),
                Span::styled(format!(" {} ", c.badge.label()), badge_st),
                Span::styled(format!(" {}", c.description), theme::muted()),
            ]))
        })
        .collect();
    let mut st = ListState::default().with_selected((!items.is_empty()).then_some(app.palette_sel));
    f.render_stateful_widget(
        List::new(items).style(theme::panel()).highlight_style(theme::selected()),
        list_a,
        &mut st,
    );
    f.render_widget(
        Paragraph::new(" type to search Nexus & harness commands · ↑↓ select · Enter execute · Esc close")
            .style(theme::faint()),
        keys,
    );
}

fn draw_board(f: &mut Frame, area: Rect, app: &App) {
    let inner = popup(f, area, "PROGRESS BOARD + PLAN  (Ctrl+B / /board)");
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
    .style(theme::panel())
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
            .style(theme::panel())
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
    let inner = popup(f, area, "SUBAGENTS + LIVE LOG  (Ctrl+L / /subagents)");
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
        List::new(items).block(block("SUBAGENTS", true)).style(theme::panel()).highlight_style(theme::selected()),
        list_a,
        &mut st,
    );
    let h = log_a.height.saturating_sub(2) as usize;
    let start = app.logs.len().saturating_sub(h);
    let log: Vec<Line> = app.logs[start..].iter().map(|l| Line::styled(l.clone(), theme::muted())).collect();
    f.render_widget(Paragraph::new(log).block(block("LOG TAIL", false)).style(theme::panel()), log_a);
    f.render_widget(
        Paragraph::new(" ↑↓ select · x kill · r retry · f fallback (current harness/model) · u resume · c custom · F5 refresh · Esc close")
            .style(theme::faint()),
        keys,
    );
}

fn draw_diff(f: &mut Frame, area: Rect, app: &App) {
    let inner = popup(f, area, "WORKTREE DIFF  (Ctrl+D / /diff)");
    let [main, keys] = Layout::vertical([Constraint::Min(3), Constraint::Length(1)]).areas(inner);
    let [list_a, diff_a] = Layout::horizontal([Constraint::Percentage(30), Constraint::Percentage(70)]).areas(main);
    let items: Vec<ListItem> = app
        .board_tasks()
        .iter()
        .map(|t| ListItem::new(format!("[{}] {} {}", t.status, t.id, t.subject)))
        .collect();
    let mut st = ListState::default().with_selected((!items.is_empty()).then_some(app.diff_sel));
    f.render_stateful_widget(
        List::new(items).block(block("TASKS", true)).style(theme::panel()).highlight_style(theme::selected()),
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
                    theme::panel(),
                ));
            }
            if let Some(e) = &d.error {
                lines.push(Line::styled(format!(" ERROR {e}"), theme::error()));
            }
            lines.push(Line::styled("", theme::panel()));
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
        Paragraph::new(lines).block(block("DIFF", false)).style(theme::panel()).scroll((app.diff_scroll, 0)),
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
    let inner = popup(
        f,
        area,
        if editing { "ROUTING CONFIG — EDITING  (Ctrl+R)" } else { "ROUTING CONFIG  (Ctrl+R / /routing)" },
    );
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
                        Span::styled(a, theme::panel()),
                        Span::styled(cur, theme::selected()),
                        Span::styled(bc.collect::<String>(), theme::panel()),
                    ])
                } else {
                    Line::styled(l.clone(), theme::panel())
                }
            })
            .collect();
        f.render_widget(Paragraph::new(lines).style(theme::panel()), main);
        f.render_widget(Paragraph::new(" Ctrl+S save (POST /api/routing) · Esc cancel edit").style(theme::faint()), keys);
    } else {
        let text = app
            .routing
            .as_ref()
            .map(|v| serde_json::to_string_pretty(v).unwrap_or_default())
            .unwrap_or_else(|| "Loading GET /api/routing…".into());
        f.render_widget(Paragraph::new(text).style(theme::panel()).scroll((app.routing_scroll, 0)), main);
        f.render_widget(
            Paragraph::new(" ↑↓ scroll · e edit · x reset to defaults · r reload · Esc close").style(theme::faint()),
            keys,
        );
    }
}

fn draw_terminal(f: &mut Frame, area: Rect, app: &App) {
    let inner = popup(f, area, "INTERACTIVE RESUME / OS TERMINAL  (Ctrl+T / /terminal)");
    let mut lines = vec![];
    match &app.resume {
        None => lines.push(Line::styled("No resume command loaded (select an ASD session).", theme::faint())),
        Some(r) => {
            lines.push(Line::from(vec![
                Span::styled(" HARNESS ", theme::badge()),
                Span::styled(format!(" {}", r.harness), theme::bold()),
            ]));
            lines.push(Line::styled(format!("session  {}", r.harness_session_id.as_deref().unwrap_or("-")), theme::muted()));
            lines.push(Line::styled(format!("cwd      {}", r.cwd), theme::muted()));
            lines.push(Line::styled("", theme::panel()));
            lines.push(Line::styled("command:", theme::bold()));
            lines.push(Line::styled(format!("  {}", r.cli_string), theme::selected()));
            if let Some(e) = &r.error {
                lines.push(Line::styled(format!(" ERROR {e}"), theme::error()));
            }
        }
    }
    lines.push(Line::styled("", theme::panel()));
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
    lines.push(Line::styled("", theme::panel()));
    lines.push(Line::styled(
        "Enter/o launch in OS terminal · k/Tab terminal kind · r reload · Esc close",
        theme::faint(),
    ));
    f.render_widget(Paragraph::new(lines).style(theme::panel()).wrap(Wrap { trim: false }), inner);
}

fn draw_picker(f: &mut Frame, area: Rect, app: &App) {
    let started = app.is_session_started();
    let title = if started {
        "MODEL PICKER  (Ctrl+K / /models)"
    } else {
        "HARNESS & MODEL  (Ctrl+K / /models)"
    };
    let inner = popup(f, area, title);
    let [tabs, q, list_a, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(inner);
    if started {
        let spans = vec![
            Span::styled(format!(" {} ", app.harness), theme::harness_badge()),
            Span::styled("  (harness locked for started session; /new to switch)", theme::muted()),
        ];
        f.render_widget(Paragraph::new(Line::from(spans)).style(theme::panel()), tabs);
    } else {
        let spans: Vec<Span> = HARNESSES
            .iter()
            .enumerate()
            .map(|(i, h)| {
                Span::styled(
                    format!(" {h} "),
                    if i == app.picker_harness { theme::selected() } else { theme::muted() },
                )
            })
            .collect();
        f.render_widget(Paragraph::new(Line::from(spans)).style(theme::panel()), tabs);
    }
    f.render_widget(Paragraph::new(format!(" search: {}█", app.picker_query)).style(theme::bold()), q);
    let active_harness = if started {
        app.harness.as_str()
    } else {
        HARNESSES[app.picker_harness]
    };
    let models = app.picker_models();
    let items: Vec<ListItem> = models
        .iter()
        .map(|m| {
            let cur = app.harness == active_harness
                && (app.model.as_deref() == Some(m.as_str()) || (app.model.is_none() && m == "(default)"));
            ListItem::new(format!("{} {m}", if cur { "●" } else { " " }))
        })
        .collect();
    let mut st = ListState::default().with_selected(Some(app.picker_sel));
    f.render_stateful_widget(
        List::new(items).style(theme::panel()).highlight_style(theme::selected()),
        list_a,
        &mut st,
    );
    let hint = if started {
        " type to filter · ↑↓ select · F5 refresh models · Enter apply · Esc close"
    } else {
        " type to filter · ↑↓ select · Tab/←→ harness · F5 refresh models · Enter apply · Esc close"
    };
    f.render_widget(Paragraph::new(hint).style(theme::faint()), keys);
}

fn draw_permission_modal(f: &mut Frame, area: Rect, app: &App) {
    let Some((_, perm)) = app.pending_permissions.front() else {
        return;
    };
    let title = if app.pending_permissions.len() > 1 {
        format!("PERMISSION APPROVAL ({} pending)", app.pending_permissions.len())
    } else {
        "PERMISSION APPROVAL".into()
    };
    let a = centered(area, 72, 58);
    let inner = popup(f, a, &title);

    let option_labels: Vec<String> = if perm.options.is_empty() {
        vec!["Allow once (allow_once)".into(), "Reject (reject_once)".into()]
    } else {
        perm.options
            .iter()
            .map(|o| format!("{} ({})", o.name, o.kind))
            .collect()
    };
    let opts_h = (option_labels.len() as u16).clamp(2, 6);

    let [details_a, opts_a, keys_a] = Layout::vertical([
        Constraint::Min(4),
        Constraint::Length(opts_h),
        Constraint::Length(1),
    ])
    .areas(inner);

    let mut detail_lines: Vec<Line> = Vec::new();
    let mut tool_spans = vec![
        Span::styled(" Tool: ", theme::muted()),
        Span::styled(perm.tool_name.clone(), theme::bold()),
    ];
    if let Some(kind) = &perm.tool_kind {
        tool_spans.push(Span::styled(format!("  [{kind}]"), theme::harness_badge()));
    }
    detail_lines.push(Line::from(tool_spans));

    if let Some(locs) = &perm.locations {
        if !locs.is_empty() {
            detail_lines.push(Line::from(vec![
                Span::styled(" Locations: ", theme::muted()),
                Span::styled(locs.join(", "), theme::panel()),
            ]));
        }
    }

    if let Some(input) = &perm.tool_input {
        if !input.trim().is_empty() {
            detail_lines.push(Line::styled(" Input:", theme::muted()));
            let preview: String = input.chars().take(400).collect();
            for line in wrap(&preview, details_a.width.saturating_sub(4) as usize) {
                detail_lines.push(Line::styled(format!("   {line}"), theme::panel()));
            }
        }
    }
    f.render_widget(
        Paragraph::new(detail_lines)
            .style(theme::panel())
            .wrap(Wrap { trim: false }),
        details_a,
    );

    let items: Vec<ListItem> = option_labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let marker = if i == app.permission_sel { "▶" } else { " " };
            ListItem::new(format!(" {marker} {label}"))
        })
        .collect();
    let mut st = ListState::default().with_selected(Some(app.permission_sel.min(items.len().saturating_sub(1))));
    f.render_stateful_widget(
        List::new(items)
            .style(theme::panel())
            .highlight_style(theme::selected()),
        opts_a,
        &mut st,
    );
    f.render_widget(
        Paragraph::new(" ↑/↓ select · Enter confirm · y allow · a always · n/Esc reject").style(theme::faint()),
        keys_a,
    );
}

fn draw_help(f: &mut Frame, area: Rect) {
    let inner = popup(f, area, "HELP  (F1 / /help)");
    let rows = [
        ("Slash Cmds", "/sessions (/s) · /cd [dir] · /new · /models · /harness [name] · /theme · /sidebar · /clear · /exit"),
        ("Harness /", "Any harness command (e.g. /compact, /asd, /review) appears in / popup & sends to turn stream"),
        ("@ & !", "@path attaches file with autocomplete · !cmd runs local shell in project_root"),
        ("Input", "Enter send · Shift+Enter/Alt+Enter newline · ↑↓←→/Home/End cursor · Tab switch harness"),
        ("Palette", "Ctrl+P command palette · Ctrl+K model picker · Ctrl+X cancel stream · Ctrl+E fold/unfold all"),
        ("Modals", "Ctrl+B board+PLAN · Ctrl+L subagents · Ctrl+D diff · Ctrl+R routing · Ctrl+T OS terminal"),
        ("Mouse", "Wheel scrolls timeline/modals · Click selects / or @ popup item or folds Thinking/Tool cards"),
    ];
    let lines: Vec<Line> = rows
        .iter()
        .map(|(a, b)| {
            Line::from(vec![
                Span::styled(format!(" {a:<12}"), theme::bold()),
                Span::styled(*b, theme::muted()),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines).style(theme::panel()).wrap(Wrap { trim: false }), inner);
}
