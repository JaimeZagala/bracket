//! Fonts: Bracket ships no fonts and uses the ones installed on the system.
//!
//! Finding a font by family name is platform work, so `bracket-app` provides a [`FontSource`].
//! This module decides which families to use:
//!
//! - **Monospace (editor):** the configured family, else Consolas, Cascadia Mono, Courier New.
//! - **Proportional (interface):** Segoe UI.
//! - **Fallbacks, loaded on demand:** when visible text contains a character the loaded fonts
//!   cannot draw, a symbol font or a CJK font is loaded on a worker thread and added at the end
//!   of both families. CJK fonts are 9 to 20 MB, so they are only loaded when needed.

use std::sync::Arc;

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use eframe::epaint::text::{FontInsert, FontPriority, InsertFontFamily};

/// A font file read from disk. `index` selects the face inside a `.ttc` collection.
#[derive(Clone)]
pub struct FontFile {
    pub family: String,
    pub bytes: Vec<u8>,
    pub index: u32,
}

impl std::fmt::Debug for FontFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontFile")
            .field("family", &self.family)
            .field("bytes", &self.bytes.len())
            .field("index", &self.index)
            .finish()
    }
}

/// Finds and reads installed fonts by family name.
pub trait FontSource: Send + Sync {
    fn load(&self, family: &str) -> Option<FontFile>;
}

pub const DEFAULT_MONOSPACE: &str = "Consolas";
const MONOSPACE_FALLBACKS: &[&str] = &["Consolas", "Cascadia Mono", "Courier New"];
const PROPORTIONAL_FAMILIES: &[&str] = &["Segoe UI"];
const SYMBOL_FALLBACKS: &[&str] = &["Segoe UI Symbol"];
const CJK_FALLBACKS: &[&str] = &["Microsoft YaHei", "Yu Gothic", "MS Gothic"];
const HANGUL_FALLBACKS: &[&str] = &["Malgun Gothic"];

/// What `bracket-app` hands over: the font source (kept for fallbacks) and the startup fonts.
pub struct FontSetup {
    pub source: Arc<dyn FontSource>,
    pub startup: StartupFonts,
}

/// The fonts needed for the first frame.
#[derive(Debug)]
pub struct StartupFonts {
    pub monospace: Option<FontFile>,
    pub proportional: Option<FontFile>,
    /// The configured family, if it is not installed and a fallback is used instead.
    pub missing_family: Option<String>,
}

/// Loads the editor and interface fonts. Meant to run on a worker thread during startup.
pub fn load_startup_fonts(source: &dyn FontSource, configured: &str) -> StartupFonts {
    let requested = source.load(configured);
    let missing_family = requested.is_none().then(|| configured.to_owned());
    let monospace = requested.or_else(|| first_available(source, MONOSPACE_FALLBACKS));
    let proportional = first_available(source, PROPORTIONAL_FAMILIES);
    StartupFonts {
        monospace,
        proportional,
        missing_family,
    }
}

fn first_available(source: &dyn FontSource, families: &[&str]) -> Option<FontFile> {
    families.iter().find_map(|family| source.load(family))
}

/// egui font definitions with the startup fonts. Each family falls back to the other font, so
/// both families work even if only one font was found. Returns `None` if no font was found.
pub fn definitions(fonts: StartupFonts) -> Option<FontDefinitions> {
    let mut defs = FontDefinitions::empty();
    let mut add = |file: FontFile| {
        let name = file.family.clone();
        defs.font_data
            .insert(name.clone(), Arc::new(font_data(file)));
        name
    };
    let mono = fonts.monospace.map(&mut add);
    let prop = fonts.proportional.map(&mut add);
    if mono.is_none() && prop.is_none() {
        return None;
    }
    let order = |first: &Option<String>, second: &Option<String>| {
        first
            .iter()
            .chain(second.iter())
            .cloned()
            .collect::<Vec<_>>()
    };
    defs.families
        .insert(FontFamily::Monospace, order(&mono, &prop));
    defs.families
        .insert(FontFamily::Proportional, order(&prop, &mono));
    Some(defs)
}

/// Moves the bytes into egui (CJK fonts are tens of megabytes; no copies).
fn font_data(file: FontFile) -> FontData {
    let mut data = FontData::from_owned(file.bytes);
    data.index = file.index;
    data
}

/// Which fallback can draw a character the loaded fonts cannot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fallback {
    Symbols,
    /// Han ideographs and Japanese kana.
    Cjk,
    /// Korean: the Chinese and Japanese fonts do not include it.
    Hangul,
}

impl Fallback {
    pub fn for_char(c: char) -> Self {
        if is_hangul(c) {
            Self::Hangul
        } else if is_cjk(c) {
            Self::Cjk
        } else {
            Self::Symbols
        }
    }

