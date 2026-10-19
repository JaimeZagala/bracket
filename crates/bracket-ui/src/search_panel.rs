//! Find in folder (Ctrl+Shift+F), in the side panel: the query with its options, files to
//! include and exclude, and the results grouped by file as they arrive. A search starts a
//! moment after typing stops (or at once with Enter) on a worker thread; a newer one cancels it.
//!
//! With the replace row open (Ctrl+Shift+H) each match shows what replaces it, with check
//! boxes to leave files or matches out, and "Replace all" hands the plan to the app.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use bracket_core::{CancelToken, Document, Generations};
use bracket_index::{BadPattern, WalkOptions};
use bracket_search::{
    FileMatches, FileReplace, FindError, FindOptions, Finder, FolderQuery, Replacement, Summary,
    search_folder,
};
use bracket_syntax::LanguageRegistry;
use eframe::egui::{
    self, Color32, FontId, Id, Key, Modifiers, Rect, RichText, Sense, Shape, Stroke, TextStyle, Ui,
    pos2,
    text::{CCursor, CCursorRange, LayoutJob, TextFormat},
    vec2,
};

use crate::i18n::Texts;
use crate::tabs;

/// Typing waits this long before searching.
const DEBOUNCE: Duration = Duration::from_millis(300);
/// Matches shown at most.
pub const LIMIT: usize = 20_000;
const ROW_HEIGHT: f32 = 22.0;

/// Open this file at this place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenMatch {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
}

/// The matches to replace, file by file, as checked in the panel.
#[derive(Debug, Clone)]
pub struct ReplacePlan {
    pub finder: Finder,
    pub template: String,
    pub files: Vec<FileReplace>,
    /// Matches chosen, in all files.
    pub matches: usize,
}

/// Replaces the chosen matches of a file that is open in a tab: in its buffer, as one undo
/// step, without saving. Returns how many were replaced, or `None` if the document is
/// read-only or its text is not what was searched (it has unsaved changes).
pub fn replace_in_document(
    doc: &mut Document,
    finder: &Finder,
    template: &str,
    file: &FileReplace,
) -> Option<usize> {
    if doc.is_read_only() {
        return None;
    }
    let replacement = Replacement::new(template, finder, doc.line_ending().as_str());
    let text = doc.buffer.text().to_string();
    let all = finder.replace_all(&text, &replacement, &CancelToken::new())?;
    if all.len() != file.expected {
        return None;
    }
    let chosen: Vec<_> = file
        .chosen
        .iter()
        .filter_map(|&i| all.get(i).cloned())
        .collect();
    doc.buffer.replace_ranges(&chosen);
    Some(chosen.len())
}

/// What the user asked for in the panel.
#[derive(Debug, Clone)]
pub enum SearchAction {
    Open(OpenMatch),
    ReplaceAll(ReplacePlan),
}

enum SearchEvent {
    File(FileMatches),
    Done(Result<Summary, BadPattern>),
}

/// One visible row of the results.
#[derive(Clone, Copy)]
enum Row {
    File(usize),
    Match(usize, usize),
}

#[derive(Default)]
pub struct SearchPanel {
    query: String,
    options: FindOptions,
    include: String,
    exclude: String,
    focus: bool,
    changed_at: Option<Instant>,
    events: Option<Receiver<SearchEvent>>,
    searches: Generations,
    results: Vec<FileMatches>,
    collapsed: HashSet<PathBuf>,
    rows: Vec<Row>,
    summary: Option<Summary>,
    error: Option<String>,
    /// The replace row is shown and matches come with their replacement.
    replace_open: bool,
    replacement: String,
    focus_replacement: bool,
    /// Files and matches (by index in their file) left out of "Replace all".
    unchecked_files: HashSet<PathBuf>,
    unchecked: HashSet<(PathBuf, usize)>,
}

