//! The editor widget: paints only the visible lines of a document and turns keyboard, mouse,
//! clipboard and IME input into buffer operations.
//!
//! All editing logic lives in `bracket-core`; this module only maps input to commands and
//! positions to pixels.

use std::collections::HashSet;
use std::sync::Arc;

use bracket_core::movement::{self, Motion};
use bracket_core::{Document, Selection, Selections};

use crate::fonts::Fallback;
use eframe::egui::output::IMEOutput;
use eframe::egui::text::{CCursor, Galley};
use eframe::egui::{
    self, Align2, Color32, Event, EventFilter, FontId, IMEPurpose, Id, ImeEvent, Key, Modifiers,
    Pos2, Rect, ScrollArea, Sense, Ui, Vec2, pos2, vec2,
};

const DEFAULT_FONT_SIZE: f32 = 14.0;

/// What the view asks the application to do after handling input.
#[derive(Debug, Default)]
pub struct ViewOutput {
    /// A visible character that no loaded font can draw (the app may load a fallback font).
    pub missing_glyph: Option<char>,
}

pub struct EditorView {
    id: Id,
    /// IME composition text, shown at the caret but not yet part of the buffer.
    preedit: String,
    /// Widest line laid out so far: sizes the horizontal scroll range without scanning the file.
    max_line_width: f32,
    /// Anchor of the selection being made with the mouse.
    drag_anchor: Option<usize>,
    scroll_to_caret: bool,
    /// Lines that fit in the viewport, from the last frame (Page Up/Down distance).
    page_lines: usize,
    font_size: f32,
    /// Fallback kinds already requested by the app: their characters are not checked again.
    fallbacks_requested: HashSet<Fallback>,
}

impl EditorView {
    pub fn with_font_size(font_size: f32) -> Self {
        Self {
            font_size,
            ..Self::new()
        }
    }

    pub fn new() -> Self {
        Self {
            id: Id::new("bracket-editor"),
            preedit: String::new(),
            max_line_width: 0.0,
            drag_anchor: None,
            scroll_to_caret: false,
            page_lines: 1,
            font_size: DEFAULT_FONT_SIZE,
            fallbacks_requested: HashSet::new(),
        }
    }

    pub fn id(&self) -> Id {
        self.id
    }

    /// Scrolls the primary caret into view on the next frame.
    pub fn request_scroll_to_caret(&mut self) {
        self.scroll_to_caret = true;
    }

    /// Stops checking characters of `kind` for missing glyphs (its fallback was requested).
    pub fn fallback_requested(&mut self, kind: Fallback) {
        self.fallbacks_requested.insert(kind);
    }

    /// Lines that fit in the viewport, as measured in the last frame.
    pub fn page_lines(&self) -> usize {
        self.page_lines
    }

    /// Forgets per-document state after another document is loaded.
    pub fn reset(&mut self) {
        *self = Self {
            id: self.id,
            font_size: self.font_size,
            fallbacks_requested: std::mem::take(&mut self.fallbacks_requested),
            ..Self::new()
        };
    }