    fn families(self) -> &'static [&'static str] {
        match self {
            Self::Symbols => SYMBOL_FALLBACKS,
            Self::Cjk => CJK_FALLBACKS,
            Self::Hangul => HANGUL_FALLBACKS,
        }
    }

    /// A character typical of the fallback, used to measure its baseline.
    fn sample(self) -> char {
        match self {
            Self::Symbols => '√',
            Self::Cjk => '日',
            Self::Hangul => '한',
        }
    }
}

/// A fallback font ready to be added to egui, with its baseline already aligned.
pub struct LoadedFallback {
    name: String,
    data: FontData,
}

impl LoadedFallback {
    pub fn family(&self) -> &str {
        &self.name
    }
}

/// Loads the first installed font of a fallback group and aligns its baseline with the editor
/// font `primary` at `font_size`. Meant to run on a worker thread.
pub fn load_fallback(
    source: &dyn FontSource,
    fallback: Fallback,
    primary: &str,
    font_size: f32,
) -> Option<LoadedFallback> {
    let file = first_available(source, fallback.families())?;
    let name = file.family.clone();
    let mut data = font_data(file);
    if let Some(primary) = source.load(primary) {
        let (measured, shift) = baseline_shift(primary, data, fallback.sample(), font_size);
        data = measured;
        data.tweak.y_offset_factor = -shift / font_size;
    }
    Some(LoadedFallback { name, data })
}

/// How far below the editor font's baseline egui puts the baseline of `sample` drawn with
/// `fallback`, in points. egui places each glyph by its own font's ascent, so fonts that split
/// their height differently (CJK fonts, for example) sit lower than Latin text.
///
/// Measured by laying out "H" + `sample` in a throwaway egui context. The fallback's bytes are
/// shared with that context and handed back afterwards, so nothing is copied.
fn baseline_shift(
    primary: FontFile,
    fallback: FontData,
    sample: char,
    font_size: f32,
) -> (FontData, f32) {
    let fallback = Arc::new(fallback);
    let mut defs = FontDefinitions::empty();
    defs.font_data
        .insert("primary".to_owned(), Arc::new(font_data(primary)));
    defs.font_data
        .insert("fallback".to_owned(), Arc::clone(&fallback));
    defs.families.insert(
        FontFamily::Monospace,
        vec!["primary".to_owned(), "fallback".to_owned()],
    );

    let ctx = egui::Context::default();
    ctx.set_fonts(defs);
    let mut shift = 0.0;
    // Fonts set before a pass take effect in that pass; two passes make sure of it.
    for _ in 0..2 {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let galley = ui.fonts_mut(|f| {
                f.layout_no_wrap(
                    format!("H{sample}"),
                    egui::FontId::monospace(font_size),
                    egui::Color32::WHITE,
                )
            });
            if let [latin, other] = galley.rows[0].glyphs.as_slice() {
                shift = other.pos.y - latin.pos.y;
            }
        });
        output.textures_delta.clear();
    }
    drop(ctx);
    let data = Arc::try_unwrap(fallback).unwrap_or_else(|shared| (*shared).clone());
    (data, shift)
}

/// How to add a loaded fallback to egui: last in both families.
pub fn fallback_insert(fallback: LoadedFallback) -> FontInsert {
    let lowest = |family| InsertFontFamily {
        family,
        priority: FontPriority::Lowest,
    };
    FontInsert::new(
        &fallback.name,
        fallback.data,
        vec![
            lowest(FontFamily::Monospace),
            lowest(FontFamily::Proportional),
        ],
    )
}

/// Hangul syllables and jamo.
fn is_hangul(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x11FF     // Hangul Jamo
        | 0x3130..=0x318F   // Hangul compatibility jamo
        | 0xA960..=0xA97F   // Hangul Jamo extended-A
        | 0xAC00..=0xD7FF   // Hangul syllables and Jamo extended-B
    )
}