impl SearchPanel {
    /// Shows the replace row (Ctrl+Shift+H) and gives it the keyboard if there is a query.
    pub fn open_replace(&mut self) {
        if !self.replace_open {
            self.replace_open = true;
            // The results need their replacements.
            if !self.query.is_empty() {
                self.changed_at = Some(Instant::now() - DEBOUNCE);
            }
        }
        if self.query.is_empty() {
            self.focus = true;
        } else {
            self.focus_replacement = true;
        }
    }

    /// Searches again at once (after "Replace all" changed the files).
    pub fn refresh(&mut self) {
        if !self.query.is_empty() {
            self.changed_at = Some(Instant::now() - DEBOUNCE);
        }
    }

    fn is_checked(&self, path: &Path, index: Option<usize>) -> bool {
        !self.unchecked_files.contains(path)
            && index.is_none_or(|i| !self.unchecked.contains(&(path.to_path_buf(), i)))
    }

    /// The checked matches as a plan for "Replace all".
    fn plan(&self) -> Option<ReplacePlan> {
        let finder = Finder::new(&self.query, self.options).ok()?;
        let mut matches = 0;
        let files: Vec<FileReplace> = self
            .results
            .iter()
            .filter(|f| self.is_checked(&f.path, None))
            .filter_map(|f| {
                let chosen: Vec<usize> = (0..f.matches.len())
                    .filter(|&i| self.is_checked(&f.path, Some(i)))
                    .collect();
                matches += chosen.len();
                (!chosen.is_empty()).then(|| FileReplace {
                    path: f.path.clone(),
                    expected: f.matches.len(),
                    chosen,
                })
            })
            .collect();
        (!files.is_empty()).then(|| ReplacePlan {
            finder,
            template: self.replacement.clone(),
            files,
            matches,
        })
    }
    /// Gives the query field the keyboard on the next frame, with its text selected.
    pub fn focus(&mut self) {
        self.focus = true;
    }

    /// Forgets the results (the project changed) and stops a running search.
    pub fn reset(&mut self) {
        self.searches.cancel();
        self.events = None;
        self.results.clear();
        self.rows.clear();
        self.summary = None;
        self.error = None;
        // Search the new project as soon as the panel is drawn.
        if !self.query.is_empty() {
            self.changed_at = Some(Instant::now() - DEBOUNCE);
        }
    }

    /// Takes the results that arrived. Call every frame.
    pub fn poll(&mut self) {
        let Some(events) = &self.events else { return };
        let mut changed = false;
        for event in events.try_iter() {
            match event {
                SearchEvent::File(file) => {
                    let key = file.relative.to_lowercase();
                    let at = self
                        .results
                        .partition_point(|f| f.relative.to_lowercase() < key);
                    self.results.insert(at, file);
                    changed = true;
                }
                SearchEvent::Done(Ok(summary)) => {
                    self.summary = Some(summary);
                    self.searches.cancel();
                }
                SearchEvent::Done(Err(bad)) => {
                    self.error = Some(format!("{}\u{1}{}", bad.pattern, bad.error));
                    self.searches.cancel();
                }
            }
        }
        if changed {
            self.rebuild_rows();
        }
    }

    fn running(&self) -> bool {
        self.searches.is_running()
    }

    fn rebuild_rows(&mut self) {
        self.rows.clear();
        for (f, file) in self.results.iter().enumerate() {
            self.rows.push(Row::File(f));
            if !self.collapsed.contains(&file.path) {
                self.rows
                    .extend((0..file.matches.len()).map(|m| Row::Match(f, m)));
            }
        }
    }

