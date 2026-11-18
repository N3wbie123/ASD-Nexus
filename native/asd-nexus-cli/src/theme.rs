//! Dual-mode TUI theme: OpenCode-inspired `DarkColor` (default) and strict
//! `Monochrome` (grayscale `r == g == b` only, toggleable via `/theme`).

use std::cell::Cell;

use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    DarkColor,
    Monochrome,
}

impl ThemeMode {
    pub fn name(self) -> &'static str {
        match self {
            ThemeMode::DarkColor => "dark",
            ThemeMode::Monochrome => "monochrome",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            ThemeMode::DarkColor => ThemeMode::Monochrome,
            ThemeMode::Monochrome => ThemeMode::DarkColor,
        }
    }
}

thread_local! {
    static CURRENT_MODE: Cell<ThemeMode> = const { Cell::new(ThemeMode::DarkColor) };
}

pub fn mode() -> ThemeMode {
    CURRENT_MODE.with(|c| c.get())
}

pub fn set_mode(m: ThemeMode) {
    CURRENT_MODE.with(|c| c.set(m));
}

pub fn toggle_mode() -> ThemeMode {
    let next = mode().toggle();
    set_mode(next);
    next
}

// Monochrome constants
pub const BG: Color = Color::Black;
pub const FG: Color = Color::White;
pub const MUTED: Color = Color::Gray;
pub const FAINT: Color = Color::DarkGray;

// DarkColor (OpenCode slate) constants
pub const DARK_BG: Color = Color::Rgb(13, 15, 20);
pub const DARK_SURFACE: Color = Color::Rgb(22, 26, 35);
pub const DARK_PANEL: Color = Color::Rgb(18, 22, 30);
pub const DARK_HEADER: Color = Color::Rgb(17, 20, 28);
pub const DARK_FOOTER: Color = Color::Rgb(15, 18, 25);
pub const DARK_FG: Color = Color::Rgb(230, 235, 243);
pub const DARK_MUTED: Color = Color::Rgb(148, 160, 180);
pub const DARK_FAINT: Color = Color::Rgb(96, 106, 126);
pub const DARK_ACCENT: Color = Color::Rgb(122, 162, 247);
pub const DARK_CYAN: Color = Color::Rgb(125, 207, 255);
pub const DARK_PURPLE: Color = Color::Rgb(187, 154, 247);
pub const DARK_AMBER: Color = Color::Rgb(224, 175, 104);
pub const DARK_GREEN: Color = Color::Rgb(158, 206, 106);
pub const DARK_RED: Color = Color::Rgb(247, 118, 142);
pub const DARK_SEL_BG: Color = Color::Rgb(38, 46, 66);

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

pub fn base_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_FG).bg(DARK_BG),
        ThemeMode::Monochrome => Style::default().fg(FG).bg(BG),
    }
}

pub fn surface_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_FG).bg(DARK_SURFACE),
        ThemeMode::Monochrome => Style::default().fg(FG).bg(gray(18)),
    }
}

pub fn surface_muted_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_MUTED).bg(DARK_SURFACE),
        ThemeMode::Monochrome => Style::default().fg(MUTED).bg(gray(18)),
    }
}

pub fn surface_faint_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_FAINT).bg(DARK_SURFACE),
        ThemeMode::Monochrome => Style::default().fg(FAINT).bg(gray(18)),
    }
}

pub fn surface_accent_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_ACCENT).bg(DARK_SURFACE).add_modifier(Modifier::BOLD),
        ThemeMode::Monochrome => Style::default().fg(FG).bg(gray(18)).add_modifier(Modifier::BOLD),
    }
}

pub fn muted_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_MUTED).bg(DARK_BG),
        ThemeMode::Monochrome => Style::default().fg(MUTED).bg(BG),
    }
}

pub fn faint_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_FAINT).bg(DARK_BG),
        ThemeMode::Monochrome => Style::default().fg(FAINT).bg(BG),
    }
}

