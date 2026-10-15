//! Installed fonts, found through the Windows registry.
//!
//! Windows lists fonts under `...\CurrentVersion\Fonts`: per-user installs in HKCU (with full
//! paths) and system fonts in HKLM (file names relative to `%WINDIR%\Fonts`). Value names look
//! like `Consolas (TrueType)`, `Cascadia Mono Regular (TrueType)` or, for collections,
//! `MS Gothic & MS UI Gothic & MS PGothic (TrueType)`, where each name is a face index.

use std::path::{Path, PathBuf};

use bracket_ui::fonts::{FontFile, FontSource};
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use winreg::types::FromRegValue;

const FONTS_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts";

/// One registry entry: the face names it contains and the file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FontEntry {
    names: Vec<String>,
    path: PathBuf,
}

/// A snapshot of the installed fonts (read once; reading the registry takes well under 1 ms).
pub struct SystemFonts {
    /// Per-user fonts first, so they win over system fonts with the same name.
    entries: Vec<FontEntry>,
}

impl SystemFonts {
    pub fn read() -> Self {
        let windir =
            std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        let system_dir = windir.join("Fonts");
        let mut entries = Vec::new();
        for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            let Ok(key) = RegKey::predef(root).open_subkey(FONTS_KEY) else {
                continue;
            };
            for (name, value) in key.enum_values().flatten() {
                if let Ok(file) = String::from_reg_value(&value)
                    && let Some(entry) = parse_entry(&name, &file, &system_dir)
                {
                    entries.push(entry);
                }
            }
        }
        Self { entries }
    }

    /// Path and face index of the best match for `family`.
    fn find(&self, family: &str) -> Option<(PathBuf, u32)> {
        find_in(&self.entries, family)
    }
}

impl FontSource for SystemFonts {
    fn load(&self, family: &str) -> Option<FontFile> {
        let (path, index) = self.find(family)?;
        let bytes = std::fs::read(path).ok()?;
        Some(FontFile {
            family: family.to_owned(),
            bytes,
            index,
        })
    }
}

/// Turns a registry value into an entry. Only TrueType/OpenType outline fonts are kept.
fn parse_entry(value_name: &str, file: &str, system_dir: &Path) -> Option<FontEntry> {
    let names = value_name
        .strip_suffix(" (TrueType)")
        .or_else(|| value_name.strip_suffix(" (OpenType)"))?;
    let path = Path::new(file);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        system_dir.join(path)
    };
    Some(FontEntry {
        names: names.split(" & ").map(str::to_owned).collect(),
        path,
    })
}

/// Picks the entry whose face name best matches `family` (case-insensitive):
/// exact name, then `<family> Regular`, then `<family> <style>` for variable fonts registered
/// under their default style, skipping bold and italic faces.
fn find_in(entries: &[FontEntry], family: &str) -> Option<(PathBuf, u32)> {
    let family = family.trim().to_lowercase();
    if family.is_empty() {
        return None;
    }
    let mut best: Option<(u8, PathBuf, u32)> = None;
    for entry in entries {
        for (index, name) in entry.names.iter().enumerate() {
            let Some(score) = match_score(&name.to_lowercase(), &family) else {
                continue;
            };
            if best
                .as_ref()
                .is_none_or(|(best_score, _, _)| score > *best_score)
            {
                best = Some((score, entry.path.clone(), index as u32));
            }
        }
    }
    best.map(|(_, path, index)| (path, index))
}

fn match_score(name: &str, family: &str) -> Option<u8> {
    if name == family {
        return Some(3);
    }
    let style = name.strip_prefix(family)?.strip_prefix(' ')?;
    if style == "regular" {
        Some(2)
    } else if !style.contains("bold") && !style.contains("italic") {
        Some(1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries() -> Vec<FontEntry> {
        let dir = Path::new(r"C:\Windows\Fonts");
        [
            (
                r"Source Code Pro ExtraLight Regular (TrueType)",
                r"C:\Users\u\Fonts\SourceCodePro.ttf",
            ),
            ("Consolas (TrueType)", "consola.ttf"),
            ("Consolas Bold (TrueType)", "consolab.ttf"),
            ("Cascadia Mono Regular (TrueType)", "CascadiaMono.ttf"),
            (
                "MS Gothic & MS UI Gothic & MS PGothic (TrueType)",
                "msgothic.ttc",
            ),
            (
                "Yu Gothic Regular & Yu Gothic UI Semilight (TrueType)",
                "YuGothR.ttc",
            ),
            (
                "Yu Gothic Bold & Yu Gothic UI Bold (TrueType)",
                "YuGothB.ttc",
            ),
            ("Modern (All res)", "modern.fon"),
        ]
        .iter()
        .filter_map(|(name, file)| parse_entry(name, file, dir))
        .collect()
    }

    #[test]
    fn bitmap_fonts_are_ignored() {
        assert_eq!(entries().len(), 7);
    }

    #[test]
    fn exact_name_wins_over_styles() {
        assert_eq!(
            find_in(&entries(), "Consolas"),
            Some((PathBuf::from(r"C:\Windows\Fonts\consola.ttf"), 0))
        );
    }

    #[test]
    fn regular_suffix_and_case_are_accepted() {
        assert_eq!(
            find_in(&entries(), "cascadia mono"),
            Some((PathBuf::from(r"C:\Windows\Fonts\CascadiaMono.ttf"), 0))
        );
    }

    #[test]
    fn collections_give_the_face_index() {
        assert_eq!(
            find_in(&entries(), "MS PGothic"),
            Some((PathBuf::from(r"C:\Windows\Fonts\msgothic.ttc"), 2))
        );
        assert_eq!(
            find_in(&entries(), "Yu Gothic"),
            Some((PathBuf::from(r"C:\Windows\Fonts\YuGothR.ttc"), 0))
        );
    }

    #[test]
    fn variable_fonts_registered_with_a_style_are_found() {
        assert_eq!(
            find_in(&entries(), "Source Code Pro"),
            Some((PathBuf::from(r"C:\Users\u\Fonts\SourceCodePro.ttf"), 0))
        );
    }

    #[test]
    fn unknown_or_empty_family_is_not_found() {
        assert_eq!(find_in(&entries(), "Comic Mono"), None);
        assert_eq!(find_in(&entries(), "  "), None);
        assert_eq!(find_in(&entries(), "Consola"), None, "no prefix matches");
    }

    #[test]
    fn installed_system_fonts_are_found() {
        // Consolas and Segoe UI ship with every Windows 10/11.
        let fonts = SystemFonts::read();
        for family in ["Consolas", "Segoe UI"] {
            let file = fonts
                .load(family)
                .unwrap_or_else(|| panic!("{family} not found"));
            assert!(file.bytes.len() > 10_000);
        }
    }
}
