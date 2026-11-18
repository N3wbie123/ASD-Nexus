//! Dual-mode visual system: Antigravity Dark Slate (`ThemeMode::DarkColor`, default)
//! and strict pure grayscale (`ThemeMode::Monochrome`).
//!
//! - In `ThemeMode::DarkColor`, surfaces use deep Antigravity slate tones
//!   (`#0d1117` / `#131720` / `#1a202c`), 1px hairline borders, cyan/blue
//!   focus & action accents, emerald diff additions, rose diff deletions, and
//!   rounded cards.
//! - In `ThemeMode::Monochrome`, every color anywhere in the UI is strictly
//!   grayscale (`r == g == b`, `is_grayscale`), ranging from `#0a0a0a` to `#ffffff`.

use egui::{Color32, FontFamily, FontId, Rounding, Shadow, Stroke, TextStyle, Visuals};

/// Active visual theme mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ThemeMode {
    /// Antigravity dark slate color palette (default).
    #[default]
    DarkColor,
    /// Strict grayscale palette (`r == g == b`).
    Monochrome,
}

impl ThemeMode {
    pub fn label(self) -> &'static str {
        match self {
            ThemeMode::DarkColor => "Dark",
            ThemeMode::Monochrome => "Mono",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            ThemeMode::DarkColor => ThemeMode::Monochrome,
            ThemeMode::Monochrome => ThemeMode::DarkColor,
        }
    }
}

/// Build an opaque gray `Color32` with equal RGB channels.
pub const fn gray(v: u8) -> Color32 {
    Color32::from_rgb(v, v, v)
}

// Strict monochrome constants (kept for backwards compatibility and Monochrome mode).
pub const BG: Color32 = gray(0x0a);
pub const PANEL: Color32 = gray(0x11);
pub const SURFACE: Color32 = gray(0x17);
pub const SURFACE_HI: Color32 = gray(0x1f);
pub const RAISED: Color32 = gray(0x26);
pub const BORDER: Color32 = gray(0x33);
pub const BORDER_HI: Color32 = gray(0x55);
pub const MUTED: Color32 = gray(0x6e);
pub const DIM: Color32 = gray(0x8a);
pub const TEXT: Color32 = gray(0xd4);
pub const TEXT_HI: Color32 = gray(0xee);
pub const WHITE: Color32 = gray(0xff);
pub const BLACK: Color32 = gray(0x0a);

/// Named monochrome palette (used by tests and the status bar).
pub const PALETTE: &[(&str, Color32)] = &[
    ("bg", BG),
    ("panel", PANEL),
    ("surface", SURFACE),
    ("surface_hi", SURFACE_HI),
    ("raised", RAISED),
    ("border", BORDER),
    ("border_hi", BORDER_HI),
    ("muted", MUTED),
    ("dim", DIM),
    ("text", TEXT),
    ("text_hi", TEXT_HI),
    ("white", WHITE),
    ("black", BLACK),
];

/// Semantic palette resolved for a given [`ThemeMode`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemePalette {
    pub bg: Color32,
    pub panel: Color32,
    pub surface: Color32,
    pub surface_hi: Color32,
    pub raised: Color32,
    pub code_bg: Color32,
    pub border: Color32,
    pub border_hi: Color32,
    pub muted: Color32,
    pub dim: Color32,
    pub text: Color32,
    pub text_hi: Color32,
    pub white: Color32,
    pub black: Color32,
    pub accent: Color32,
    pub accent_fill: Color32,
    pub accent_bg: Color32,
    pub user_bubble: Color32,
    pub user_border: Color32,
    pub card_rounding: f32,
    pub pill_rounding: f32,
}