    pub fn show(&mut self, ui: &mut Ui, doc: &mut Document) -> ViewOutput {
        let mut output = ViewOutput::default();
        let font = FontId::monospace(self.font_size);
        let (row_height, char_width) =
            ui.fonts_mut(|f| (f.row_height(&font), f.glyph_width(&font, 'M')));
        let metrics = Metrics {
            font,
            row_height,
            char_width,
            gutter_width: gutter_width(doc.buffer.len_lines(), char_width),
        };

        ScrollArea::both()
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let line_count = doc.buffer.len_lines();
                let size = vec2(
                    (metrics.gutter_width + self.max_line_width + 4.0 * char_width)
                        .max(ui.available_width()),
                    line_count as f32 * row_height,
                );
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                let response = ui.interact(rect, self.id, Sense::click_and_drag());
                let layout = Layout {
                    origin: rect.min,
                    metrics: &metrics,
                };

                self.handle_pointer(ui, &response, doc, &layout);

                self.page_lines = ((viewport.height() / row_height) as usize)
                    .saturating_sub(1)
                    .max(1);
                if response.has_focus() {
                    self.handle_events(ui, doc, self.page_lines);
                }

                // Input above may have edited the text: count the lines again before painting.
                // The content size allocated earlier is one frame behind, which is harmless.
                let line_count = doc.buffer.len_lines();
                let first =
                    ((viewport.min.y / row_height).floor().max(0.0) as usize).min(line_count);
                let last = ((viewport.max.y / row_height).ceil().max(0.0) as usize).min(line_count);
                output.missing_glyph =
                    self.paint(ui, doc, &layout, first..last, response.has_focus());

                let caret_rect = self.caret_rect(ui, doc, &layout);
                if response.has_focus() {
                    let to_global = ui
                        .ctx()
                        .layer_transform_to_global(ui.layer_id())
                        .unwrap_or_default();
                    let visible = ui.clip_rect();
                    ui.ctx().output_mut(|o| {
                        o.ime = Some(IMEOutput {
                            purpose: IMEPurpose::Normal,
                            rect: to_global * visible,
                            cursor_rect: to_global * caret_rect,
                            should_interrupt_composition: false,
                        });
                    });
                }
                if self.scroll_to_caret {
                    self.scroll_to_caret = false;
                    ui.scroll_to_rect(caret_rect.expand2(vec2(4.0 * char_width, row_height)), None);
                }
            });

        output
    }

    fn handle_pointer(
        &mut self,
        ui: &Ui,
        response: &egui::Response,
        doc: &mut Document,
        layout: &Layout<'_>,
    ) {
        let Some(point) = response.interact_pointer_pos() else {
            if !response.dragged() {
                self.drag_anchor = None;
            }
            return;
        };
        let (pressed, shift) = ui.input(|i| (i.pointer.primary_pressed(), i.modifiers.shift));
        let pos = position_at_point(ui, doc, layout, point);

        if pressed {
            response.request_focus();
            doc.buffer.seal_undo_step();
            let anchor = if shift {
                doc.buffer.selections().primary().anchor
            } else {
                pos
            };
            self.drag_anchor = Some(anchor);
            doc.buffer
                .set_selections(Selections::single(Selection::new(anchor, pos)));
        } else if response.dragged()
            && let Some(anchor) = self.drag_anchor
        {
            doc.buffer
                .set_selections(Selections::single(Selection::new(anchor, pos)));
            self.scroll_to_caret = true;
        }

        if response.double_clicked() {
            let (start, end) = movement::word_range_at(doc.buffer.text(), pos);
            doc.buffer
                .set_selections(Selections::single(Selection::new(start, end)));
            self.drag_anchor = None;
        }
    }

    fn handle_events(&mut self, ui: &Ui, doc: &mut Document, page_lines: usize) {
        // Keep Tab and the arrow keys for the editor instead of focus navigation.
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                self.id,
                EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: false,
                },
            );
        });

        let events = ui.input(|i| i.events.clone());
        for event in events {
            match event {
                Event::Ime(ime) => self.handle_ime(doc, ime),
                // While composing, the IME owns the keyboard.
                _ if !self.preedit.is_empty() => {}
                Event::Text(text) => self.edit(doc, |d| d.buffer.insert(&text)),
                Event::Paste(text) => {
                    let text = doc.line_ending().normalize(&text);
                    self.edit(doc, |d| d.buffer.insert(&text));
                }
                Event::Copy => copy_selections(ui, doc),
                Event::Cut => {
                    copy_selections(ui, doc);
                    if doc.buffer.selections().iter().any(|s| !s.is_caret()) {
                        self.edit(doc, |d| d.buffer.delete_backward());
                    }
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => self.handle_key(doc, key, modifiers, page_lines),
                _ => {}
            }
        }
    }

    fn handle_ime(&mut self, doc: &mut Document, ime: ImeEvent) {
        match ime {
            ImeEvent::Preedit { text, .. } => self.preedit = text,
            ImeEvent::Commit(text) => {
                self.preedit.clear();
                if !text.is_empty() && text != "\n" && text != "\r" {
                    self.edit(doc, |d| d.buffer.insert(&text));
                }
            }
            _ => {}
        }
        self.scroll_to_caret = true;
    }

    fn handle_key(
        &mut self,
        doc: &mut Document,
        key: Key,
        modifiers: Modifiers,
        page_lines: usize,
    ) {
        let command = modifiers.command;
        let shift = modifiers.shift;
        let motion = match key {
            Key::ArrowLeft if command => Some(Motion::WordLeft),
            Key::ArrowLeft => Some(Motion::Left),
            Key::ArrowRight if command => Some(Motion::WordRight),
            Key::ArrowRight => Some(Motion::Right),
            Key::ArrowUp => Some(Motion::Up(1)),
            Key::ArrowDown => Some(Motion::Down(1)),
            Key::Home if command => Some(Motion::DocumentStart),
            Key::Home => Some(Motion::LineStart),
            Key::End if command => Some(Motion::DocumentEnd),
            Key::End => Some(Motion::LineEnd),
            Key::PageUp => Some(Motion::Up(page_lines)),
            Key::PageDown => Some(Motion::Down(page_lines)),
            _ => None,
        };
        if let Some(motion) = motion {
            doc.buffer.move_selections(motion, shift);
            self.scroll_to_caret = true;
            return;
        }

        match key {
            Key::Backspace => self.edit(doc, |d| d.buffer.delete_backward()),
            Key::Delete => self.edit(doc, |d| d.buffer.delete_forward()),
            Key::Enter => {
                let line_ending = doc.line_ending().as_str();
                self.edit(doc, |d| d.buffer.insert(line_ending));
            }
            Key::Tab if !command && !shift => self.edit(doc, |d| d.buffer.insert("\t")),
            Key::A if command => doc.buffer.select_all(),
            Key::Z if command && shift => self.edit(doc, |d| {
                d.buffer.redo();
            }),
            Key::Z if command => self.edit(doc, |d| {
                d.buffer.undo();
            }),
            Key::Y if command => self.edit(doc, |d| {
                d.buffer.redo();
            }),
            _ => {}
        }
    }

    /// Runs a text-changing command unless the document is read-only.
    fn edit(&mut self, doc: &mut Document, command: impl FnOnce(&mut Document)) {
        if doc.is_read_only() {
            return;
        }
        command(doc);
        self.scroll_to_caret = true;
    }

    fn paint(
        &mut self,
        ui: &Ui,
        doc: &Document,
        layout: &Layout<'_>,
        lines: std::ops::Range<usize>,
        has_focus: bool,
    ) -> Option<char> {
        let mut missing_glyph = None;
        let painter = ui.painter();
        let visuals = ui.visuals();
        let text_color = visuals.text_color();
        let weak_color = visuals.weak_text_color();
        let selection_color = visuals.selection.bg_fill;
        let caret_color = visuals.text_cursor.stroke.color;
        let m = layout.metrics;
        let text = doc.buffer.text();
        let selections = doc.buffer.selections();
        let primary_head = selections.primary().head;

        for line in lines {
            let y = layout.origin.y + line as f32 * m.row_height;
            painter.text(
                pos2(layout.origin.x + m.gutter_width - m.char_width, y),
                Align2::RIGHT_TOP,
                (line + 1).to_string(),
                m.font.clone(),
                weak_color,
            );

            let line_start = movement::line_start(text, line);
            let line_end = movement::line_end(text, line);
            let preedit = (!self.preedit.is_empty()
                && (line_start..=line_end).contains(&primary_head))
            .then_some((primary_head, self.preedit.as_str()));
            let display = DisplayLine::build(doc, line, preedit);
            if missing_glyph.is_none() {
                missing_glyph =
                    first_missing_glyph(ui, &m.font, &display.text, &self.fallbacks_requested);
            }
            let galley = ui
                .fonts_mut(|f| f.layout_no_wrap(display.text.clone(), m.font.clone(), text_color));
            self.max_line_width = self.max_line_width.max(galley.size().x);
            let text_origin = pos2(layout.origin.x + m.gutter_width, y);
            let x_of = |pos: usize| text_origin.x + display.x_of(&galley, pos - line_start);

            for sel in selections.iter().filter(|s| !s.is_caret()) {
                if sel.end() < line_start || sel.start() > line_end {
                    continue;
                }
                let x0 = x_of(sel.start().max(line_start));
                let mut x1 = x_of(sel.end().min(line_end));
                // Show that the line break is selected too.
                if sel.end() > line_end {
                    x1 += m.char_width * 0.5;
                }
                painter.rect_filled(
                    Rect::from_min_max(pos2(x0, y), pos2(x1, y + m.row_height)),
                    0.0,
                    selection_color,
                );
            }

            painter.galley(text_origin, Arc::clone(&galley), text_color);

            if let Some(range) = &display.preedit_range {
                let x0 = text_origin.x + galley.pos_from_cursor(CCursor::new(range.start)).min.x;
                let x1 = text_origin.x + galley.pos_from_cursor(CCursor::new(range.end)).min.x;
                let underline_y = y + m.row_height - 1.0;
                painter.line_segment(
                    [pos2(x0, underline_y), pos2(x1, underline_y)],
                    (1.0, text_color),
                );
            }

            if has_focus {
                for sel in selections.iter() {
                    if (line_start..=line_end).contains(&sel.head) {
                        let x = x_of(sel.head);
                        painter.rect_filled(
                            Rect::from_min_size(pos2(x, y), vec2(2.0, m.row_height)),
                            0.0,
                            caret_color,
                        );
                    }
                }
            }
        }
        missing_glyph
    }

    /// Screen rectangle of the primary caret, used for IME placement and scrolling.
    fn caret_rect(&self, ui: &Ui, doc: &Document, layout: &Layout<'_>) -> Rect {
        let m = layout.metrics;
        let text = doc.buffer.text();
        let head = doc.buffer.selections().primary().head;
        let line = movement::line_of(text, head);
        let line_start = movement::line_start(text, line);
        let preedit = (!self.preedit.is_empty()).then_some((head, self.preedit.as_str()));
        let display = DisplayLine::build(doc, line, preedit);
        let galley = ui
            .fonts_mut(|f| f.layout_no_wrap(display.text.clone(), m.font.clone(), Color32::WHITE));
        let x = layout.origin.x + m.gutter_width + display.x_of(&galley, head - line_start);
        let y = layout.origin.y + line as f32 * m.row_height;
        Rect::from_min_size(pos2(x, y), vec2(2.0, m.row_height))
    }
}