    /// Starts searching `root` with the current query, filters and options.
    fn start(&mut self, ctx: &egui::Context, root: &Path, project_exclude: &[String]) {
        self.changed_at = None;
        self.results.clear();
        self.rows.clear();
        self.collapsed.clear();
        self.unchecked.clear();
        self.unchecked_files.clear();
        self.summary = None;
        self.error = None;
        self.events = None;
        let finder = match Finder::new(&self.query, self.options) {
            Ok(finder) => finder,
            Err(FindError::Empty) => {
                self.searches.cancel();
                return;
            }
            Err(FindError::Invalid(message)) => {
                self.searches.cancel();
                self.error = Some(message);
                return;
            }
        };
        let split = |text: &str| -> Vec<String> {
            text.split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_owned)
                .collect()
        };
        let mut exclude = project_exclude.to_vec();
        exclude.extend(split(&self.exclude));
        let query = FolderQuery {
            root: root.to_path_buf(),
            finder,
            walk: WalkOptions {
                exclude,
                include: split(&self.include),
            },
            limit: LIMIT,
            replacement: self.replace_open.then(|| self.replacement.clone()),
        };
        let task = self.searches.start();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let file_tx = tx.clone();
            let repaint = ctx.clone();
            let summary = search_folder(&query, &task.token, move |file| {
                let _ = file_tx.send(SearchEvent::File(file));
                repaint.request_repaint();
            });
            let _ = tx.send(SearchEvent::Done(summary));
            ctx.request_repaint();
        });
        self.events = Some(rx);
    }

    /// Draws the panel. `project_exclude`: the configured exclusions, always applied.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        root: &Path,
        project_exclude: &[String],
        registry: &LanguageRegistry,
        texts: &Texts,
    ) -> Option<SearchAction> {
        let ctx = ui.ctx().clone();
        let query_id = Id::new("search-query");
        let replace_id = Id::new("search-replacement");
        let mut replace_all = false;
        let mut start_now = false;
        if ui.memory(|m| m.has_focus(query_id))
            && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter))
        {
            start_now = true;
        }
        let mut changed = false;
        ui.add_space(4.0);
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(6, 0))
            .show(ui, |ui| {
                let mut output = egui::TextEdit::singleline(&mut self.query)
                    .id(query_id)
                    .hint_text(texts.search_hint())
                    .desired_width(f32::INFINITY)
                    .show(ui);
                if self.focus {
                    output.response.request_focus();
                    let all = CCursorRange::two(
                        CCursor::new(0),
                        CCursor::new(self.query.chars().count()),
                    );
                    output.state.cursor.set_char_range(Some(all));
                    output.state.store(ui.ctx(), query_id);
                    self.focus = false;
                }
                changed |= output.response.changed();
                ui.horizontal(|ui| {
                    for (label, tip, option) in [
                        ("Aa", texts.find_match_case(), &mut self.options.match_case),
                        ("ab", texts.find_whole_word(), &mut self.options.whole_word),
                        (".*", texts.find_regex(), &mut self.options.regex),
                    ] {
                        let text = RichText::new(label).monospace();
                        if ui
                            .selectable_label(*option, text)
                            .on_hover_text(tip)
                            .clicked()
                        {
                            *option = !*option;
                            changed = true;
                        }
                    }
                    ui.separator();
                    if ui
                        .selectable_label(self.replace_open, texts.search_replace_toggle())
                        .on_hover_text(texts.search_replace_toggle_tip())
                        .clicked()
                    {
                        self.replace_open = !self.replace_open;
                        changed = true;
                    }
                });
                if self.replace_open {
                    let mut output = egui::TextEdit::singleline(&mut self.replacement)
                        .id(replace_id)
                        .hint_text(texts.replace_hint())
                        .desired_width(f32::INFINITY)
                        .show(ui);
                    if self.focus_replacement {
                        output.response.request_focus();
                        let all = CCursorRange::two(
                            CCursor::new(0),
                            CCursor::new(self.replacement.chars().count()),
                        );
                        output.state.cursor.set_char_range(Some(all));
                        output.state.store(ui.ctx(), replace_id);
                        self.focus_replacement = false;
                    }
                    changed |= output.response.changed();
                    let ready =
                        !self.running() && self.summary.is_some() && !self.results.is_empty();
                    if ui
                        .add_enabled(ready, egui::Button::new(texts.replace_all()))
                        .on_hover_text(texts.search_replace_all_tip())
                        .clicked()
                    {
                        replace_all = true;
                    }
                }
                for (text, hint) in [
                    (&mut self.include, texts.search_include()),
                    (&mut self.exclude, texts.search_exclude()),
                ] {
                    let field = egui::TextEdit::singleline(text)
                        .hint_text(hint)
                        .desired_width(f32::INFINITY);
                    changed |= ui.add(field).changed();
                }
                ui.add_space(2.0);
                self.summary_line(ui, texts);
            });
        ui.add_space(4.0);

        if changed {
            self.changed_at = Some(Instant::now());
            if self.query.is_empty() {
                self.searches.cancel();
                self.events = None;
                self.results.clear();
                self.rows.clear();
                self.summary = None;
                self.error = None;
                self.changed_at = None;
            }
        }
        if start_now {
            self.start(&ctx, root, project_exclude);
        } else if let Some(at) = self.changed_at {
            let waited = at.elapsed();
            if waited >= DEBOUNCE {
                self.start(&ctx, root, project_exclude);
            } else {
                ctx.request_repaint_after(DEBOUNCE - waited);
            }
        }

        if replace_all && let Some(plan) = self.plan() {
            return Some(SearchAction::ReplaceAll(plan));
        }
        self.results_list(ui, registry).map(SearchAction::Open)
    }

    fn summary_line(&self, ui: &mut Ui, texts: &Texts) {
        if let Some(error) = &self.error {
            let text = match error.split_once('\u{1}') {
                Some((pattern, reason)) => texts.search_bad_filter(pattern, reason),
                None => texts.find_invalid().to_owned(),
            };
            let label = ui.colored_label(ui.visuals().error_fg_color, text);
            if !error.contains('\u{1}') {
                label.on_hover_text(error.as_str());
            }
            return;
        }
        let matches: usize = self.results.iter().map(|f| f.matches.len()).sum();
        if self.running() {
            let text = if matches == 0 {
                texts.search_searching().to_owned()
            } else {
                format!(
                    "{} {}",
                    texts.search_searching(),
                    texts.search_summary(&matches.to_string(), &self.results.len().to_string())
                )
            };
            ui.weak(text);
            return;
        }
        let Some(summary) = &self.summary else { return };
        if matches == 0 {
            ui.weak(texts.search_none());
        } else {
            ui.weak(texts.search_summary(&matches.to_string(), &self.results.len().to_string()));
        }
        if summary.truncated {
            ui.weak(texts.search_truncated(&LIMIT.to_string()));
        }
        if summary.files_skipped > 0 {
            ui.weak(texts.search_skipped(&summary.files_skipped.to_string()));
        }
    }

    fn results_list(&mut self, ui: &mut Ui, registry: &LanguageRegistry) -> Option<OpenMatch> {
        let mut picked = None;
        let mut toggled = None;
        // A check box clicked: the file, and the match in it (`None` for the whole file).
        let mut checked: Option<(PathBuf, Option<usize>)> = None;
        let this = &*self;
        let replacing = this.replace_open;
        egui::ScrollArea::vertical()
            .id_salt("search-results")
            .auto_shrink(false)
            .show_rows(ui, ROW_HEIGHT, this.rows.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for row in &this.rows[range] {
                    match *row {
                        Row::File(f) => {
                            let file = &this.results[f];
                            let open = !this.collapsed.contains(&file.path);
                            let check = replacing.then(|| this.is_checked(&file.path, None));
                            let (response, check_clicked) =
                                file_row(ui, file, open, check, registry);
                            if check_clicked {
                                checked = Some((file.path.clone(), None));
                            } else if response.clicked() {
                                toggled = Some(file.path.clone());
                            }
                        }
                        Row::Match(f, m) => {
                            let file = &this.results[f];
                            let found = &file.matches[m];
                            let check = replacing.then(|| this.is_checked(&file.path, Some(m)));
                            let (response, check_clicked) = match_row(ui, found, check);
                            if check_clicked {
                                checked = Some((file.path.clone(), Some(m)));
                            } else if response.clicked() {
                                picked = Some(OpenMatch {
                                    path: file.path.clone(),
                                    line: found.line,
                                    column: found.column,
                                });
                            }
                        }
                    }
                }
            });
        if let Some(path) = toggled {
            if !self.collapsed.remove(&path) {
                self.collapsed.insert(path);
            }
            self.rebuild_rows();
        }
        match checked {
            Some((path, None)) => {
                if !self.unchecked_files.remove(&path) {
                    self.unchecked_files.insert(path);
                }
            }
            Some((path, Some(index))) => {
                let key = (path, index);
                if !self.unchecked.remove(&key) {
                    self.unchecked.insert(key);
                }
            }
            None => {}
        }
        picked
    }
}

