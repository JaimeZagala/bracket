//! The project tree in the side panel. Each folder is listed on a worker thread the first time
//! it is opened (and again each time it is reopened); only the visible rows are drawn. Works with
//! the mouse and the keyboard, and shows a language badge next to files.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use bracket_core::CancelToken;
use bracket_index::{BadPattern, Entry, WalkOptions, list_dir};
use bracket_syntax::LanguageRegistry;
use eframe::egui::{
    self, Color32, EventFilter, FontId, Id, Key, Modifiers, Rect, Sense, Shape, Stroke, TextStyle,
    Ui, WidgetInfo, WidgetType, pos2, vec2,
};

use crate::i18n::Texts;
use crate::tabs;

const ROW_HEIGHT: f32 = 22.0;
const INDENT: f32 = 14.0;
const LEFT_PADDING: f32 = 6.0;
const CHEVRON_WIDTH: f32 = 16.0;
const GAP: f32 = 5.0;

/// What the user did in the tree that the app has to carry out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeAction {
    /// Open the file in a normal tab and move the keyboard to it.
    Open(PathBuf),
    /// Show the file in the preview tab; the keyboard stays in the tree.
    Preview(PathBuf),
    /// Escape: give the keyboard back to the editor.
    Leave,
}

/// A folder listed on the worker thread.
struct Listing {
    dir: PathBuf,
    entries: Vec<Entry>,
    /// The exclusions could not be used (the folder was listed without them).
    bad_pattern: Option<BadPattern>,
}

/// One visible row.
struct Row {
    path: PathBuf,
    name: String,
    depth: usize,
    is_dir: bool,
}

pub struct ProjectTree {
    root: PathBuf,
    /// Listed folders; `None` while a folder's first listing is on its way.
    folders: HashMap<PathBuf, Option<Vec<Entry>>>,
    /// Open folders. A folder inside a closed one stays open, as it was, for when its parent
    /// opens again.
    expanded: HashSet<PathBuf>,
    selected: Option<PathBuf>,
    /// The visible rows, rebuilt when a listing arrives or a folder opens or closes.
    rows: Vec<Row>,
    rows_stale: bool,
    /// Scroll the selected row into view on the next frame.
    reveal_selected: bool,
    /// The scroll offset and viewport height of the last frame.
    scroll: (f32, f32),
    requests: Sender<PathBuf>,
    listings: Receiver<Listing>,
    cancel: CancelToken,
    bad_pattern_reported: bool,
}

impl ProjectTree {
    /// Starts listing `root`. `notify` is called whenever a listing is ready.
    pub fn new(root: PathBuf, options: WalkOptions, notify: impl Fn() + Send + 'static) -> Self {
        let (requests, request_rx) = mpsc::channel::<PathBuf>();
        let (listing_tx, listings) = mpsc::channel();
        let cancel = CancelToken::new();
        let worker_root = root.clone();
        let worker_cancel = cancel.clone();
        thread::Builder::new()
            .name("project-tree".to_owned())
            .spawn(move || {
                let mut options = options;
                while let Ok(dir) = request_rx.recv() {
                    if worker_cancel.is_cancelled() {
                        break;
                    }
                    let (entries, bad_pattern) = match list_dir(&worker_root, &dir, &options) {
                        Ok(entries) => (entries, None),
                        Err(bad) => {
                            // Reported once; the tree goes on without the exclusions.
                            options = WalkOptions::default();
                            let entries =
                                list_dir(&worker_root, &dir, &options).unwrap_or_default();
                            (entries, Some(bad))
                        }
                    };
                    let listing = Listing {
                        dir,
                        entries,
                        bad_pattern,
                    };
                    if listing_tx.send(listing).is_err() {
                        break;
                    }
                    notify();
                }
            })
            .expect("the project tree thread starts");
        let mut tree = Self {
            folders: HashMap::new(),
            expanded: HashSet::new(),
            selected: None,
            rows: Vec::new(),
            rows_stale: true,
            reveal_selected: false,
            scroll: (0.0, 0.0),
            requests,
            listings,
            cancel,
            bad_pattern_reported: false,
            root,
        };
        tree.request(tree.root.clone());
        tree
    }

    fn id() -> Id {
        Id::new("project-tree")
    }

    /// Opens these folders (from the session); the ones outside the project are ignored.
    pub fn expand(&mut self, folders: impl IntoIterator<Item = PathBuf>) {
        let root = self.root.clone();
        self.expanded.extend(
            folders
                .into_iter()
                .filter(|f| f.starts_with(&root) && *f != root),
        );
        self.rows_stale = true;
    }

