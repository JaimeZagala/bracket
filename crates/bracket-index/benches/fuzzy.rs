//! Fuzzy search benchmarks for Ctrl+P: one query over the 50,000 files of the generated project.
//! The budget is 50 ms per keystroke.

use std::hint::black_box;

use bracket_core::CancelToken;
use bracket_index::fuzzy::{Query, search};
use bracket_index::{WalkOptions, walk};
use criterion::{Criterion, criterion_group, criterion_main};

#[path = "support/project.rs"]
mod project;

fn queries(c: &mut Criterion) {
    let root = project::default_root();
    project::generate(&root).expect("generate the benchmark project");
    let index = walk(&root, &WalkOptions::default(), &CancelToken::new())
        .unwrap()
        .unwrap();
    assert_eq!(index.files.len(), project::FILE_COUNT);

    let mut group = c.benchmark_group("fuzzy");
    // From a single letter (almost everything matches) to a long, precise query.
    for (name, text) in [
        ("one_letter", "f"),
        ("word", "file"),
        ("scattered", "fl12cs"),
        ("path", "src07/module03/file12"),
        ("no_match", "zzz"),
    ] {
        let query = Query::new(text);
        group.bench_function(name, |b| {
            b.iter(|| search(black_box(&index.files), &query, 200))
        });
    }
    group.finish();
}

criterion_group!(benches, queries);
criterion_main!(benches);