pub fn bold_for(m: ThemeMode) -> Style {
    base_for(m).add_modifier(Modifier::BOLD)
}

pub fn dim_for(m: ThemeMode) -> Style {
    muted_for(m).add_modifier(Modifier::DIM)
}

pub fn title_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_ACCENT).bg(DARK_BG).add_modifier(Modifier::BOLD),
        ThemeMode::Monochrome => base_for(m).add_modifier(Modifier::BOLD),
    }
}

pub fn accent_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_ACCENT).bg(DARK_BG).add_modifier(Modifier::BOLD),
        ThemeMode::Monochrome => base_for(m).add_modifier(Modifier::BOLD),
    }
}

pub fn selected_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_FG).bg(DARK_SEL_BG).add_modifier(Modifier::BOLD),
        ThemeMode::Monochrome => base_for(m).add_modifier(Modifier::REVERSED | Modifier::BOLD),
    }
}

pub fn badge_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_BG).bg(DARK_ACCENT).add_modifier(Modifier::BOLD),
        ThemeMode::Monochrome => Style::default().fg(BG).bg(FG).add_modifier(Modifier::BOLD),
    }
}

pub fn harness_badge_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_CYAN).bg(DARK_SURFACE).add_modifier(Modifier::BOLD),
        ThemeMode::Monochrome => Style::default().fg(FG).bg(gray(36)).add_modifier(Modifier::BOLD),
    }
}

pub fn error_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_RED).bg(DARK_BG).add_modifier(Modifier::BOLD),
        ThemeMode::Monochrome => base_for(m).add_modifier(Modifier::BOLD | Modifier::REVERSED),
    }
}

pub fn border_for(m: ThemeMode, focused: bool) -> Style {
    match m {
        ThemeMode::DarkColor => {
            if focused {
                Style::default().fg(DARK_ACCENT).bg(DARK_BG).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DARK_FAINT).bg(DARK_BG)
            }
        }
        ThemeMode::Monochrome => {
            if focused {
                bold_for(m)
            } else {
                faint_for(m)
            }
        }
    }
}

pub fn panel_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_FG).bg(DARK_PANEL),
        ThemeMode::Monochrome => Style::default().fg(FG).bg(gray(12)),
    }
}

pub fn header_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_FG).bg(DARK_HEADER).add_modifier(Modifier::BOLD),
        ThemeMode::Monochrome => Style::default().fg(FG).bg(gray(28)).add_modifier(Modifier::BOLD),
    }
}

pub fn header_muted_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_MUTED).bg(DARK_HEADER),
        ThemeMode::Monochrome => Style::default().fg(MUTED).bg(gray(28)),
    }
}

pub fn footer_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_MUTED).bg(DARK_FOOTER),
        ThemeMode::Monochrome => Style::default().fg(MUTED).bg(gray(20)),
    }
}

pub fn thinking_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_PURPLE).bg(DARK_BG).add_modifier(Modifier::ITALIC),
        ThemeMode::Monochrome => muted_for(m).add_modifier(Modifier::ITALIC),
    }
}

pub fn tool_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_CYAN).bg(DARK_BG).add_modifier(Modifier::BOLD),
        ThemeMode::Monochrome => bold_for(m),
    }
}

pub fn shell_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_AMBER).bg(DARK_BG).add_modifier(Modifier::BOLD),
        ThemeMode::Monochrome => bold_for(m),
    }
}

pub fn diff_add_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_GREEN).bg(DARK_BG).add_modifier(Modifier::BOLD),
        ThemeMode::Monochrome => bold_for(m),
    }
}

pub fn diff_del_for(m: ThemeMode) -> Style {
    match m {
        ThemeMode::DarkColor => Style::default().fg(DARK_RED).bg(DARK_BG),
        ThemeMode::Monochrome => faint_for(m).add_modifier(Modifier::DIM),
    }
}

