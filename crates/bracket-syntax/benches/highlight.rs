//! Highlighting cost: a first pass over a C# file, typing near the top while looking further
//! down, and scrolling through text already highlighted.

use std::hint::black_box;

use bracket_core::Rope;
use bracket_syntax::highlight::Engine;
use criterion::{Criterion, criterion_group, criterion_main};

fn csharp(lines: usize) -> String {
    let mut text = String::from("namespace Demo;\n\npublic class Sample\n{\n");
    for i in 0..lines / 4 {
        text.push_str(&format!(
            "    // Method {i}\n    public int Get{i}(string name) => name.Length + {i};\n    private readonly List<string> _items{i} = new();\n\n"
        ));
    }
    text.push_str("}\n");
    text
}

fn highlight(c: &mut Criterion) {
    let text = Rope::from_str(&csharp(10_000));

    c.bench_function("highlight/first_pass_10k_lines_csharp", |b| {
        b.iter(|| {
            let mut engine = Engine::new("source.cs").unwrap();
            engine.set_text(&text);
            black_box(engine.highlight(0..text.len_lines(), || false))
        })
    });

    // A page in the middle after typing in a comment near the top: the parser state is back
    // to the old one at the next checkpoint, so the rest is reused.
    let mut engine = Engine::new("source.cs").unwrap();
    engine.set_text(&text);
    engine.highlight(0..text.len_lines(), || false);
    let mut edited = text.clone();
    edited.insert(text.line_to_char(12) + 10, "x");
    c.bench_function("highlight/page_at_line_5000_after_typing_at_line_12", |b| {
        b.iter(|| {
            engine.set_text(&text);
            engine.set_text(&edited);
            black_box(engine.highlight(5000..5050, || false))
        })
    });

    // Scrolling through text that was already highlighted: from the nearest checkpoint.
    c.bench_function("highlight/page_at_line_5000_unchanged", |b| {
        b.iter(|| black_box(engine.highlight(5000..5050, || false)))
    });
}

criterion_group!(benches, highlight);
criterion_main!(benches);
