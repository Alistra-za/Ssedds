//! Windows system fonts, loaded at run time (nothing font-related is redistributed).
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn fonts_dir() -> PathBuf {
    std::env::var_os("WINDIR")
        .map(|w| PathBuf::from(w).join("Fonts"))
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows\Fonts"))
}

/// The font definitions built from `dir`: Segoe UI (proportional) and Consolas (monospace) with
/// Segoe UI Symbol, Segoe UI Emoji and the CJK system fonts as fallbacks. A missing optional font is skipped; a missing
/// `segoeui.ttf` is an error naming its path.
fn definitions_from(dir: &Path) -> Result<egui::FontDefinitions, String> {
    let primary = dir.join("segoeui.ttf");
    let primary_bytes = std::fs::read(&primary)
        .map_err(|e| format!("cannot load the system font {}: {e}", primary.display()))?;
    let mut defs = egui::FontDefinitions::empty();
    defs.font_data.insert(
        "segoeui".to_owned(),
        Arc::new(egui::FontData::from_owned(primary_bytes)),
    );
    let mut proportional = vec!["segoeui".to_owned()];
    let mut monospace = Vec::new();
    for (name, file, proportional_fallback, monospace_use) in [
        ("seguisym", "seguisym.ttf", true, true),
        ("seguiemj", "seguiemj.ttf", true, false),
        // #50: CJK fallbacks (Chinese, Japanese, Korean); each adds 13-20 MB of font data.
        ("msyh", "msyh.ttc", true, true),
        ("yugothm", "YuGothM.ttc", true, true),
        ("malgun", "malgun.ttf", true, true),
        ("consola", "consola.ttf", false, true),
    ] {
        let Ok(bytes) = std::fs::read(dir.join(file)) else {
            continue;
        };
        defs.font_data
            .insert(name.to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
        if proportional_fallback {
            proportional.push(name.to_owned());
        }
        if monospace_use {
            // Consolas leads the monospace family; symbols follow it.
            if name == "consola" {
                monospace.insert(0, name.to_owned());
            } else {
                monospace.push(name.to_owned());
            }
        }
    }
    if monospace.is_empty() || monospace[0] != "consola" {
        // No Consolas: fall back to the proportional face so monospace text still renders.
        monospace.insert(0, "segoeui".to_owned());
    }
    defs.families
        .insert(egui::FontFamily::Proportional, proportional);
    defs.families.insert(egui::FontFamily::Monospace, monospace);
    Ok(defs)
}

/// Installs Segoe UI, Segoe UI Symbol, Segoe UI Emoji and Consolas from `%WINDIR%\Fonts`.
pub fn install_system_fonts(ctx: &egui::Context) -> Result<(), String> {
    ctx.set_fonts(definitions_from(&fonts_dir())?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_fonts_install_and_fill_the_proportional_family() {
        let ctx = egui::Context::default();
        install_system_fonts(&ctx).expect("Windows system fonts");
        let defs = definitions_from(&fonts_dir()).unwrap();
        assert!(!defs.families[&egui::FontFamily::Proportional].is_empty());
        assert_eq!(defs.families[&egui::FontFamily::Proportional][0], "segoeui");
        assert!(!defs.families[&egui::FontFamily::Monospace].is_empty());
    }

    /// Trace-debug 2026-10-16 #50: CJK names get a proportional fallback (Chinese, Japanese, Korean system fonts).
    #[test]
    fn cjk_names_have_a_proportional_fallback() {
        let dir = fonts_dir();
        let defs = definitions_from(&dir).unwrap();
        let family = &defs.families[&egui::FontFamily::Proportional];
        for (name, file) in [
            ("msyh", "msyh.ttc"),
            ("yugothm", "YuGothM.ttc"),
            ("malgun", "malgun.ttf"),
        ] {
            if dir.join(file).exists() {
                assert!(family.iter().any(|f| f == name), "{name}: {family:?}");
            }
        }
        if dir.join("msyh.ttc").exists() {
            let mut fonts = egui::epaint::text::Fonts::new(Default::default(), defs);
            let id = egui::FontId::proportional(14.0);
            assert!(fonts.has_glyphs(&id, "中文 日本語 한국어"));
        }
    }

    #[test]
    fn a_missing_primary_font_names_its_path() {
        let d = std::env::temp_dir().join("sdc-no-such-fonts-dir");
        let err = definitions_from(&d).unwrap_err();
        assert!(err.contains("segoeui.ttf"), "{err}");
    }
}
