//! Find in folder (Ctrl+Shift+F), in the side panel: the query with its options, files to
//! include and exclude, and the results grouped by file as they arrive. A search starts a
//! moment after typing stops (or at once with Enter) on a worker thread; a newer one cancels it.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use bracket_core::Generations;
use bracket_index::{BadPattern, WalkOptions};
use bracket_search::{
    FileMatches, FindError, FindOptions, Finder, FolderQuery, Summary, search_folder,
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
}

impl SearchPanel {
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
    ) -> Option<OpenMatch> {
        let ctx = ui.ctx().clone();
        let query_id = Id::new("search-query");
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
                });
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

        self.results_list(ui, registry)
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
        let (rows, results, collapsed) = (&self.rows, &self.results, &self.collapsed);
        egui::ScrollArea::vertical()
            .id_salt("search-results")
            .auto_shrink(false)
            .show_rows(ui, ROW_HEIGHT, rows.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for row in &rows[range] {
                    match *row {
                        Row::File(f) => {
                            let file = &results[f];
                            let open = !collapsed.contains(&file.path);
                            if file_row(ui, file, open, registry).clicked() {
                                toggled = Some(file.path.clone());
                            }
                        }
                        Row::Match(f, m) => {
                            let file = &results[f];
                            let found = &file.matches[m];
                            if match_row(ui, found).clicked() {
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
        picked
    }
}

/// A file: an arrow, its badge, its name, its folder and how many matches it has.
fn file_row(
    ui: &mut Ui,
    file: &FileMatches,
    open: bool,
    registry: &LanguageRegistry,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, file.relative.clone())
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
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
    response
}

/// A match: its line number and the line, with the match in the accent colour.
fn match_row(ui: &mut Ui, found: &bracket_search::LineMatch) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, found.preview.clone())
    });
    if !ui.is_rect_visible(rect) {
        return response;
    }
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
    let x = rect.left() + 30.0;
    let number = painter.layout_no_wrap(
        found.line.to_string(),
        FontId::proportional(font.size * 0.85),
        weak,
    );
    let number_width = number.size().x;
    painter.galley(pos2(x, center_y - number.size().y / 2.0), number, weak);
    let mut job = LayoutJob::default();
    for (i, c) in found.preview.chars().enumerate() {
        let matched = found.highlight.contains(&i);
        job.append(
            &c.to_string(),
            0.0,
            TextFormat {
                font_id: font.clone(),
                color: if matched { accent } else { text },
                background: if matched {
                    accent.gamma_multiply(0.18)
                } else {
                    Color32::TRANSPARENT
                },
                ..TextFormat::default()
            },
        );
    }
    let line = painter.layout_job(job);
    let left = x + number_width + 8.0;
    painter
        .with_clip_rect(Rect::from_min_max(pos2(left, rect.top()), rect.max))
        .galley(pos2(left, center_y - line.size().y / 2.0), line, text);
    response
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
