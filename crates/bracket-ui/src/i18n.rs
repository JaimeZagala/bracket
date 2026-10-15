//! User-visible texts. Every string shown in the interface lives here, in Spanish (the default
//! and reference language) and English.
//!
//! Phase 0 uses this minimal hand-written catalog; the translation approach for the real
//! interface is decided in Phase 1 by measuring its impact on executable size.

use bracket_session::ConfigWarning;
pub use bracket_session::Language;

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
    pub config_unreadable: fn(error: &str) -> String,
    pub config_syntax: fn(error: &str) -> String,
    pub config_invalid_value: fn(key: &str, found: &str) -> String,
    pub config_unknown_key: fn(key: &str) -> String,
    pub more_warnings: fn(count: usize) -> String,
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
    config_unreadable: |error| format!("No se pudo leer la configuración: {error}"),
    config_syntax: |error| {
        format!(
            "La configuración tiene un error de sintaxis y se usan los valores por defecto: {error}"
        )
    },
    config_invalid_value: |key, found| {
        format!("Valor no válido en la configuración: {key} = {found}; se usa el valor por defecto")
    },
    config_unknown_key: |key| format!("Clave desconocida en la configuración: {key}"),
    more_warnings: |count| match count {
        1 => "(y 1 aviso más)".to_owned(),
        n => format!("(y {n} avisos más)"),
    },
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
    config_unreadable: |error| format!("Could not read the configuration: {error}"),
    config_syntax: |error| {
        format!("The configuration has a syntax error and defaults are used: {error}")
    },
    config_invalid_value: |key, found| {
        format!("Invalid value in the configuration: {key} = {found}; the default is used")
    },
    config_unknown_key: |key| format!("Unknown key in the configuration: {key}"),
    more_warnings: |count| match count {
        1 => "(1 more warning)".to_owned(),
        n => format!("({n} more warnings)"),
    },
};

impl Texts {
    /// Translated text for a configuration warning.
    pub fn config_warning(&self, warning: &ConfigWarning) -> String {
        match warning {
            ConfigWarning::Unreadable { error } => (self.config_unreadable)(error),
            ConfigWarning::Syntax { error } => (self.config_syntax)(error),
            ConfigWarning::InvalidValue { key, found } => (self.config_invalid_value)(key, found),
            ConfigWarning::UnknownKey { key } => (self.config_unknown_key)(key),
        }
    }
}

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
    fn config_warnings_are_translated() {
        let warning = ConfigWarning::InvalidValue {
            key: "editor.tab_width".into(),
            found: "0".into(),
        };
        assert!(
            texts(Language::Spanish)
                .config_warning(&warning)
                .starts_with("Valor no válido")
        );
        assert!(
            texts(Language::English)
                .config_warning(&warning)
                .starts_with("Invalid value")
        );
    }

    #[test]
    fn more_warnings_uses_singular_and_plural() {
        assert_eq!(
            (texts(Language::Spanish).more_warnings)(1),
            "(y 1 aviso más)"
        );
        assert_eq!(
            (texts(Language::Spanish).more_warnings)(3),
            "(y 3 avisos más)"
        );
        assert_eq!(
            (texts(Language::English).more_warnings)(1),
            "(1 more warning)"
        );
        assert_eq!(
            (texts(Language::English).more_warnings)(3),
            "(3 more warnings)"
        );
    }

    #[test]
    fn spanish_is_the_default() {
        assert_eq!(Language::default(), Language::Spanish);
    }
}
