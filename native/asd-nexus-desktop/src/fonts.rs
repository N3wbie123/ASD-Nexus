//! Windows system Korean font loader.
//!
//! Loads `C:\Windows\Fonts\malgun.ttf` (Malgun Gothic), falling back to
//! `gulim.ttc`, into `egui::FontDefinitions` as a fallback face for both the
//! proportional and monospace families. Missing fonts (non-Windows, CI,
//! headless tests) are skipped cleanly and the egui defaults are kept.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

/// Font key registered in `FontDefinitions::font_data`.
pub const KOREAN_FONT_KEY: &str = "asd-korean";

/// Default candidate list, in priority order.
pub fn default_candidates() -> Vec<PathBuf> {
    let win = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let fonts = win.join("Fonts");
    vec![fonts.join("malgun.ttf"), fonts.join("gulim.ttc")]
}

/// Read the first candidate that exists and is non-empty.
pub fn read_first_font(candidates: &[PathBuf]) -> Option<(PathBuf, Vec<u8>)> {
    candidates.iter().find_map(|p| match std::fs::read(p) {
        Ok(bytes) if !bytes.is_empty() => Some((p.clone(), bytes)),
        _ => None,
    })
}

/// Register raw font bytes as a fallback face for all families.
pub fn install_font_bytes(defs: &mut FontDefinitions, bytes: Vec<u8>) {
    defs.font_data
        .insert(KOREAN_FONT_KEY.to_owned(), Arc::new(FontData::from_owned(bytes)));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        let list = defs.families.entry(family).or_default();
        list.retain(|k| k != KOREAN_FONT_KEY);
        list.push(KOREAN_FONT_KEY.to_owned());
    }
}

/// Try candidates in order; returns the path that was loaded, if any.
pub fn install_korean_font(defs: &mut FontDefinitions, candidates: &[PathBuf]) -> Option<PathBuf> {
    let (path, bytes) = read_first_font(candidates)?;
    install_font_bytes(defs, bytes);
    Some(path)
}

/// Build font definitions (egui defaults + Korean fallback when available).
pub fn build_font_definitions(candidates: &[PathBuf]) -> (FontDefinitions, Option<PathBuf>) {
    let mut defs = FontDefinitions::default();
    let loaded = install_korean_font(&mut defs, candidates);
    (defs, loaded)
}

/// Configure the context's fonts with the system Korean font, if present.
pub fn configure_fonts(ctx: &egui::Context) -> Option<PathBuf> {
    let (defs, loaded) = build_font_definitions(&default_candidates());
    ctx.set_fonts(defs);
    loaded
}

/// Human-readable status for the status bar.
pub fn describe(loaded: Option<&Path>) -> String {
    match loaded {
        Some(p) => format!("font: {}", p.file_name().and_then(|s| s.to_str()).unwrap_or("?")),
        None => "font: egui default (no Korean system font)".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("asd-nexus-desktop-font-{}-{name}", std::process::id()))
    }

    #[test]
    fn missing_fonts_are_skipped() {
        let cands = vec![tmp("nope1.ttf"), tmp("nope2.ttc")];
        let (defs, loaded) = build_font_definitions(&cands);
        assert!(loaded.is_none());
        assert!(!defs.font_data.contains_key(KOREAN_FONT_KEY));
        assert_eq!(defs.families, FontDefinitions::default().families);
    }

    #[test]
    fn fallback_order_and_empty_skip() {
        let empty = tmp("empty.ttf");
        let present = tmp("present.ttc");
        std::fs::write(&empty, b"").unwrap();
        std::fs::write(&present, b"fake-font-bytes").unwrap();
        let cands = vec![tmp("missing.ttf"), empty.clone(), present.clone()];
        let mut defs = FontDefinitions::default();
        let loaded = install_korean_font(&mut defs, &cands);
        assert_eq!(loaded.as_deref(), Some(present.as_path()));
        assert!(defs.font_data.contains_key(KOREAN_FONT_KEY));
        for fam in [FontFamily::Proportional, FontFamily::Monospace] {
            let list = &defs.families[&fam];
            assert_eq!(list.last().map(String::as_str), Some(KOREAN_FONT_KEY));
            assert_eq!(list.iter().filter(|k| *k == KOREAN_FONT_KEY).count(), 1);
        }
        // Idempotent re-install does not duplicate the family entry.
        install_font_bytes(&mut defs, b"x".to_vec());
        assert_eq!(defs.families[&FontFamily::Proportional].iter().filter(|k| *k == KOREAN_FONT_KEY).count(), 1);
        let _ = std::fs::remove_file(empty);
        let _ = std::fs::remove_file(present);
    }

    #[test]
    fn real_system_font_parses_when_present() {
        let (defs, loaded) = build_font_definitions(&default_candidates());
        if loaded.is_none() {
            return; // non-Windows / font not installed: cleanly skipped
        }
        let ctx = egui::Context::default();
        ctx.set_fonts(defs);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("한글 폰트 테스트 — 안녕하세요");
            });
        });
    }
}