impl Default for EditorView {
    fn default() -> Self {
        Self::new()
    }
}

struct Metrics {
    font: FontId,
    row_height: f32,
    char_width: f32,
    gutter_width: f32,
}

/// Where the content starts on screen and how big things are.
struct Layout<'a> {
    origin: Pos2,
    metrics: &'a Metrics,
}

fn gutter_width(line_count: usize, char_width: f32) -> f32 {
    let digits = line_count.to_string().len().max(3);
    (digits as f32 + 2.0) * char_width
}

/// One line as displayed: tabs expanded to spaces and the IME composition spliced in,
/// plus the mapping from buffer chars to displayed chars.
struct DisplayLine {
    text: String,
    /// `map[i]` is the displayed char index of the i-th char of the line; one extra entry for
    /// the line end.
    map: Vec<usize>,
    preedit_range: Option<std::ops::Range<usize>>,
}

impl DisplayLine {
    fn build(doc: &Document, line: usize, preedit: Option<(usize, &str)>) -> Self {
        let text = doc.buffer.text();
        let tab_width = doc.buffer.tab_width();
        let start = movement::line_start(text, line);
        let end = movement::line_end(text, line);

        let mut out = String::with_capacity(end - start);
        let mut map = Vec::with_capacity(end - start + 1);
        let mut shown = 0; // chars written to `out`
        let mut column = 0; // visual column, used for tab stops (the composition does not count)
        let mut preedit_range = None;

        let mut insert_preedit = |pos: usize, out: &mut String, shown: &mut usize| {
            if let Some((at, composition)) = preedit
                && at == pos
            {
                let before = *shown;
                out.push_str(composition);
                *shown += composition.chars().count();
                preedit_range = Some(before..*shown);
            }
        };

        for (i, c) in text.slice(start..end).chars().enumerate() {
            insert_preedit(start + i, &mut out, &mut shown);
            map.push(shown);
            if c == '\t' {
                let spaces = tab_width - column % tab_width;
                out.extend(std::iter::repeat_n(' ', spaces));
                shown += spaces;
                column += spaces;
            } else {
                out.push(c);
                shown += 1;
                column += 1;
            }
        }
        insert_preedit(end, &mut out, &mut shown);
        map.push(shown);

        Self {
            text: out,
            map,
            preedit_range,
        }
    }

