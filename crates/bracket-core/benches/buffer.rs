//! Buffer benchmarks: opening a large file and inserting in the middle of it.

use std::hint::black_box;
use std::path::PathBuf;

use bracket_core::document::{self, Document, LineEnding};
use bracket_core::{Buffer, Selections};
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};

const LINES: usize = 1_000_000;

/// Deterministic text with CRLF line endings, indentation and non-ASCII chars.
fn large_text(lines: usize) -> String {
    let mut text = String::with_capacity(lines * 56);
    for i in 0..lines {
        let indent = "    ".repeat(i % 4);
        let tail: String = "public class Bracket { año 日本語 => return value; }"
            .chars()
            .take((i % 7) * 7)
            .collect();
        text.push_str(&format!("{indent}{i:07}: {tail}\r\n"));
    }
    text
}

fn open(c: &mut Criterion) {
    let text = large_text(LINES);
    let path: PathBuf =
        std::env::temp_dir().join(format!("bracket-bench-{}.txt", std::process::id()));
    std::fs::write(&path, &text).expect("write benchmark file");

    let mut group = c.benchmark_group("open");
    group.sample_size(10);
    group.bench_function("rope_from_str_1m_lines", |b| {
        b.iter(|| Buffer::from_text(black_box(&text)))
    });
    group.bench_function("load_file_1m_lines", |b| {
        b.iter(|| {
            let loaded = document::load_text(&path).expect("load benchmark file");
            Document::from_loaded(path.clone(), loaded, LineEnding::CrLf)
        })
    });
    group.finish();

    let _ = std::fs::remove_file(&path);
}

fn insert_middle(c: &mut Criterion) {
    let mut buffer = Buffer::from_text(&large_text(LINES));
    let middle = buffer.len_chars() / 2;
    buffer.set_selections(Selections::caret(middle));

    c.bench_function("insert_char_middle_1m_lines", |b| {
        // Cloning a rope is cheap (shared nodes); returning the buffer keeps its drop out of
        // the measurement.
        b.iter_batched(
            || buffer.clone(),
            |mut b| {
                b.insert("x");
                b
            },
            BatchSize::SmallInput,
        )
    });
}

criterion_group!(benches, open, insert_middle);
criterion_main!(benches);
