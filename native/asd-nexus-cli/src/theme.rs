//! Pure monochrome theme. Only black / white / grays (r == g == b) and
//! REVERSED / BOLD / DIM modifiers are allowed. No colored accents anywhere.

use ratatui::style::{Color, Modifier, Style};

pub const BG: Color = Color::Black;
pub const FG: Color = Color::White;
pub const MUTED: Color = Color::Gray;
pub const FAINT: Color = Color::DarkGray;

/// Grayscale RGB helper: always emits r == g == b.
pub const fn gray(v: u8) -> Color {
    Color::Rgb(v, v, v)
}

/// True when the color is strictly achromatic.
pub fn is_grayscale(c: Color) -> bool {
    match c {
        Color::Black | Color::White | Color::Gray | Color::DarkGray => true,
        Color::Rgb(r, g, b) => r == g && g == b,
        _ => false,
    }
}

pub fn base() -> Style {
    Style::default().fg(FG).bg(BG)
}
pub fn muted() -> Style {
    Style::default().fg(MUTED).bg(BG)
}
pub fn faint() -> Style {
    Style::default().fg(FAINT).bg(BG)
}
pub fn bold() -> Style {
    base().add_modifier(Modifier::BOLD)
}
pub fn dim() -> Style {
    muted().add_modifier(Modifier::DIM)
}
pub fn title() -> Style {
    base().add_modifier(Modifier::BOLD)
}
pub fn selected() -> Style {
    base().add_modifier(Modifier::REVERSED | Modifier::BOLD)
}
/// Inverted badge (used for errors / emphasis — contrast instead of color).
pub fn badge() -> Style {
    Style::default()
        .fg(BG)
        .bg(FG)
        .add_modifier(Modifier::BOLD)
}
pub fn error() -> Style {
    base().add_modifier(Modifier::BOLD | Modifier::REVERSED)
}
pub fn border(focused: bool) -> Style {
    if focused {
        bold()
    } else {
        faint()
    }
}
pub fn panel() -> Style {
    Style::default().fg(FG).bg(gray(12))
}
pub fn header() -> Style {
    Style::default()
        .fg(FG)
        .bg(gray(28))
        .add_modifier(Modifier::BOLD)
}
pub fn footer() -> Style {
    Style::default().fg(MUTED).bg(gray(20))
}
pub fn thinking() -> Style {
    muted().add_modifier(Modifier::ITALIC)
}
pub fn tool() -> Style {
    bold()
}
pub fn diff_add() -> Style {
    bold()
}
pub fn diff_del() -> Style {
    faint().add_modifier(Modifier::DIM)
}

/// Every theme style helper, for palette verification.
pub fn all_styles() -> Vec<(&'static str, Style)> {
    vec![
        ("base", base()),
        ("muted", muted()),
        ("faint", faint()),
        ("bold", bold()),
        ("dim", dim()),
        ("title", title()),
        ("selected", selected()),
        ("badge", badge()),
        ("error", error()),
        ("border_focused", border(true)),
        ("border", border(false)),
        ("panel", panel()),
        ("header", header()),
        ("footer", footer()),
        ("thinking", thinking()),
        ("tool", tool()),
        ("diff_add", diff_add()),
        ("diff_del", diff_del()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_color_is_grayscale() {
        for c in [BG, FG, MUTED, FAINT, gray(0), gray(128), gray(255)] {
            assert!(is_grayscale(c), "{c:?}");
        }
        for (name, s) in all_styles() {
            assert!(s.fg.is_some() || s.bg.is_some(), "{name} sets no color");
            if let Some(fg) = s.fg {
                assert!(is_grayscale(fg), "{name} fg {fg:?}");
            }
            if let Some(bg) = s.bg {
                assert!(is_grayscale(bg), "{name} bg {bg:?}");
            }
            assert!(s.underline_color.is_none(), "{name} underline color");
        }
    }

    #[test]
    fn grayscale_detector_rejects_colors() {
        for c in [
            Color::Red,
            Color::Blue,
            Color::Cyan,
            Color::Green,
            Color::Yellow,
            Color::Magenta,
            Color::LightBlue,
            Color::Indexed(33),
            Color::Rgb(10, 20, 10),
            Color::Reset,
        ] {
            assert!(!is_grayscale(c), "{c:?}");
        }
    }
}