/// A check box at `left_center`, registered over the row so it gets its own clicks. Returns
/// whether it was clicked.
fn check_box(
    ui: &mut Ui,
    left_center: egui::Pos2,
    checked: bool,
    salt: impl std::hash::Hash + std::fmt::Debug,
) -> bool {
    let rect = Rect::from_center_size(left_center + vec2(7.0, 0.0), vec2(14.0, 14.0));
    let response = ui.interact(rect, ui.id().with(("check", salt)), Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, checked, ""));
    let visuals = ui.visuals();
    let stroke = Stroke::new(1.0, visuals.text_color());
    let painter = ui.painter();
    let box_rect = rect.shrink(1.5);
    if checked {
        painter.rect_filled(box_rect, 2.0, visuals.selection.bg_fill);
    }
    painter.rect_stroke(box_rect, 2.0, stroke, egui::StrokeKind::Inside);
    if checked {
        let c = box_rect.center();
        let mark = Stroke::new(1.6, visuals.selection.stroke.color);
        painter.line_segment([c + vec2(-3.5, 0.0), c + vec2(-1.0, 2.5)], mark);
        painter.line_segment([c + vec2(-1.0, 2.5), c + vec2(3.5, -2.5)], mark);
    }
    response.clicked()
}

/// A file: an arrow, its badge, its name, its folder and how many matches it has; with a
/// check box when replacing. Returns the row and whether the check box was clicked.
fn file_row(
    ui: &mut Ui,
    file: &FileMatches,
    open: bool,
    check: Option<bool>,
    registry: &LanguageRegistry,
) -> (egui::Response, bool) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, file.relative.clone())
    });
    if !ui.is_rect_visible(rect) {
        return (response, false);
    }
    let check_clicked = check.is_some_and(|checked| {
        check_box(
            ui,
            pos2(rect.left() + 22.0, rect.center().y),
            checked,
            &file.path,
        )
    });
    let visuals = ui.visuals();
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, 0.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let text = visuals.text_color();
    let weak = visuals.weak_text_color();
    let center_y = rect.center().y;
    let mut x = rect.left() + 6.0;
    let center = pos2(x + 8.0, center_y);
    let s = 3.5;
    let points = if open {
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
    painter.add(Shape::convex_polygon(points, text, Stroke::NONE));
    x += 16.0;
    if check.is_some() {
        x += 18.0;
    }
    let font = TextStyle::Body.resolve(ui.style());
    if let Some(language) = registry.for_path(&file.path) {
        let badge_font = FontId::proportional((font.size * 0.75).round());
        x += tabs::paint_badge(painter, &language.badge, badge_font, pos2(x, center_y)) + 5.0;
    }
    let (folder, name) = match file.relative.rsplit_once('\\') {
        Some((folder, name)) => (folder, name),
        None => ("", file.relative.as_str()),
    };
    let count = painter.layout_no_wrap(
        file.matches.len().to_string(),
        FontId::proportional(font.size * 0.85),
        weak,
    );
    let right = rect.right() - 8.0 - count.size().x;
    painter.galley(pos2(right, center_y - count.size().y / 2.0), count, weak);
    let clip = painter.with_clip_rect(Rect::from_min_max(
        pos2(x, rect.top()),
        pos2(right - 6.0, rect.bottom()),
    ));
    let name = painter.layout_no_wrap(name.to_owned(), font.clone(), text);
    let name_width = name.size().x;
    clip.galley(pos2(x, center_y - name.size().y / 2.0), name, text);
    let folder = painter.layout_no_wrap(
        folder.to_owned(),
        FontId::proportional(font.size * 0.85),
        weak,
    );
    clip.galley(
        pos2(x + name_width + 8.0, center_y - folder.size().y / 2.0),
        folder,
        weak,
    );
    (response, check_clicked)
}