pub const DARK_COLOR_PALETTE: ThemePalette = ThemePalette {
    bg: Color32::from_rgb(0x0d, 0x11, 0x17),
    panel: Color32::from_rgb(0x13, 0x17, 0x20),
    surface: Color32::from_rgb(0x1a, 0x20, 0x2c),
    surface_hi: Color32::from_rgb(0x21, 0x29, 0x38),
    raised: Color32::from_rgb(0x28, 0x32, 0x44),
    code_bg: Color32::from_rgb(0x11, 0x16, 0x21),
    border: Color32::from_rgb(0x25, 0x2f, 0x3f),
    border_hi: Color32::from_rgb(0x3b, 0x4b, 0x63),
    muted: Color32::from_rgb(0x64, 0x74, 0x8b),
    dim: Color32::from_rgb(0x94, 0xa3, 0xb8),
    text: Color32::from_rgb(0xe2, 0xe8, 0xf0),
    text_hi: Color32::from_rgb(0xf8, 0xfa, 0xfc),
    white: Color32::from_rgb(0xff, 0xff, 0xff),
    black: Color32::from_rgb(0x09, 0x0d, 0x14),
    accent: Color32::from_rgb(0x38, 0xbd, 0xf8),
    accent_fill: Color32::from_rgb(0x25, 0x63, 0xeb),
    accent_bg: Color32::from_rgb(0x11, 0x28, 0x40),
    user_bubble: Color32::from_rgb(0x17, 0x25, 0x3b),
    user_border: Color32::from_rgb(0x2d, 0x4a, 0x73),
    card_rounding: 8.0,
    pill_rounding: 6.0,
};

pub const MONOCHROME_PALETTE: ThemePalette = ThemePalette {
    bg: BG,
    panel: PANEL,
    surface: SURFACE,
    surface_hi: SURFACE_HI,
    raised: RAISED,
    code_bg: SURFACE_HI,
    border: BORDER,
    border_hi: BORDER_HI,
    muted: MUTED,
    dim: DIM,
    text: TEXT,
    text_hi: TEXT_HI,
    white: WHITE,
    black: BLACK,
    accent: WHITE,
    accent_fill: RAISED,
    accent_bg: SURFACE_HI,
    user_bubble: RAISED,
    user_border: BORDER_HI,
    card_rounding: 6.0,
    pill_rounding: 4.0,
};

/// Resolve the semantic palette for `mode`.
pub const fn palette(mode: ThemeMode) -> ThemePalette {
    match mode {
        ThemeMode::DarkColor => DARK_COLOR_PALETTE,
        ThemeMode::Monochrome => MONOCHROME_PALETTE,
    }
}

/// `true` when the color carries no hue (all RGB channels equal).
pub fn is_grayscale(c: Color32) -> bool {
    c.r() == c.g() && c.g() == c.b()
}

/// Build the monochrome egui `Visuals` (strictly grayscale).
pub fn visuals() -> Visuals {
    let mut v = Visuals::dark();
    v.dark_mode = true;
    v.override_text_color = Some(TEXT);
    v.hyperlink_color = TEXT_HI;
    v.faint_bg_color = SURFACE;
    v.extreme_bg_color = BG;
    v.code_bg_color = SURFACE_HI;
    v.warn_fg_color = TEXT_HI;
    v.error_fg_color = WHITE;
    v.window_fill = PANEL;
    v.panel_fill = PANEL;
    v.window_stroke = Stroke::new(1.0_f32, BORDER);
    v.window_rounding = Rounding::same(4.0);
    v.menu_rounding = Rounding::same(4.0);
    v.window_shadow = Shadow { color: Color32::from_black_alpha(110), ..v.window_shadow };
    v.popup_shadow = Shadow { color: Color32::from_black_alpha(110), ..v.popup_shadow };
    v.selection.bg_fill = gray(0x3a);
    v.selection.stroke = Stroke::new(1.0_f32, WHITE);
    v.text_cursor.stroke = Stroke::new(2.0_f32, WHITE);
    v.striped = true;

    let w = &mut v.widgets;
    let r = Rounding::same(4.0);

    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.weak_bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    w.noninteractive.rounding = r;

    w.inactive.bg_fill = SURFACE_HI;
    w.inactive.weak_bg_fill = SURFACE;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    w.inactive.rounding = r;

    w.hovered.bg_fill = RAISED;
    w.hovered.weak_bg_fill = RAISED;
    w.hovered.bg_stroke = Stroke::new(1.0_f32, BORDER_HI);
    w.hovered.fg_stroke = Stroke::new(1.5_f32, TEXT_HI);
    w.hovered.rounding = r;

    w.active.bg_fill = gray(0x3a);
    w.active.weak_bg_fill = gray(0x3a);
    w.active.bg_stroke = Stroke::new(1.0_f32, WHITE);
    w.active.fg_stroke = Stroke::new(2.0_f32, WHITE);
    w.active.rounding = r;

    w.open.bg_fill = SURFACE_HI;
    w.open.weak_bg_fill = SURFACE_HI;
    w.open.bg_stroke = Stroke::new(1.0_f32, BORDER_HI);
    w.open.fg_stroke = Stroke::new(1.0_f32, TEXT_HI);
    w.open.rounding = r;

    v
}