    /// The open folders worth keeping in the session: the ones that still exist as far as the
    /// listings tell, sorted.
    pub fn expanded_for_session(&self) -> Vec<PathBuf> {
        let mut folders: Vec<PathBuf> = self
            .expanded
            .iter()
            .filter(|folder| self.may_exist(folder))
            .cloned()
            .collect();
        folders.sort();
        folders
    }

    /// False if a listing of one of `folder`'s parents shows it is gone.
    fn may_exist(&self, folder: &Path) -> bool {
        let mut child = folder;
        while let Some(parent) = child.parent() {
            if let Some(Some(entries)) = self.folders.get(parent)
                && !entries.iter().any(|e| e.is_dir && e.path == child)
            {
                return false;
            }
            if parent == self.root {
                return true;
            }
            child = parent;
        }
        false
    }

    fn request(&mut self, dir: PathBuf) {
        self.folders.entry(dir.clone()).or_insert(None);
        let _ = self.requests.send(dir);
    }

    /// Takes the listings that are ready. Returns the exclusion pattern that could not be used,
    /// the first time one is found.
    pub fn poll(&mut self) -> Option<BadPattern> {
        let mut bad = None;
        while let Ok(listing) = self.listings.try_recv() {
            if let Some(pattern) = listing.bad_pattern
                && !self.bad_pattern_reported
            {
                self.bad_pattern_reported = true;
                bad = Some(pattern);
            }
            self.folders.insert(listing.dir, Some(listing.entries));
            self.rows_stale = true;
        }
        bad
    }

    /// Moves the keyboard to the tree.
    pub fn focus(&mut self, ctx: &egui::Context) {
        ctx.memory_mut(|m| m.request_focus(Self::id()));
        self.reveal_selected = true;
    }

    fn set_expanded(&mut self, folder: &Path, open: bool) {
        if open {
            self.expanded.insert(folder.to_path_buf());
            // Listed again, in case it changed since it was last open.
            self.request(folder.to_path_buf());
        } else {
            self.expanded.remove(folder);
        }
        self.rows_stale = true;
    }

    /// Rebuilds the visible rows; open folders that were never listed are requested.
    fn rebuild_rows(&mut self) {
        let mut rows = Vec::new();
        let mut missing = Vec::new();
        self.collect_rows(&self.root, 0, &mut rows, &mut missing);
        self.rows = rows;
        for dir in missing {
            self.request(dir);
        }
        self.rows_stale = false;
    }

    fn collect_rows(
        &self,
        dir: &Path,
        depth: usize,
        rows: &mut Vec<Row>,
        missing: &mut Vec<PathBuf>,
    ) {
        match self.folders.get(dir) {
            Some(Some(entries)) => {
                for entry in entries {
                    rows.push(Row {
                        path: entry.path.clone(),
                        name: entry.name.clone(),
                        depth,
                        is_dir: entry.is_dir,
                    });
                    if entry.is_dir && self.expanded.contains(&entry.path) {
                        self.collect_rows(&entry.path, depth + 1, rows, missing);
                    }
                }
            }
            Some(None) => {}
            None => missing.push(dir.to_path_buf()),
        }
    }

    fn selected_index(&self) -> Option<usize> {
        let selected = self.selected.as_ref()?;
        self.rows.iter().position(|row| &row.path == selected)
    }

    fn select(&mut self, index: usize) {
        if let Some(row) = self.rows.get(index) {
            self.selected = Some(row.path.clone());
            self.reveal_selected = true;
        }
    }