    /// Horizontal offset of the char at `offset` within the line.
    fn x_of(&self, galley: &Galley, offset: usize) -> f32 {
        let shown = self.map[offset.min(self.map.len() - 1)];
        galley.pos_from_cursor(CCursor::new(shown)).min.x
    }

    /// Char offset within the line closest to horizontal offset `x`.
    fn offset_at(&self, galley: &Galley, x: f32) -> usize {
        let shown = galley.cursor_from_pos(Vec2::new(x, 0.0)).index.0;
        // First buffer char whose displayed position is at or after the hit, snapping a hit
        // inside an expanded tab to the nearer side.
        match self.map.binary_search(&shown) {
            Ok(i) => i,
            Err(i) => {
                let i = i.min(self.map.len() - 1);
                if i == 0 {
                    return 0;
                }
                let (left, right) = (self.map[i - 1], self.map[i]);
                if shown - left <= right - shown {
                    i - 1
                } else {
                    i
                }
            }
        }
    }
}

/// Buffer position under a screen point.
fn position_at_point(ui: &Ui, doc: &Document, layout: &Layout<'_>, point: Pos2) -> usize {
    let m = layout.metrics;
    let text = doc.buffer.text();
    let last_line = doc.buffer.len_lines() - 1;
    let line = (((point.y - layout.origin.y) / m.row_height)
        .floor()
        .max(0.0) as usize)
        .min(last_line);
    let display = DisplayLine::build(doc, line, None);
    let galley =
        ui.fonts_mut(|f| f.layout_no_wrap(display.text.clone(), m.font.clone(), Color32::WHITE));
    let x = point.x - layout.origin.x - m.gutter_width;
    movement::line_start(text, line) + display.offset_at(&galley, x)
}