/// A match: its line number and the line, with the match in the accent colour. When replacing,
/// a check box and the replacement after the struck-out match. Returns the row and whether the
/// check box was clicked.
fn match_row(
    ui: &mut Ui,
    found: &bracket_search::LineMatch,
    check: Option<bool>,
) -> (egui::Response, bool) {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, found.preview.clone())
    });
    if !ui.is_rect_visible(rect) {
        return (response, false);
    }
    let check_clicked = check.is_some_and(|checked| {
        let salt = (found.line, found.column, &found.preview);
        check_box(ui, pos2(rect.left() + 28.0, rect.center().y), checked, salt)
    });
    let visuals = ui.visuals();
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, 0.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let font = TextStyle::Body.resolve(ui.style());
    let text = visuals.text_color();
    let weak = visuals.weak_text_color();
    let accent = visuals.hyperlink_color;
    let center_y = rect.center().y;
    let x = rect.left() + if check.is_some() { 48.0 } else { 30.0 };
    let number = painter.layout_no_wrap(
        found.line.to_string(),
        FontId::proportional(font.size * 0.85),
        weak,
    );
    let number_width = number.size().x;
    painter.galley(pos2(x, center_y - number.size().y / 2.0), number, weak);
    // With a replacement (and the match checked): the match struck out, then what replaces it.
    let replacement = found.replacement.as_deref().filter(|_| check == Some(true));
    let removed = visuals.error_fg_color;
    let added = if visuals.dark_mode {
        Color32::from_rgb(110, 200, 120)
    } else {
        Color32::from_rgb(30, 130, 50)
    };
    let mut job = LayoutJob::default();
    let format = |color: Color32, background: Color32, strike: bool| TextFormat {
        font_id: font.clone(),
        color,
        background,
        strikethrough: if strike {
            Stroke::new(1.0, color)
        } else {
            Stroke::NONE
        },
        ..TextFormat::default()
    };
    let unchecked = check == Some(false);
    for (i, c) in found.preview.chars().enumerate() {
        let matched = found.highlight.contains(&i);
        let style = match (matched, replacement.is_some(), unchecked) {
            (_, _, true) => format(weak, Color32::TRANSPARENT, false),
            (true, true, _) => format(removed, removed.gamma_multiply(0.12), true),
            (true, false, _) => format(accent, accent.gamma_multiply(0.18), false),
            (false, _, _) => format(text, Color32::TRANSPARENT, false),
        };
        job.append(&c.to_string(), 0.0, style);
        if let Some(new) = replacement
            && i + 1 == found.highlight.end
        {
            job.append(new, 0.0, format(added, added.gamma_multiply(0.15), false));
        }
    }
    let line = painter.layout_job(job);
    let left = x + number_width + 8.0;
    painter
        .with_clip_rect(Rect::from_min_max(pos2(left, rect.top()), rect.max))
        .galley(pos2(left, center_y - line.size().y / 2.0), line, text);
    (response, check_clicked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn results_arrive_in_order_and_files_collapse() {
        let root = std::env::temp_dir().join(format!("bracket-panel-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        for name in ["b.txt", "A.txt", "c.txt"] {
            fs::write(root.join(name), "needle\nneedle\n").unwrap();
        }
        let ctx = egui::Context::default();
        let mut panel = SearchPanel {
            query: "needle".into(),
            ..SearchPanel::default()
        };
        panel.start(&ctx, &root, &[]);
        let deadline = Instant::now() + Duration::from_secs(10);
        while panel.summary.is_none() {
            assert!(Instant::now() < deadline, "the search never finished");
            thread::sleep(Duration::from_millis(5));
            panel.poll();
        }
        let names: Vec<&str> = panel.results.iter().map(|f| f.relative.as_str()).collect();
        assert_eq!(names, ["A.txt", "b.txt", "c.txt"]);
        assert_eq!(panel.rows.len(), 3 + 6);
        panel.collapsed.insert(root.join("b.txt"));
        panel.rebuild_rows();
        assert_eq!(panel.rows.len(), 3 + 4);

        panel.reset();
        assert!(panel.results.is_empty() && panel.rows.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_plan_has_only_the_checked_matches() {
        let root = std::env::temp_dir().join(format!("bracket-plan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        for name in ["a.txt", "b.txt"] {
            fs::write(root.join(name), "old old old\n").unwrap();
        }
        let ctx = egui::Context::default();
        let mut panel = SearchPanel {
            query: "old".into(),
            replacement: "new".into(),
            replace_open: true,
            ..SearchPanel::default()
        };
        panel.start(&ctx, &root, &[]);
        let deadline = Instant::now() + Duration::from_secs(10);
        while panel.summary.is_none() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
            panel.poll();
        }
        assert!(
            panel.results[0]
                .matches
                .iter()
                .all(|m| m.replacement.as_deref() == Some("new"))
        );
        panel.unchecked.insert((root.join("a.txt"), 1));
        panel.unchecked_files.insert(root.join("b.txt"));
        let plan = panel.plan().unwrap();
        assert_eq!(plan.matches, 2);
        assert_eq!(plan.files.len(), 1);
        assert_eq!(plan.files[0].chosen, [0, 2]);
        assert_eq!(plan.files[0].expected, 3);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn open_documents_change_in_their_buffer_unless_their_text_differs() {
        use bracket_core::LineEnding;
        use bracket_core::encoding::TextEncoding;
        let mut doc = Document::new_empty(LineEnding::CrLf, TextEncoding::UTF8);
        doc.buffer.insert("old old old");
        let finder = Finder::new("old", FindOptions::default()).unwrap();
        let file = FileReplace {
            path: PathBuf::from("x.txt"),
            expected: 3,
            chosen: vec![1],
        };
        assert_eq!(
            replace_in_document(&mut doc, &finder, "new", &file),
            Some(1)
        );
        assert_eq!(doc.buffer.text().to_string(), "old new old");
        assert!(doc.is_dirty(), "not saved");
        assert!(doc.buffer.undo());
        assert_eq!(doc.buffer.text().to_string(), "old old old");
        // Unsaved changes made the text differ from what was searched.
        doc.buffer.insert(" old");
        assert_eq!(replace_in_document(&mut doc, &finder, "new", &file), None);
    }

    #[test]
    fn an_invalid_query_or_filter_is_reported() {
        let ctx = egui::Context::default();
        let root = std::env::temp_dir();
        let mut panel = SearchPanel {
            query: "(".into(),
            options: FindOptions {
                regex: true,
                ..FindOptions::default()
            },
            ..SearchPanel::default()
        };
        panel.start(&ctx, &root, &[]);
        assert!(panel.error.is_some());
        assert!(!panel.running());
    }
}
