//! Pure monochrome visual system.
//!
//! Every color used anywhere in the desktop UI comes from this module and is
//! strictly grayscale (`r == g == b`), ranging from `#0a0a0a` to `#ffffff`.
//! There are no colored accents: emphasis is expressed through contrast,
//! inversion (white-on-black badges), stroke weight, and typography.

use egui::{Color32, Rounding, Shadow, Stroke, Visuals};

/// Build an opaque gray `Color32` with equal RGB channels.
pub const fn gray(v: u8) -> Color32 {
    Color32::from_rgb(v, v, v)
}

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

/// Named palette (used by tests and the legend in the status bar).
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

/// `true` when the color carries no hue (all RGB channels equal).
pub fn is_grayscale(c: Color32) -> bool {
    c.r() == c.g() && c.g() == c.b()
}

/// Build the monochrome egui `Visuals`.
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
    v.window_rounding = Rounding::same(2.0);
    v.menu_rounding = Rounding::same(2.0);
    v.window_shadow = Shadow { color: Color32::from_black_alpha(110), ..v.window_shadow };
    v.popup_shadow = Shadow { color: Color32::from_black_alpha(110), ..v.popup_shadow };
    v.selection.bg_fill = gray(0x3a);
    v.selection.stroke = Stroke::new(1.0_f32, WHITE);
    v.text_cursor.stroke = Stroke::new(2.0_f32, WHITE);
    v.striped = true;

    let w = &mut v.widgets;
    let r = Rounding::same(2.0);

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

/// Apply the monochrome theme to a context.
pub fn apply(ctx: &egui::Context) {
    ctx.set_visuals(visuals());
    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(8.0, 4.0);
    });
}

/// Extract every `Color32` rendered in an egui `Debug` dump
/// (`#RR_GG_BB_AA` format). Used to assert that *all* colors reachable from
/// `Visuals` are grayscale, including ones we did not explicitly set.
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
// Semantic monochrome styles: badges and diff indicators
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
///
/// - terminal success (done/merged/approved): inverted white-on-black
/// - errors (failed/error/killed/rejected): black on white with heavy stroke
/// - running/in-progress: bright text with bright stroke
/// - everything else: muted outline
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

/// Monochrome diff style: (text color, background, strikethrough).
/// Additions are bright on a raised band, deletions are dim + strikethrough.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiffStyle {
    pub fg: Color32,
    pub bg: Color32,
    pub strike: bool,
    pub strong: bool,
}

pub fn diff_style(kind: DiffLineKind) -> DiffStyle {
    match kind {
        DiffLineKind::Meta => DiffStyle { fg: TEXT_HI, bg: SURFACE_HI, strike: false, strong: true },
        DiffLineKind::Hunk => DiffStyle { fg: DIM, bg: SURFACE, strike: false, strong: false },
        DiffLineKind::Add => DiffStyle { fg: WHITE, bg: RAISED, strike: false, strong: true },
        DiffLineKind::Del => DiffStyle { fg: MUTED, bg: BG, strike: true, strong: false },
        DiffLineKind::Context => DiffStyle { fg: TEXT, bg: Color32::TRANSPARENT, strike: false, strong: false },
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
        let dump = format!("{:?}", visuals());
        let colors = debug_colors(&dump);
        assert!(colors.len() > 20, "expected many colors in Visuals dump, got {}", colors.len());
        for [r, g, b, a] in colors {
            assert!(r == g && g == b, "non-grayscale color #{r:02x}{g:02x}{b:02x}{a:02x} in Visuals");
        }
    }

    #[test]
    fn badges_and_diff_styles_are_grayscale() {
        for s in ["done", "failed", "running", "pending", "weird", "merged", "killed"] {
            for c in status_badge(s).colors() {
                assert!(is_grayscale(c), "badge {s}: {c:?}");
            }
        }
        for k in [DiffLineKind::Meta, DiffLineKind::Hunk, DiffLineKind::Add, DiffLineKind::Del, DiffLineKind::Context] {
            let st = diff_style(k);
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
    fn debug_color_parser() {
        let c = debug_colors("a #0A_0A_0A_FF b #12_34_56_78 #zz_00_00_00");
        assert_eq!(c, vec![[10, 10, 10, 255], [0x12, 0x34, 0x56, 0x78]]);
    }
}
