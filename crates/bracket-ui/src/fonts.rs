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

use eframe::egui::{FontData, FontDefinitions, FontFamily};
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
}

/// Loads the first installed font of a fallback group. Meant to run on a worker thread.
pub fn load_fallback(source: &dyn FontSource, fallback: Fallback) -> Option<FontFile> {
    first_available(source, fallback.families())
}

/// How to add a loaded fallback to egui: last in both families.
pub fn fallback_insert(file: FontFile) -> FontInsert {
    let lowest = |family| InsertFontFamily {
        family,
        priority: FontPriority::Lowest,
    };
    let name = file.family.clone();
    FontInsert::new(
        &name,
        font_data(file),
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
            load_fallback(&src, Fallback::Cjk).unwrap().family,
            "MS Gothic",
            "Malgun Gothic has no kana or full Han coverage"
        );
        assert_eq!(
            load_fallback(&src, Fallback::Hangul).unwrap().family,
            "Malgun Gothic"
        );
        assert!(load_fallback(&source(&[]), Fallback::Symbols).is_none());
    }
}