/// Build the Antigravity dark slate egui `Visuals`.
pub fn dark_color_visuals() -> Visuals {
    let p = DARK_COLOR_PALETTE;
    let mut v = Visuals::dark();
    v.dark_mode = true;
    v.override_text_color = Some(p.text);
    v.hyperlink_color = p.accent;
    v.faint_bg_color = p.surface;
    v.extreme_bg_color = p.bg;
    v.code_bg_color = p.code_bg;
    v.warn_fg_color = Color32::from_rgb(0xfb, 0xbf, 0x24);
    v.error_fg_color = Color32::from_rgb(0xf8, 0x71, 0x71);
    v.window_fill = p.panel;
    v.panel_fill = p.panel;
    v.window_stroke = Stroke::new(1.0_f32, p.border);
    v.window_rounding = Rounding::same(8.0);
    v.menu_rounding = Rounding::same(6.0);
    v.window_shadow = Shadow { color: Color32::from_black_alpha(120), ..v.window_shadow };
    v.popup_shadow = Shadow { color: Color32::from_black_alpha(120), ..v.popup_shadow };
    v.selection.bg_fill = Color32::from_rgb(0x1d, 0x4e, 0xd8);
    v.selection.stroke = Stroke::new(1.0_f32, p.accent);
    v.text_cursor.stroke = Stroke::new(2.0_f32, p.accent);
    v.striped = true;

    let w = &mut v.widgets;
    let r = Rounding::same(6.0);

    w.noninteractive.bg_fill = p.panel;
    w.noninteractive.weak_bg_fill = p.panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.noninteractive.rounding = r;

    w.inactive.bg_fill = p.surface_hi;
    w.inactive.weak_bg_fill = p.surface;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, p.text);
    w.inactive.rounding = r;

    w.hovered.bg_fill = p.raised;
    w.hovered.weak_bg_fill = p.raised;
    w.hovered.bg_stroke = Stroke::new(1.0_f32, p.accent);
    w.hovered.fg_stroke = Stroke::new(1.5_f32, p.text_hi);
    w.hovered.rounding = r;

    w.active.bg_fill = p.accent_fill;
    w.active.weak_bg_fill = p.accent_fill;
    w.active.bg_stroke = Stroke::new(1.0_f32, p.accent);
    w.active.fg_stroke = Stroke::new(2.0_f32, p.white);
    w.active.rounding = r;

    w.open.bg_fill = p.surface_hi;
    w.open.weak_bg_fill = p.surface_hi;
    w.open.bg_stroke = Stroke::new(1.0_f32, p.accent);
    w.open.fg_stroke = Stroke::new(1.0_f32, p.text_hi);
    w.open.rounding = r;

    v
}

/// Build `Visuals` for the requested [`ThemeMode`].
pub fn visuals_for(mode: ThemeMode) -> Visuals {
    match mode {
        ThemeMode::DarkColor => dark_color_visuals(),
        ThemeMode::Monochrome => visuals(),
    }
}