/// Han ideographs, kana and their punctuation and full-width forms.
fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x2FDF   // CJK radicals
        | 0x3000..=0x30FF   // CJK punctuation, hiragana, katakana
        | 0x3100..=0x31FF   // Bopomofo, katakana extensions (Hangul is checked first)
        | 0x3200..=0x4DBF   // enclosed CJK, compatibility, extension A
        | 0x4E00..=0x9FFF   // CJK unified ideographs
        | 0xF900..=0xFAFF   // CJK compatibility ideographs
        | 0xFF00..=0xFFEF   // half-width and full-width forms
        | 0x20000..=0x3FFFF // extensions B and later
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct FakeSource(HashMap<&'static str, u32>);

    impl FontSource for FakeSource {
        fn load(&self, family: &str) -> Option<FontFile> {
            self.0.get(family).map(|&index| FontFile {
                family: family.to_owned(),
                bytes: vec![0; 4],
                index,
            })
        }
    }

    fn source(families: &[&'static str]) -> FakeSource {
        FakeSource(families.iter().map(|f| (*f, 0)).collect())
    }

    #[test]
    fn configured_family_is_used_when_installed() {
        let fonts =
            load_startup_fonts(&source(&["Fira Code", "Consolas", "Segoe UI"]), "Fira Code");
        assert_eq!(fonts.monospace.unwrap().family, "Fira Code");
        assert_eq!(fonts.proportional.unwrap().family, "Segoe UI");
        assert_eq!(fonts.missing_family, None);
    }

    #[test]
    fn missing_family_falls_back_and_is_reported() {
        let fonts = load_startup_fonts(&source(&["Cascadia Mono", "Segoe UI"]), "Nope Mono");
        assert_eq!(fonts.monospace.unwrap().family, "Cascadia Mono");
        assert_eq!(fonts.missing_family.as_deref(), Some("Nope Mono"));
    }

    #[test]
    fn families_fall_back_to_each_other() {
        let fonts = load_startup_fonts(&source(&["Consolas"]), "Consolas");
        let defs = definitions(fonts).unwrap();
        assert_eq!(defs.families[&FontFamily::Monospace], vec!["Consolas"]);
        assert_eq!(defs.families[&FontFamily::Proportional], vec!["Consolas"]);
    }

    #[test]
    fn no_font_at_all_gives_none() {
        let fonts = load_startup_fonts(&source(&[]), "Consolas");
        assert!(definitions(fonts).is_none());
    }

    #[test]
    fn fallback_kind_depends_on_the_character() {
        assert_eq!(Fallback::for_char('日'), Fallback::Cjk);
        assert_eq!(Fallback::for_char('한'), Fallback::Hangul);
        assert_eq!(Fallback::for_char('カ'), Fallback::Cjk);
        assert_eq!(Fallback::for_char('Ａ'), Fallback::Cjk);
        assert_eq!(Fallback::for_char('√'), Fallback::Symbols);
        assert_eq!(Fallback::for_char('♞'), Fallback::Symbols);
    }

    #[test]
    fn cjk_fallback_uses_the_first_installed_font() {
        let src = source(&["Malgun Gothic", "MS Gothic"]);
        assert_eq!(
            load_fallback(&src, Fallback::Cjk, "Consolas", 14.0)
                .unwrap()
                .family()
                .to_owned(),
            "MS Gothic",
            "Malgun Gothic has no kana or full Han coverage"
        );
        assert_eq!(
            load_fallback(&src, Fallback::Hangul, "Consolas", 14.0)
                .unwrap()
                .family()
                .to_owned(),
            "Malgun Gothic"
        );
        assert!(load_fallback(&source(&[]), Fallback::Symbols, "Consolas", 14.0).is_none());
    }

    /// Reads fonts straight from `%WINDIR%\Fonts` by file name, for tests with real fonts.
    struct WindowsFonts(&'static [(&'static str, &'static str)]);

    impl FontSource for WindowsFonts {
        fn load(&self, family: &str) -> Option<FontFile> {
            let (_, file) = self.0.iter().find(|(name, _)| *name == family)?;
            let windir = std::env::var("WINDIR").ok()?;
            let bytes = std::fs::read(format!(r"{windir}\Fonts\{file}")).ok()?;
            Some(FontFile {
                family: family.to_owned(),
                bytes,
                index: 0,
            })
        }
    }

    /// Regression: CJK glyphs sat about 2 px below Latin text before the baseline correction.
    #[test]
    fn cjk_fallback_is_aligned_with_the_latin_baseline() {
        let source = WindowsFonts(&[
            ("Consolas", "consola.ttf"),
            ("Microsoft YaHei", "msyh.ttc"),
            ("MS Gothic", "msgothic.ttc"),
        ]);
        let Some(fallback) = load_fallback(&source, Fallback::Cjk, "Consolas", 14.0) else {
            return; // No CJK font on this machine.
        };
        let LoadedFallback { data, .. } = fallback;
        assert!(data.tweak.y_offset_factor < 0.0, "the CJK font is moved up");
        let primary = source.load("Consolas").unwrap();
        let (_, shift_after) = baseline_shift_with_tweak(primary, data, '日', 14.0);
        assert!(shift_after.abs() <= 1.0, "still {shift_after} px off");
    }

    /// Like `baseline_shift`, but measures the glyph's visible top instead of the logical
    /// baseline, so the font's y_offset tweak is taken into account.
    fn baseline_shift_with_tweak(
        primary: FontFile,
        fallback: FontData,
        sample: char,
        font_size: f32,
    ) -> (FontData, f32) {
        let tweak = fallback.tweak.clone();
        let (data, logical_shift) = baseline_shift(primary, fallback, sample, font_size);
        let visual_shift = logical_shift + tweak.y_offset_factor * font_size + tweak.y_offset;
        (data, visual_shift)
    }
}