    /// The keys the tree handles while it has the keyboard.
    fn handle_keys(&mut self, ctx: &egui::Context) -> Option<TreeAction> {
        if self.rows.is_empty() {
            return None;
        }
        let pressed = |key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key));
        let page = ((self.scroll.1 / ROW_HEIGHT) as usize).max(1);
        let last = self.rows.len() - 1;
        let Some(index) = self.selected_index() else {
            if [Key::ArrowDown, Key::ArrowUp, Key::Home, Key::End]
                .into_iter()
                .any(pressed)
            {
                self.select(0);
            }
            return None;
        };
        if pressed(Key::ArrowDown) {
            self.select((index + 1).min(last));
        } else if pressed(Key::ArrowUp) {
            self.select(index.saturating_sub(1));
        } else if pressed(Key::PageDown) {
            self.select((index + page).min(last));
        } else if pressed(Key::PageUp) {
            self.select(index.saturating_sub(page));
        } else if pressed(Key::Home) {
            self.select(0);
        } else if pressed(Key::End) {
            self.select(last);
        } else if pressed(Key::ArrowRight) {
            let row = &self.rows[index];
            if row.is_dir {
                if self.expanded.contains(&row.path) {
                    // Into the first child, if it is listed already.
                    if self
                        .rows
                        .get(index + 1)
                        .is_some_and(|r| r.depth > row.depth)
                    {
                        self.select(index + 1);
                    }
                } else {
                    let path = row.path.clone();
                    self.set_expanded(&path, true);
                }
            }
        } else if pressed(Key::ArrowLeft) {
            let row = &self.rows[index];
            if row.is_dir && self.expanded.contains(&row.path) {
                let path = row.path.clone();
                self.set_expanded(&path, false);
            } else if let Some(parent) = self.rows[..index]
                .iter()
                .rposition(|r| r.depth + 1 == row.depth)
            {
                self.select(parent);
            }
        } else if pressed(Key::Enter) {
            let row = &self.rows[index];
            if row.is_dir {
                let (path, open) = (row.path.clone(), !self.expanded.contains(&row.path));
                self.set_expanded(&path, open);
            } else {
                return Some(TreeAction::Open(row.path.clone()));
            }
        } else if pressed(Key::Space) {
            let row = &self.rows[index];
            if !row.is_dir {
                return Some(TreeAction::Preview(row.path.clone()));
            }
        } else if pressed(Key::Escape) {
            return Some(TreeAction::Leave);
        }
        None
    }

    /// Draws the tree in the side panel.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        registry: &LanguageRegistry,
        texts: &Texts,
    ) -> Option<TreeAction> {
        let id = Self::id();
        // Registered first and under the rows, so it never takes their clicks; it only holds
        // the keyboard focus for the whole tree.
        let area = ui.interact(ui.max_rect(), id, Sense::focusable_noninteractive());
        let focused = area.has_focus();
        let mut action = None;
        if focused {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    EventFilter {
                        tab: false,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                );
            });
            if self.rows_stale {
                self.rebuild_rows();
            }
            action = self.handle_keys(ui.ctx());
        }
        if self.rows_stale {
            self.rebuild_rows();
        }

        let name = self.root.file_name().map_or_else(
            || self.root.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        ui.horizontal(|ui| {
            ui.add_space(LEFT_PADDING);
            ui.label(egui::RichText::new(name).strong())
                .on_hover_text(self.root.display().to_string());
        });
        ui.add_space(2.0);

        if self.rows.is_empty() {
            if matches!(self.folders.get(&self.root), Some(Some(_))) {
                ui.horizontal(|ui| {
                    ui.add_space(LEFT_PADDING);
                    ui.weak(texts.tree_empty());
                });
            }
            return action;
        }

        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("project-tree-rows")
            .auto_shrink(false);
        if self.reveal_selected
            && let Some(index) = self.selected_index()
        {
            let (offset, height) = self.scroll;
            let top = index as f32 * ROW_HEIGHT;
            if top < offset {
                scroll = scroll.vertical_scroll_offset(top);
            } else if top + ROW_HEIGHT > offset + height && height > 0.0 {
                scroll = scroll.vertical_scroll_offset(top + ROW_HEIGHT - height);
            }
        }
        self.reveal_selected = false;
        // The row clicked, and whether it was a double click.
        let mut clicked = None;
        let output = scroll.show_rows(ui, ROW_HEIGHT, self.rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for index in range {
                let row = &self.rows[index];
                let selected = self.selected.as_ref() == Some(&row.path);
                let expanded = row.is_dir && self.expanded.contains(&row.path);
                let response = row_widget(ui, row, selected, focused, expanded, registry);
                if response.clicked() || response.double_clicked() {
                    clicked = Some((index, response.double_clicked()));
                }
            }
        });
        self.scroll = (output.state.offset.y, output.inner_rect.height());

        if let Some((index, double)) = clicked {
            ui.memory_mut(|m| m.request_focus(id));
            self.select(index);
            let row = &self.rows[index];
            if row.is_dir {
                let (path, open) = (row.path.clone(), !self.expanded.contains(&row.path));
                self.set_expanded(&path, open);
            } else if double {
                action = Some(TreeAction::Open(row.path.clone()));
            } else {
                action = Some(TreeAction::Preview(row.path.clone()));
            }
        }
        action
    }
}

