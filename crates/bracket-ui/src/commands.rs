//! The command registry: every command with its id, its text, its keyboard shortcuts, the
//! menu it belongs to and when it is available. Menus, shortcuts and the command palette all
//! come from here.

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};

use crate::i18n::Texts;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    New,
    Open,
    OpenFolder,
    CloseFolder,
    Save,
    SaveAs,
    CloseTab,
    Exit,
    Palette,
    ToggleSidebar,
    QuickOpen,
    GoToLine,
    FocusTree,
    NextTab,
    PreviousTab,
}

/// The menu a command is listed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    File,
    View,
    Go,
}

/// What the availability of commands depends on.
#[derive(Debug, Clone, Copy, Default)]
pub struct State {
    pub has_project: bool,
    /// The active tab has finished loading.
    pub active_loaded: bool,
    pub tabs: usize,
}

const CTRL: Modifiers = Modifiers::COMMAND;
const CTRL_SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);

impl Command {
    /// In menu order.
    pub const ALL: [Self; 15] = [
        Self::New,
        Self::Open,
        Self::OpenFolder,
        Self::Save,
        Self::SaveAs,
        Self::CloseTab,
        Self::CloseFolder,
        Self::Exit,
        Self::Palette,
        Self::ToggleSidebar,
        Self::QuickOpen,
        Self::GoToLine,
        Self::FocusTree,
        Self::NextTab,
        Self::PreviousTab,
    ];

    /// Stable identifier, for settings and for the user to refer to a command.
    pub fn id(self) -> &'static str {
        match self {
            Self::New => "file.new",
            Self::Open => "file.open",
            Self::OpenFolder => "file.openFolder",
            Self::CloseFolder => "file.closeFolder",
            Self::Save => "file.save",
            Self::SaveAs => "file.saveAs",
            Self::CloseTab => "file.closeTab",
            Self::Exit => "file.exit",
            Self::Palette => "view.commandPalette",
            Self::ToggleSidebar => "view.toggleSidebar",
            Self::QuickOpen => "go.file",
            Self::GoToLine => "go.line",
            Self::FocusTree => "go.projectTree",
            Self::NextTab => "go.nextTab",
            Self::PreviousTab => "go.previousTab",
        }
    }

    pub fn menu(self) -> Menu {
        match self {
            Self::New
            | Self::Open
            | Self::OpenFolder
            | Self::CloseFolder
            | Self::Save
            | Self::SaveAs
            | Self::CloseTab
            | Self::Exit => Menu::File,
            Self::Palette | Self::ToggleSidebar => Menu::View,
            Self::QuickOpen
            | Self::GoToLine
            | Self::FocusTree
            | Self::NextTab
            | Self::PreviousTab => Menu::Go,
        }
    }

    pub fn label(self, texts: &'static Texts) -> &'static str {
        match self {
            Self::New => texts.menu_new(),
            Self::Open => texts.menu_open(),
            Self::OpenFolder => texts.menu_open_folder(),
            Self::CloseFolder => texts.menu_close_folder(),
            Self::Save => texts.menu_save(),
            Self::SaveAs => texts.menu_save_as(),
            Self::CloseTab => texts.menu_close_tab(),
            Self::Exit => texts.menu_exit(),
            Self::Palette => texts.menu_command_palette(),
            Self::ToggleSidebar => texts.menu_sidebar(),
            Self::QuickOpen => texts.menu_quick_open(),
            Self::GoToLine => texts.menu_go_to_line(),
            Self::FocusTree => texts.menu_focus_tree(),
            Self::NextTab => texts.menu_next_tab(),
            Self::PreviousTab => texts.menu_previous_tab(),
        }
    }

    /// "File: Save", as listed in the command palette.
    pub fn palette_label(self, texts: &'static Texts) -> String {
        let menu = match self.menu() {
            Menu::File => texts.menu_file(),
            Menu::View => texts.menu_view(),
            Menu::Go => texts.menu_go(),
        };
        // Menu entries that open a dialog or a box end with "…"; the palette does not need it.
        format!("{menu}: {}", self.label(texts).trim_end_matches('…'))
    }

    /// Its shortcuts; the first one is shown in menus and in the palette.
    pub fn shortcuts(self) -> &'static [KeyboardShortcut] {
        // A constant per command, so the slice lives for the whole program.
        macro_rules! keys {
            ($($modifiers:expr, $key:ident);+) => {{
                const KEYS: &[KeyboardShortcut] = &[$(KeyboardShortcut::new($modifiers, Key::$key)),+];
                KEYS
            }};
        }
        match self {
            Self::New => keys!(CTRL, N),
            Self::Open => keys!(CTRL, O),
            Self::Save => keys!(CTRL, S),
            Self::SaveAs => keys!(CTRL_SHIFT, S),
            Self::CloseTab => keys!(CTRL, W),
            Self::Palette => keys!(CTRL_SHIFT, P; Modifiers::NONE, F1),
            Self::ToggleSidebar => keys!(CTRL, B),
            Self::QuickOpen => keys!(CTRL, P),
            Self::GoToLine => keys!(CTRL, G),
            Self::FocusTree => keys!(CTRL_SHIFT, E),
            Self::NextTab => keys!(CTRL, Tab; CTRL, PageDown),
            Self::PreviousTab => keys!(CTRL_SHIFT, Tab; CTRL, PageUp),
            Self::OpenFolder | Self::CloseFolder | Self::Exit => &[],
        }
    }

    pub fn shortcut(self) -> Option<KeyboardShortcut> {
        self.shortcuts().first().copied()
    }

    pub fn is_available(self, state: &State) -> bool {
        match self {
            Self::CloseFolder | Self::ToggleSidebar | Self::FocusTree => state.has_project,
            Self::Save | Self::SaveAs | Self::GoToLine => state.active_loaded,
            Self::NextTab | Self::PreviousTab => state.tabs > 1,
            _ => true,
        }
    }

    /// The commands of a menu, in order.
    pub fn in_menu(menu: Menu) -> impl Iterator<Item = Self> {
        Self::ALL.into_iter().filter(move |c| c.menu() == menu)
    }
}

