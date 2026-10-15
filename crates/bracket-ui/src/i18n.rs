//! User-visible texts. Every string shown in the interface lives here, in Spanish (the default
//! and reference language) and English.
//!
//! Phase 0 uses this minimal hand-written catalog; the translation approach for the real
//! interface is decided in Phase 1 by measuring its impact on executable size.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language {
    #[default]
    Spanish,
    English,
}

pub struct Texts {
    pub untitled: &'static str,
    pub loading: &'static str,
    pub read_only: &'static str,
    pub drop_hint: &'static str,
    pub position: fn(line: usize, column: usize) -> String,
    pub saved: &'static str,
    pub save_needs_path: &'static str,
    pub save_read_only: &'static str,
    pub open_failed: fn(error: &str) -> String,
    pub save_failed: fn(error: &str) -> String,
    pub unsaved_title: &'static str,
    pub unsaved_message: &'static str,
    pub save: &'static str,
    pub discard: &'static str,
    pub cancel: &'static str,
}

impl Language {
    /// Phase 0 stand-in for the language setting: `BRACKET_LANGUAGE=en` selects English,
    /// anything else keeps Spanish.
    pub fn from_env() -> Self {
        match std::env::var("BRACKET_LANGUAGE").as_deref() {
            Ok("en") => Self::English,
            _ => Self::Spanish,
        }
    }
}

pub fn texts(language: Language) -> &'static Texts {
    match language {
        Language::Spanish => &SPANISH,
        Language::English => &ENGLISH,
    }
}

static SPANISH: Texts = Texts {
    untitled: "Sin título",
    loading: "Cargando…",
    read_only: "Solo lectura (no es UTF-8)",
    drop_hint: "Arrastra un fichero a la ventana para abrirlo",
    position: |line, column| format!("Lín {line}, Col {column}"),
    saved: "Guardado",
    save_needs_path: "Este documento no tiene ruta; ábrelo desde un fichero para poder guardarlo",
    save_read_only: "El fichero está en solo lectura y no se puede guardar",
    open_failed: |error| format!("No se pudo abrir el fichero: {error}"),
    save_failed: |error| format!("No se pudo guardar el fichero: {error}"),
    unsaved_title: "Cambios sin guardar",
    unsaved_message: "El documento tiene cambios sin guardar. ¿Qué quieres hacer?",
    save: "Guardar",
    discard: "Descartar cambios",
    cancel: "Cancelar",
};

static ENGLISH: Texts = Texts {
    untitled: "Untitled",
    loading: "Loading…",
    read_only: "Read-only (not UTF-8)",
    drop_hint: "Drop a file on the window to open it",
    position: |line, column| format!("Ln {line}, Col {column}"),
    saved: "Saved",
    save_needs_path: "This document has no path; open it from a file to be able to save it",
    save_read_only: "The file is read-only and cannot be saved",
    open_failed: |error| format!("Could not open the file: {error}"),
    save_failed: |error| format!("Could not save the file: {error}"),
    unsaved_title: "Unsaved changes",
    unsaved_message: "The document has unsaved changes. What do you want to do?",
    save: "Save",
    discard: "Discard changes",
    cancel: "Cancel",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_catalogs_are_complete_and_differ() {
        let es = texts(Language::Spanish);
        let en = texts(Language::English);
        for (a, b) in [
            (es.untitled, en.untitled),
            (es.unsaved_title, en.unsaved_title),
            (es.discard, en.discard),
        ] {
            assert!(!a.is_empty() && !b.is_empty());
            assert_ne!(a, b);
        }
        assert_eq!((es.position)(3, 7), "Lín 3, Col 7");
        assert_eq!((en.position)(3, 7), "Ln 3, Col 7");
    }

    #[test]
    fn spanish_is_the_default() {
        assert_eq!(Language::default(), Language::Spanish);
    }
}