impl Drop for ProjectTree {
    /// Folders still waiting to be listed are not listed.
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// One row: indentation, an arrow for folders or a badge for files, and the name.
fn row_widget(
    ui: &mut Ui,
    row: &Row,
    selected: bool,
    focused: bool,
    expanded: bool,
    registry: &LanguageRegistry,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    response.widget_info(|| {
        WidgetInfo::selected(
            WidgetType::SelectableLabel,
            true,
            selected,
            row.name.clone(),
        )
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let visuals = ui.visuals();
    let painter = ui.painter();
    if selected {
        let fill = if focused {
            visuals.selection.bg_fill
        } else {
            visuals.widgets.inactive.weak_bg_fill
        };
        painter.rect_filled(rect, 0.0, fill);
    } else if response.hovered() {
        painter.rect_filled(rect, 0.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let text_color = if selected && focused {
        visuals.selection.stroke.color
    } else {
        visuals.text_color()
    };

    let mut x = rect.left() + LEFT_PADDING + row.depth as f32 * INDENT;
    let center_y = rect.center().y;
    if row.is_dir {
        let center = pos2(x + CHEVRON_WIDTH / 2.0, center_y);
        let s = 3.5;
        let points = if expanded {
            vec![
                center + vec2(-s, -s / 2.0),
                center + vec2(s, -s / 2.0),
                center + vec2(0.0, s),
            ]
        } else {
            vec![
                center + vec2(-s / 2.0, -s),
                center + vec2(s, 0.0),
                center + vec2(-s / 2.0, s),
            ]
        };
        painter.add(Shape::convex_polygon(points, text_color, Stroke::NONE));
    }
    x += CHEVRON_WIDTH;

    let font = TextStyle::Body.resolve(ui.style());
    if !row.is_dir
        && let Some(language) = registry.for_path(&row.path)
    {
        let badge_font = FontId::proportional((font.size * 0.75).round());
        x += tabs::paint_badge(painter, &language.badge, badge_font, pos2(x, center_y)) + GAP;
    }
    let galley = painter.layout_no_wrap(row.name.clone(), font, text_color);
    let text_rect = Rect::from_min_max(pos2(x, rect.top()), rect.max);
    painter.with_clip_rect(text_rect).galley(
        pos2(x, center_y - galley.size().y / 2.0),
        galley,
        Color32::WHITE,
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, Instant};

    fn project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("bracket-tree-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for path in ["src/Models/User.cs", "src/Program.cs", "README.md"] {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "x").unwrap();
        }
        root
    }

    /// Rebuilds the rows until every folder they need is listed.
    fn settle(tree: &mut ProjectTree) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            tree.poll();
            tree.rebuild_rows();
            if tree.folders.values().all(Option::is_some) {
                return;
            }
            assert!(Instant::now() < deadline, "listings never arrived");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn rows(tree: &ProjectTree) -> Vec<String> {
        tree.rows
            .iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth), r.name))
            .collect()
    }

    #[test]
    fn open_folders_show_their_children_in_order() {
        let root = project("rows");
        let mut tree = ProjectTree::new(root.clone(), WalkOptions::default(), || {});
        settle(&mut tree);
        assert_eq!(rows(&tree), ["src", "README.md"]);

        tree.expand([root.join("src"), root.join(r"src\Models")]);
        settle(&mut tree);
        assert_eq!(
            rows(&tree),
            [
                "src",
                "  Models",
                "    User.cs",
                "  Program.cs",
                "README.md"
            ]
        );

        // Closing src keeps Models open inside it.
        tree.set_expanded(&root.join("src"), false);
        settle(&mut tree);
        assert_eq!(rows(&tree), ["src", "README.md"]);
        assert_eq!(
            tree.expanded_for_session(),
            [root.join(r"src\Models")],
            "kept for when src opens again"
        );
        drop(tree);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn folders_that_are_gone_or_outside_are_not_kept() {
        let root = project("session");
        let mut tree = ProjectTree::new(root.clone(), WalkOptions::default(), || {});
        tree.expand([
            root.join("src"),
            root.join("deleted"),
            PathBuf::from(r"C:\elsewhere"),
        ]);
        settle(&mut tree);
        assert_eq!(tree.expanded_for_session(), [root.join("src")]);
        drop(tree);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_bad_exclusion_is_reported_once_and_the_tree_still_works() {
        let root = project("bad");
        let options = WalkOptions {
            exclude: vec!["src/[".into()],
        };
        let mut tree = ProjectTree::new(root.clone(), options, || {});
        tree.expand([root.join("src")]);
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut reported = Vec::new();
        while tree.folders.len() < 2 || tree.folders.values().any(Option::is_none) {
            reported.extend(tree.poll());
            tree.rebuild_rows();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        reported.extend(tree.poll());
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].pattern, "src/[");
        assert_eq!(
            rows(&tree),
            ["src", "  Models", "  Program.cs", "README.md"]
        );
        drop(tree);
        fs::remove_dir_all(&root).unwrap();
    }
}
