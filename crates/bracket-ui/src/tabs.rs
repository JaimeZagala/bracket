//! Open documents as tabs: the tab model and the tab bar, with a language badge on each tab.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use bracket_core::Document;
use bracket_core::document::{DiskStamp, SaveError};
use bracket_core::encoding::TextEncoding;
use bracket_session::SessionTab;
use bracket_syntax::{Badge, LanguageRegistry};
use eframe::egui::{
    self, Color32, FontId, Rect, Sense, Stroke, StrokeKind, TextStyle, Ui, WidgetInfo, WidgetType,
    pos2, vec2,
};

use crate::disk::DiskWatch;
use crate::editor_view::EditorView;
use crate::i18n::Texts;

/// Result of a background save: the version and encoding written, and whether it worked (with
/// the file's stamp afterwards).
pub type SaveOutcome = (u64, TextEncoding, Result<Option<DiskStamp>, SaveError>);

/// A document read on a worker thread, with the stamp of the file it came from.
pub struct Loaded {
    pub doc: Document,
    pub stamp: Option<DiskStamp>,
}

/// A file being read on a worker thread.
pub struct Loading {
    /// `None` for an untitled document restored from the session.
    pub path: Option<PathBuf>,
    /// Encoding forced with "reopen with encoding".
    pub read_encoding: Option<TextEncoding>,
    /// The session entry being restored. Until it finishes, it is saved again as it was.
    pub restore: Option<SessionTab>,
    pub rx: Receiver<io::Result<Loaded>>,
    /// The tab was opened for this file, so it goes away if the file cannot be read.
    pub new_tab: bool,
    /// Line and column (1-based) to put the caret on once loaded (`file:line:column`).
    pub goto: Option<(usize, usize)>,
    pub started: Instant,
}

/// Loads shorter than this show an empty editor rather than flashing a loading message.
pub const LOADING_MESSAGE_DELAY: Duration = Duration::from_millis(200);

pub struct Tab {
    /// Unique for the whole run; egui ids of the tab's widgets derive from it.
    pub id: u64,
    pub doc: Document,
    pub view: EditorView,
    /// Language id in the registry, from the file extension.
    pub language: Option<String>,
    /// Number shown in "Untitled N" while the document has no path.
    pub untitled: u32,
    /// Encoding picked with "reopen with encoding"; the session reopens the file with it.
    pub read_encoding: Option<TextEncoding>,
    pub loading: Option<Loading>,
    pub saving: Option<Receiver<SaveOutcome>>,
    /// The link with the file on disk: its stamp, pending checks and notice.
    pub disk: DiskWatch,
}

impl Tab {
    pub fn new(id: u64, doc: Document, view: EditorView, untitled: u32) -> Self {
        Self {
            id,
            doc,
            view,
            language: None,
            untitled,
            read_encoding: None,
            loading: None,
            saving: None,
            disk: DiskWatch::default(),
        }
    }

    /// File name, or "Untitled N". While a file is loading, its name.
    pub fn title(&self, texts: &Texts) -> String {
        match self.path().and_then(Path::file_name) {
            Some(name) => name.to_string_lossy().into_owned(),
            None => texts.untitled(self.untitled),
        }
    }

    /// The file shown (or being loaded) in this tab.
    pub fn path(&self) -> Option<&Path> {
        match &self.loading {
            Some(loading) => loading.path.as_deref(),
            None => self.doc.path(),
        }
    }

    /// Untitled, empty and unmodified: opening a file can take its place.
    pub fn is_blank(&self) -> bool {
        self.loading.is_none()
            && self.doc.path().is_none()
            && !self.doc.is_dirty()
            && self.doc.buffer.len_chars() == 0
    }

    /// Looks the language up again after the path changed.
    pub fn refresh_language(&mut self, registry: &LanguageRegistry) {
        self.language = self
            .doc
            .path()
            .and_then(|path| registry.for_path(path))
            .map(|language| language.id.clone());
    }
}

/// The smallest "Untitled N" number not used by another untitled tab.
pub fn free_untitled_number(tabs: &[Tab]) -> u32 {
    let used: Vec<u32> = tabs
        .iter()
        .filter(|tab| tab.path().is_none())
        .map(|tab| tab.untitled)
        .collect();
    (1..).find(|n| !used.contains(n)).unwrap_or(1)
}