/// The command whose shortcut was pressed, consuming the key so the editor does not see it.
/// Shortcuts with more modifiers are tried first: Ctrl+S would also match Ctrl+Shift+S.
pub fn pressed(ctx: &egui::Context) -> Option<Command> {
    let mut bindings: Vec<(Command, KeyboardShortcut)> = Command::ALL
        .into_iter()
        .flat_map(|c| c.shortcuts().iter().map(move |s| (c, *s)))
        .collect();
    bindings.sort_by_key(|(_, s)| std::cmp::Reverse(modifier_count(s.modifiers)));
    bindings
        .into_iter()
        .find(|(_, shortcut)| ctx.input_mut(|i| i.consume_shortcut(shortcut)))
        .map(|(command, _)| command)
}

fn modifier_count(modifiers: Modifiers) -> usize {
    [
        modifiers.alt,
        modifiers.shift,
        modifiers.ctrl || modifiers.command,
    ]
    .into_iter()
    .filter(|m| *m)
    .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_and_shortcuts_are_unique() {
        let ids: HashSet<&str> = Command::ALL.iter().map(|c| c.id()).collect();
        assert_eq!(ids.len(), Command::ALL.len());
        let mut seen = HashSet::new();
        for command in Command::ALL {
            for shortcut in command.shortcuts() {
                assert!(
                    seen.insert((shortcut.modifiers, shortcut.logical_key)),
                    "{} shares a shortcut",
                    command.id()
                );
            }
        }
    }

    #[test]
    fn every_command_is_listed_once_and_has_a_text() {
        let all: HashSet<Command> = Command::ALL.into_iter().collect();
        assert_eq!(all.len(), Command::ALL.len());
        for language in [
            bracket_session::Language::Spanish,
            bracket_session::Language::English,
        ] {
            let texts = crate::i18n::texts(language);
            for command in Command::ALL {
                assert!(!command.label(texts).is_empty());
                assert!(command.palette_label(texts).contains(": "));
            }
        }
    }

    #[test]
    fn availability_follows_the_state() {
        let none = State::default();
        let full = State {
            has_project: true,
            active_loaded: true,
            tabs: 2,
        };
        for command in [
            Command::CloseFolder,
            Command::FocusTree,
            Command::Save,
            Command::GoToLine,
            Command::NextTab,
        ] {
            assert!(!command.is_available(&none), "{}", command.id());
            assert!(command.is_available(&full), "{}", command.id());
        }
        assert!(Command::New.is_available(&none));
        assert!(Command::Palette.is_available(&none));
    }

    #[test]
    fn more_specific_shortcuts_go_first() {
        assert!(modifier_count(CTRL_SHIFT) > modifier_count(CTRL));
        assert_eq!(modifier_count(Modifiers::NONE), 0);
    }
}