pub fn base() -> Style {
    base_for(mode())
}
pub fn surface() -> Style {
    surface_for(mode())
}
pub fn surface_muted() -> Style {
    surface_muted_for(mode())
}
pub fn surface_faint() -> Style {
    surface_faint_for(mode())
}
pub fn surface_accent() -> Style {
    surface_accent_for(mode())
}
pub fn muted() -> Style {
    muted_for(mode())
}
pub fn faint() -> Style {
    faint_for(mode())
}
pub fn bold() -> Style {
    bold_for(mode())
}
pub fn dim() -> Style {
    dim_for(mode())
}
pub fn title() -> Style {
    title_for(mode())
}
pub fn accent() -> Style {
    accent_for(mode())
}
pub fn selected() -> Style {
    selected_for(mode())
}
pub fn badge() -> Style {
    badge_for(mode())
}
pub fn harness_badge() -> Style {
    harness_badge_for(mode())
}
pub fn error() -> Style {
    error_for(mode())
}
pub fn border(focused: bool) -> Style {
    border_for(mode(), focused)
}
pub fn panel() -> Style {
    panel_for(mode())
}
pub fn header() -> Style {
    header_for(mode())
}
pub fn header_muted() -> Style {
    header_muted_for(mode())
}
pub fn footer() -> Style {
    footer_for(mode())
}
pub fn thinking() -> Style {
    thinking_for(mode())
}
pub fn tool() -> Style {
    tool_for(mode())
}
pub fn shell() -> Style {
    shell_for(mode())
}
pub fn diff_add() -> Style {
    diff_add_for(mode())
}
pub fn diff_del() -> Style {
    diff_del_for(mode())
}

/// Every theme style helper for the given mode, for palette verification.
pub fn all_styles_for(m: ThemeMode) -> Vec<(&'static str, Style)> {
    vec![
        ("base", base_for(m)),
        ("surface", surface_for(m)),
        ("surface_muted", surface_muted_for(m)),
        ("surface_faint", surface_faint_for(m)),
        ("surface_accent", surface_accent_for(m)),
        ("muted", muted_for(m)),
        ("faint", faint_for(m)),
        ("bold", bold_for(m)),
        ("dim", dim_for(m)),
        ("title", title_for(m)),
        ("accent", accent_for(m)),
        ("selected", selected_for(m)),
        ("badge", badge_for(m)),
        ("harness_badge", harness_badge_for(m)),
        ("error", error_for(m)),
        ("border_focused", border_for(m, true)),
        ("border", border_for(m, false)),
        ("panel", panel_for(m)),
        ("header", header_for(m)),
        ("header_muted", header_muted_for(m)),
        ("footer", footer_for(m)),
        ("thinking", thinking_for(m)),
        ("tool", tool_for(m)),
        ("shell", shell_for(m)),
        ("diff_add", diff_add_for(m)),
        ("diff_del", diff_del_for(m)),
    ]
}

/// Every theme style helper for the current mode.
pub fn all_styles() -> Vec<(&'static str, Style)> {
    all_styles_for(mode())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_monochrome_theme_color_is_grayscale() {
        for c in [BG, FG, MUTED, FAINT, gray(0), gray(128), gray(255)] {
            assert!(is_grayscale(c), "{c:?}");
        }
        for (name, s) in all_styles_for(ThemeMode::Monochrome) {
            assert!(s.fg.is_some() && s.bg.is_some(), "{name} missing fg or bg");
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
    fn dark_color_theme_has_chromatic_palette() {
        for (name, s) in all_styles_for(ThemeMode::DarkColor) {
            assert!(s.fg.is_some() && s.bg.is_some(), "{name} missing fg or bg");
        }
        assert!(!is_grayscale(DARK_ACCENT));
        assert!(!is_grayscale(DARK_GREEN));
        assert!(!is_grayscale(DARK_RED));
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