/// The first character in `text` that no loaded font can draw. Invisible format characters
/// (controls, zero-width marks, BOM) are ignored: no font draws them and none needs to.
fn first_missing_glyph(
    ui: &Ui,
    font: &FontId,
    text: &str,
    requested: &HashSet<Fallback>,
) -> Option<char> {
    // Every code font covers ASCII, so the common case costs one pass over the bytes.
    if text.is_ascii() {
        return None;
    }
    ui.fonts_mut(|f| {
        text.chars()
            .filter(|c| !c.is_ascii() && !is_invisible(*c))
            .filter(|c| !requested.contains(&Fallback::for_char(*c)))
            .find(|c| !f.has_glyph(font, *c))
    })
}

fn is_invisible(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{200B}'..='\u{200F}' | '\u{2028}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{FEFF}'
        )
}

/// Copies the selected text; several selections are joined with the document's line ending.
fn copy_selections(ui: &Ui, doc: &Document) {
    let text = doc.buffer.text();
    let parts: Vec<String> = doc
        .buffer
        .selections()
        .iter()
        .filter(|s| !s.is_caret())
        .map(|s| text.slice(s.start()..s.end()).to_string())
        .collect();
    if !parts.is_empty() {
        ui.ctx().copy_text(parts.join(doc.line_ending().as_str()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bracket_core::document::LoadedText;
    use std::path::PathBuf;

    fn doc(text: &str) -> Document {
        Document::from_loaded(
            PathBuf::from("test.txt"),
            LoadedText {
                text: text.to_owned(),
                encoding: bracket_core::encoding::TextEncoding::UTF8,
                exact: true,
            },
            bracket_core::LineEnding::CrLf,
        )
    }

    #[test]
    fn tabs_expand_to_the_next_stop() {
        let d = doc("a\tb\t\tc\r\nnext");
        let line = DisplayLine::build(&d, 0, None);
        assert_eq!(line.text, "a   b       c");
        assert_eq!(line.map, vec![0, 1, 4, 5, 8, 12, 13]);
    }

    #[test]
    fn preedit_is_spliced_at_the_caret() {
        let d = doc("ab");
        let line = DisplayLine::build(&d, 0, Some((1, "日本")));
        assert_eq!(line.text, "a日本b");
        assert_eq!(line.preedit_range, Some(1..3));
        assert_eq!(line.map, vec![0, 3, 4], "chars after the caret shift right");
    }

    #[test]
    fn preedit_at_line_end() {
        let d = doc("ab\ncd");
        let line = DisplayLine::build(&d, 0, Some((2, "x")));
        assert_eq!(line.text, "abx");
        assert_eq!(line.map, vec![0, 1, 3]);
    }

    #[test]
    fn gutter_fits_the_line_count() {
        assert_eq!(gutter_width(10, 1.0), 5.0);
        assert_eq!(gutter_width(1_000_000, 1.0), 9.0);
    }

    /// Runs one headless frame of the view with `events` as input.
    fn frame(ctx: &egui::Context, view: &mut EditorView, doc: &mut Document, events: Vec<Event>) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                view.show(ui, doc);
            });
        });
        // There is no renderer to upload font textures to; egui asserts they are handled.
        output.textures_delta.clear();
    }

    /// Regression: an edit made by input inside a frame that leaves fewer lines than were on
    /// screen used to paint with the line count from before the edit and panic in ropey.
    #[test]
    fn deleting_visible_lines_within_a_frame_does_not_panic() {
        let text: Vec<String> = (1..=50).map(|i| format!("line {i}")).collect();
        let mut d = doc(&text.join("\r\n"));
        let ctx = egui::Context::default();
        let mut view = EditorView::new();
        ctx.memory_mut(|m| m.request_focus(view.id()));
        frame(&ctx, &mut view, &mut d, Vec::new());

        d.buffer.select_all();
        let delete = Event::Key {
            key: Key::Delete,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        frame(&ctx, &mut view, &mut d, vec![delete]);

        assert_eq!(d.buffer.len_chars(), 0, "the Delete key reached the editor");
        frame(&ctx, &mut view, &mut d, Vec::new());
    }
}