/// Whether two paths name the same file, without touching the disk: Windows paths are
/// case-insensitive and may be relative (command line) or absolute (dialogs).
pub fn same_file(a: &Path, b: &Path) -> bool {
    let normalize = |path: &Path| {
        std::path::absolute(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .to_lowercase()
    };
    a == b || normalize(a) == normalize(b)
}

/// Something done with the mouse in the tab bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabAction {
    Activate(usize),
    Close(usize),
}

const TAB_HEIGHT: f32 = 30.0;
const PADDING: f32 = 10.0;
const GAP: f32 = 6.0;
const CLOSE_SIZE: f32 = 18.0;

/// Draws the tabs in one scrollable row. `reveal_active` scrolls the active tab into view.
pub fn tab_bar(
    ui: &mut Ui,
    tabs: &[Tab],
    active: usize,
    registry: &LanguageRegistry,
    texts: &Texts,
    reveal_active: bool,
) -> Option<TabAction> {
    let mut action = None;
    egui::ScrollArea::horizontal()
        .id_salt("tab-bar")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                for (index, tab) in tabs.iter().enumerate() {
                    let badge = tab
                        .language
                        .as_deref()
                        .and_then(|id| registry.get(id))
                        .map(|language| &language.badge);
                    let is_active = index == active;
                    let picked = tab_widget(ui, tab, badge, is_active, texts);
                    if is_active && reveal_active {
                        picked.response.scroll_to_me(None);
                    }
                    if picked.close {
                        action = Some(TabAction::Close(index));
                    } else if picked.activate {
                        action = Some(TabAction::Activate(index));
                    }
                }
            });
        });
    action
}

struct TabResponse {
    response: egui::Response,
    activate: bool,
    close: bool,
}