/// Apply the specified [`ThemeMode`] and typography styles to a context.
pub fn apply_mode(ctx: &egui::Context, mode: ThemeMode) {
    ctx.set_visuals(visuals_for(mode));
    ctx.style_mut(|s| {
        s.text_styles.insert(TextStyle::Body, FontId::new(15.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Button, FontId::new(14.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Small, FontId::new(12.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Heading, FontId::new(20.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Monospace, FontId::new(14.0, FontFamily::Monospace));
        s.spacing.item_spacing = egui::vec2(9.0, 7.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.interact_size.y = 26.0;
    });
}

/// Apply the default theme (`ThemeMode::DarkColor`) to a context.
pub fn apply(ctx: &egui::Context) {
    apply_mode(ctx, ThemeMode::default());
}

/// Extract every `Color32` rendered in an egui `Debug` dump
/// (`#RR_GG_BB_AA` format). Used to assert that *all* colors reachable from
/// `Visuals` are grayscale in `ThemeMode::Monochrome`.
pub fn debug_colors(dump: &str) -> Vec<[u8; 4]> {
    let bytes = dump.as_bytes();
    let mut out = Vec::new();
    let hex = |s: &[u8]| -> Option<u8> { u8::from_str_radix(std::str::from_utf8(s).ok()?, 16).ok() };
    let mut i = 0;
    while i + 12 <= bytes.len() {
        let w = &bytes[i..i + 12];
        if w[0] == b'#' && w[3] == b'_' && w[6] == b'_' && w[9] == b'_' {
            if let (Some(r), Some(g), Some(b), Some(a)) =
                (hex(&w[1..3]), hex(&w[4..6]), hex(&w[7..9]), hex(&w[10..12]))
            {
                out.push([r, g, b, a]);
                i += 12;
                continue;
            }
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Semantic styles: badges and diff indicators
// ---------------------------------------------------------------------------

/// Badge style: (text color, fill, stroke).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BadgeStyle {
    pub fg: Color32,
    pub bg: Color32,
    pub stroke: Stroke,
}

impl BadgeStyle {
    pub fn colors(&self) -> [Color32; 3] {
        [self.fg, self.bg, self.stroke.color]
    }
}

/// Monochrome badge for a task / subagent status.
pub fn status_badge(status: &str) -> BadgeStyle {
    let s = status.to_ascii_lowercase();
    match s.as_str() {
        "done" | "completed" | "complete" | "merged" | "approved" | "success" => BadgeStyle {
            fg: BLACK,
            bg: TEXT_HI,
            stroke: Stroke::new(1.0_f32, TEXT_HI),
        },
        "failed" | "error" | "killed" | "rejected" | "crashed" | "interrupted" => BadgeStyle {
            fg: WHITE,
            bg: BG,
            stroke: Stroke::new(2.0_f32, WHITE),
        },
        "running" | "in_progress" | "in-progress" | "active" | "claimed" | "review" | "pending_review" => {
            BadgeStyle { fg: TEXT_HI, bg: RAISED, stroke: Stroke::new(1.0_f32, BORDER_HI) }
        }
        _ => BadgeStyle { fg: DIM, bg: SURFACE, stroke: Stroke::new(1.0_f32, BORDER) },
    }
}

/// Mode-aware status badge (`ThemeMode::DarkColor` uses semantic emerald/amber/cyan/rose;
/// `ThemeMode::Monochrome` is strictly grayscale).
pub fn status_badge_for(mode: ThemeMode, status: &str) -> BadgeStyle {
    if mode == ThemeMode::Monochrome {
        return status_badge(status);
    }
    let s = status.to_ascii_lowercase();
    match s.as_str() {
        "done" | "completed" | "complete" | "merged" | "approved" | "success" | "connected" => BadgeStyle {
            fg: Color32::from_rgb(0x4a, 0xde, 0x80),
            bg: Color32::from_rgb(0x0f, 0x29, 0x1e),
            stroke: Stroke::new(1.0_f32, Color32::from_rgb(0x16, 0x65, 0x34)),
        },
        "failed" | "error" | "killed" | "rejected" | "crashed" | "interrupted" | "offline" => BadgeStyle {
            fg: Color32::from_rgb(0xf8, 0x71, 0x71),
            bg: Color32::from_rgb(0x2d, 0x16, 0x1b),
            stroke: Stroke::new(1.5_f32, Color32::from_rgb(0x99, 0x1b, 0x1b)),
        },
        "review" | "pending_review" | "unsaved" => BadgeStyle {
            fg: Color32::from_rgb(0xfb, 0xbf, 0x24),
            bg: Color32::from_rgb(0x2b, 0x21, 0x11),
            stroke: Stroke::new(1.0_f32, Color32::from_rgb(0xb4, 0x53, 0x09)),
        },
        "running" | "in_progress" | "in-progress" | "active" | "claimed" | "connecting" | "nexus" => BadgeStyle {
            fg: Color32::from_rgb(0x38, 0xbd, 0xf8),
            bg: Color32::from_rgb(0x11, 0x28, 0x40),
            stroke: Stroke::new(1.0_f32, Color32::from_rgb(0x02, 0x84, 0xc7)),
        },
        _ => BadgeStyle {
            fg: DARK_COLOR_PALETTE.dim,
            bg: DARK_COLOR_PALETTE.surface,
            stroke: Stroke::new(1.0_f32, DARK_COLOR_PALETTE.border),
        },
    }
}

/// Classification of a unified diff line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Meta,
    Hunk,
    Add,
    Del,
    Context,
}

pub fn classify_diff_line(line: &str) -> DiffLineKind {
    if line.starts_with("+++") || line.starts_with("---") || line.starts_with("diff ") || line.starts_with("index ") {
        DiffLineKind::Meta
    } else if line.starts_with("@@") {
        DiffLineKind::Hunk
    } else if line.starts_with('+') {
        DiffLineKind::Add
    } else if line.starts_with('-') {
        DiffLineKind::Del
    } else {
        DiffLineKind::Context
    }
}

/// Diff line style: (text color, background, strikethrough, strong).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiffStyle {
    pub fg: Color32,
    pub bg: Color32,
    pub strike: bool,
    pub strong: bool,
}

/// Monochrome diff style.
pub fn diff_style(kind: DiffLineKind) -> DiffStyle {
    match kind {
        DiffLineKind::Meta => DiffStyle { fg: TEXT_HI, bg: SURFACE_HI, strike: false, strong: true },
        DiffLineKind::Hunk => DiffStyle { fg: DIM, bg: SURFACE, strike: false, strong: false },
        DiffLineKind::Add => DiffStyle { fg: WHITE, bg: RAISED, strike: false, strong: true },
        DiffLineKind::Del => DiffStyle { fg: MUTED, bg: BG, strike: true, strong: false },
        DiffLineKind::Context => DiffStyle { fg: TEXT, bg: Color32::TRANSPARENT, strike: false, strong: false },
    }
}

/// Mode-aware diff style.
pub fn diff_style_for(mode: ThemeMode, kind: DiffLineKind) -> DiffStyle {
    if mode == ThemeMode::Monochrome {
        return diff_style(kind);
    }
    match kind {
        DiffLineKind::Meta => DiffStyle {
            fg: DARK_COLOR_PALETTE.text_hi,
            bg: DARK_COLOR_PALETTE.surface_hi,
            strike: false,
            strong: true,
        },
        DiffLineKind::Hunk => DiffStyle {
            fg: DARK_COLOR_PALETTE.accent,
            bg: DARK_COLOR_PALETTE.accent_bg,
            strike: false,
            strong: false,
        },
        DiffLineKind::Add => DiffStyle {
            fg: Color32::from_rgb(0x4a, 0xde, 0x80),
            bg: Color32::from_rgb(0x0f, 0x29, 0x1e),
            strike: false,
            strong: true,
        },
        DiffLineKind::Del => DiffStyle {
            fg: Color32::from_rgb(0xf8, 0x71, 0x71),
            bg: Color32::from_rgb(0x2d, 0x16, 0x1b),
            strike: false,
            strong: false,
        },
        DiffLineKind::Context => DiffStyle {
            fg: DARK_COLOR_PALETTE.text,
            bg: Color32::TRANSPARENT,
            strike: false,
            strong: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_is_strictly_grayscale_and_in_range() {
        for (name, c) in PALETTE {
            assert!(is_grayscale(*c), "{name} is not grayscale: {c:?}");
            assert!(c.r() >= 0x0a, "{name} darker than #0a0a0a");
            assert_eq!(c.a(), 255, "{name} must be opaque");
        }
    }

    #[test]
    fn visuals_contain_only_grayscale_colors() {
        let dump = format!("{:?}", visuals_for(ThemeMode::Monochrome));
        let colors = debug_colors(&dump);
        assert!(colors.len() > 20, "expected many colors in Visuals dump, got {}", colors.len());
        for [r, g, b, a] in colors {
            assert!(r == g && g == b, "non-grayscale color #{r:02x}{g:02x}{b:02x}{a:02x} in Visuals");
        }
    }

    #[test]
    fn dark_color_theme_is_default_and_has_slate_accents() {
        assert_eq!(ThemeMode::default(), ThemeMode::DarkColor);
        assert_eq!(ThemeMode::DarkColor.toggle(), ThemeMode::Monochrome);
        let p = palette(ThemeMode::DarkColor);
        assert!(!is_grayscale(p.bg));
        assert!(!is_grayscale(p.accent));
        let add = diff_style_for(ThemeMode::DarkColor, DiffLineKind::Add);
        let del = diff_style_for(ThemeMode::DarkColor, DiffLineKind::Del);
        assert!(add.fg.g() > add.fg.r());
        assert!(del.fg.r() > del.fg.g());
    }

    #[test]
    fn badges_and_diff_styles_are_grayscale_in_monochrome() {
        for s in ["done", "failed", "running", "pending", "weird", "merged", "killed"] {
            for c in status_badge_for(ThemeMode::Monochrome, s).colors() {
                assert!(is_grayscale(c), "badge {s}: {c:?}");
            }
        }
        for k in [DiffLineKind::Meta, DiffLineKind::Hunk, DiffLineKind::Add, DiffLineKind::Del, DiffLineKind::Context] {
            let st = diff_style_for(ThemeMode::Monochrome, k);
            assert!(is_grayscale(st.fg) && is_grayscale(st.bg), "{k:?}");
        }
    }

    #[test]
    fn diff_classification() {
        assert_eq!(classify_diff_line("+++ b/x"), DiffLineKind::Meta);
        assert_eq!(classify_diff_line("--- a/x"), DiffLineKind::Meta);
        assert_eq!(classify_diff_line("@@ -1 +1 @@"), DiffLineKind::Hunk);
        assert_eq!(classify_diff_line("+add"), DiffLineKind::Add);
        assert_eq!(classify_diff_line("-del"), DiffLineKind::Del);
        assert_eq!(classify_diff_line(" ctx"), DiffLineKind::Context);
    }

    #[test]
    fn text_styles_are_readable() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let style = ctx.style();
        assert!(style.text_styles[&TextStyle::Body].size >= 15.0);
        assert!(style.text_styles[&TextStyle::Small].size >= 12.0);
        assert!(style.text_styles[&TextStyle::Monospace].size >= 14.0);
    }

    #[test]
    fn debug_color_parser() {
        let c = debug_colors("a #0A_0A_0A_FF b #12_34_56_78 #zz_00_00_00");
        assert_eq!(c, vec![[10, 10, 10, 255], [0x12, 0x34, 0x56, 0x78]]);
    }
}
