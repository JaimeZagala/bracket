//! Viewport benchmark: one full egui frame of the editor (layout, painting and tessellation)
//! for a 1000×700 window over a 1M-line document, without a window or GPU.

use std::path::PathBuf;

use bracket_core::document::{Document, LineEnding, LoadedText};
use bracket_core::{Motion, Selections};
use bracket_ui::EditorView;
use bracket_ui::fonts::{Fallback, FontFile, StartupFonts, definitions};
use criterion::{Criterion, criterion_group, criterion_main};
use eframe::egui::{self, Pos2, RawInput, Rect, vec2};

const LINES: usize = 1_000_000;

fn large_text(lines: usize) -> String {
    let mut text = String::with_capacity(lines * 56);
    for i in 0..lines {
        let indent = "\t".repeat(i % 3);
        let tail: String = "public class Bracket { año 日本語 => return value; }"
            .chars()
            .take((i % 7) * 7)
            .collect();
        text.push_str(&format!("{indent}{i:07}: {tail}\r\n"));
    }
    text
}

/// Runs one frame and returns the number of shapes painted.
fn frame(ctx: &egui::Context, view: &mut EditorView, doc: &mut Document) -> usize {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0))),
        ..Default::default()
    };
    let output = ctx.run_ui(input, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            view.show(ui, doc);
        });
    });
    let shapes = output.shapes.len();
    let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
    shapes
}

/// A context with Consolas, like the app (Bracket ships no fonts; Consolas comes with Windows).
fn context() -> egui::Context {
    let windir =
        std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
    let bytes =
        std::fs::read(windir.join("Fonts").join("consola.ttf")).expect("Consolas is installed");
    let fonts = StartupFonts {
        monospace: Some(FontFile {
            family: "Consolas".into(),
            bytes,
            index: 0,
        }),
        proportional: None,
        missing_family: None,
    };
    let ctx = egui::Context::default();
    ctx.set_fonts(definitions(fonts).expect("one font"));
    ctx
}

fn new_document() -> Document {
    Document::from_loaded(
        PathBuf::from("bench.txt"),
        LoadedText {
            text: large_text(LINES),
            encoding: bracket_core::encoding::TextEncoding::UTF8,
            exact: true,
        },
        LineEnding::CrLf,
    )
}

/// Steady frame: the same lines as the previous frame (galleys come from egui's cache).
fn paint_viewport(c: &mut Criterion) {
    let mut doc = new_document();
    let ctx = context();
    let mut view = EditorView::new();
    // As the app does once it sees CJK text without a font: request the fallback and stop
    // checking those characters (the fallback itself is not loaded here).
    view.fallback_requested(Fallback::Cjk);

    // Middle of the file, so line lookups are not trivially at the start of the rope.
    let middle = doc.buffer.text().line_to_char(LINES / 2);
    doc.buffer.set_selections(Selections::caret(middle));
    view.request_scroll_to_caret();
    // Warm-up: font atlas, galley cache and the scroll to the caret.
    let mut shapes = 0;
    for _ in 0..5 {
        shapes = frame(&ctx, &mut view, &mut doc);
    }
    // About 40 visible lines, each with a line number and its text.
    assert!(shapes > 60, "the viewport painted only {shapes} shapes");

    c.bench_function("frame_1000x700_middle_of_1m_lines", |b| {
        b.iter(|| frame(&ctx, &mut view, &mut doc))
    });
}

/// Scrolling: every frame shows a new page of lines, so each line is laid out from scratch.
fn scroll_viewport(c: &mut Criterion) {
    let mut doc = new_document();
    let ctx = context();
    let mut view = EditorView::new();
    // As the app does once it sees CJK text without a font: request the fallback and stop
    // checking those characters (the fallback itself is not loaded here).
    view.fallback_requested(Fallback::Cjk);
    for _ in 0..3 {
        frame(&ctx, &mut view, &mut doc);
    }

    c.bench_function("frame_1000x700_scrolling_page_down_1m_lines", |b| {
        b.iter(|| {
            doc.buffer.move_selections(Motion::Down(45), false);
            if doc.buffer.selections().primary().head == doc.buffer.len_chars() {
                doc.buffer.set_selections(Selections::caret(0));
            }
            view.request_scroll_to_caret();
            frame(&ctx, &mut view, &mut doc)
        })
    });
}

criterion_group!(benches, paint_viewport, scroll_viewport);
criterion_main!(benches);