/// One tab: badge, title and a close button that shows a dot while there are unsaved changes.
fn tab_widget(
    ui: &mut Ui,
    tab: &Tab,
    badge: Option<&Badge>,
    active: bool,
    texts: &Texts,
) -> TabResponse {
    let visuals = ui.visuals().clone();
    let text_color = if active {
        visuals.strong_text_color()
    } else {
        visuals.weak_text_color()
    };
    let title = tab.title(texts);
    let title_font = TextStyle::Button.resolve(ui.style());
    let badge_font = FontId::proportional((title_font.size * 0.8).round());
    let painter = ui.painter().clone();
    let title_galley = painter.layout_no_wrap(title.clone(), title_font, text_color);
    let badge_galley =
        badge.map(|b| painter.layout_no_wrap(b.text.clone(), badge_font, Color32::WHITE));
    let badge_size = badge_galley
        .as_ref()
        .map(|g| vec2(g.size().x + 8.0, g.size().y));
    let width = PADDING
        + badge_size.map_or(0.0, |size| size.x + GAP)
        + title_galley.size().x
        + GAP
        + CLOSE_SIZE
        + GAP;

    let (rect, response) = ui.allocate_exact_size(vec2(width, TAB_HEIGHT), Sense::click());
    let close_rect = Rect::from_center_size(
        pos2(rect.right() - GAP - CLOSE_SIZE / 2.0, rect.center().y),
        vec2(CLOSE_SIZE, CLOSE_SIZE),
    );
    // Registered after the tab, so it is on top and gets its own clicks.
    let close = ui.interact(
        close_rect,
        ui.id().with(("tab-close", tab.id)),
        Sense::click(),
    );

    response.widget_info(|| {
        WidgetInfo::selected(WidgetType::SelectableLabel, true, active, title.clone())
    });
    close.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, texts.menu_close_tab()));

    if ui.is_rect_visible(rect) {
        let fill = if active {
            visuals.panel_fill
        } else if response.hovered() || close.hovered() {
            visuals.widgets.hovered.weak_bg_fill
        } else {
            Color32::TRANSPARENT
        };
        painter.rect_filled(rect, 0.0, fill);
        if active {
            let accent = Rect::from_min_size(rect.min, vec2(rect.width(), 2.0));
            painter.rect_filled(accent, 0.0, visuals.selection.bg_fill);
        }

        let mut x = rect.left() + PADDING;
        if let (Some(badge), Some(galley), Some(size)) = (badge, badge_galley, badge_size) {
            let [r, g, b] = badge.color;
            let badge_rect = Rect::from_min_size(pos2(x, rect.center().y - size.y / 2.0), size);
            painter.rect_filled(badge_rect, 3.0, Color32::from_rgb(r, g, b));
            painter.galley(
                badge_rect.center() - galley.size() / 2.0,
                galley,
                Color32::WHITE,
            );
            x += size.x + GAP;
        }
        let title_pos = pos2(x, rect.center().y - title_galley.size().y / 2.0);
        painter.galley(title_pos, title_galley, text_color);

        let mark_color = if close.hovered() {
            visuals.strong_text_color()
        } else {
            text_color
        };
        if tab.doc.is_dirty() && !close.hovered() {
            painter.circle_filled(close_rect.center(), 4.0, mark_color);
        } else if active || response.hovered() || close.hovered() {
            if close.hovered() {
                painter.rect_filled(close_rect, 3.0, visuals.widgets.active.weak_bg_fill);
            }
            let half = 4.0;
            let center = close_rect.center();
            let stroke = Stroke::new(1.4, mark_color);
            painter.line_segment(
                [center + vec2(-half, -half), center + vec2(half, half)],
                stroke,
            );
            painter.line_segment(
                [center + vec2(-half, half), center + vec2(half, -half)],
                stroke,
            );
        }
        if !active {
            // A thin separator between inactive tabs.
            let x = rect.right() + 0.5;
            painter.line_segment(
                [pos2(x, rect.top() + 7.0), pos2(x, rect.bottom() - 7.0)],
                Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color),
            );
        }
        if response.has_focus() {
            painter.rect_stroke(
                rect.shrink(1.0),
                0.0,
                visuals.selection.stroke,
                StrokeKind::Inside,
            );
        }
    }

    let activate = response.clicked();
    let close_clicked = close.clicked() || response.middle_clicked();
    let response = match tab.path() {
        Some(path) => response.on_hover_text(path.display().to_string()),
        None => response,
    };
    TabResponse {
        response,
        activate,
        close: close_clicked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bracket_core::document::LineEnding;

    fn untitled_tab(id: u64, number: u32) -> Tab {
        let doc = Document::new_empty(LineEnding::CrLf, TextEncoding::UTF8);
        Tab::new(id, doc, EditorView::for_tab(id, 14.0), number)
    }

    #[test]
    fn untitled_numbers_reuse_the_lowest_free_one() {
        assert_eq!(free_untitled_number(&[]), 1);
        let tabs = vec![untitled_tab(1, 1), untitled_tab(2, 3)];
        assert_eq!(free_untitled_number(&tabs), 2);
        let tabs = vec![untitled_tab(1, 1), untitled_tab(2, 2)];
        assert_eq!(free_untitled_number(&tabs), 3);
    }

    #[test]
    fn tabs_with_a_file_do_not_hold_an_untitled_number() {
        let mut tab = untitled_tab(1, 1);
        tab.doc.set_path(PathBuf::from(r"C:\src\a.cs"));
        assert_eq!(free_untitled_number(&[tab]), 1);
    }

    #[test]
    fn a_new_empty_tab_is_blank_until_edited() {
        let mut tab = untitled_tab(1, 1);
        assert!(tab.is_blank());
        tab.doc.buffer.insert("x");
        assert!(!tab.is_blank());
    }

    #[test]
    fn paths_compare_ignoring_case() {
        assert!(same_file(
            Path::new(r"C:\Src\Program.cs"),
            Path::new(r"c:\src\program.CS")
        ));
        assert!(!same_file(
            Path::new(r"C:\src\a.cs"),
            Path::new(r"C:\src\b.cs")
        ));
    }

    #[test]
    fn the_language_follows_the_path() {
        let registry = LanguageRegistry::builtin();
        let mut tab = untitled_tab(1, 1);
        tab.refresh_language(&registry);
        assert_eq!(tab.language, None);
        tab.doc.set_path(PathBuf::from("data.json"));
        tab.refresh_language(&registry);
        assert_eq!(tab.language.as_deref(), Some("json"));
    }

    #[test]
    fn each_tab_has_its_own_editor_id() {
        assert_ne!(untitled_tab(1, 1).view.id(), untitled_tab(2, 2).view.id());
    }
}
