//! Fuzzy search benchmarks for Ctrl+P, one query over the 50,000 files of the generated project,
//! and for Ctrl+T, one query over the 210,000 symbols of the generated C# project. The budget is
//! 50 ms per keystroke.

use std::hint::black_box;

use bracket_core::CancelToken;
use bracket_index::fuzzy::{Query, search};
use bracket_index::project_symbols::{ProjectSymbols, SymbolSource};
use bracket_index::{WalkOptions, walk};
use criterion::{Criterion, criterion_group, criterion_main};

#[path = "support/project.rs"]
mod project;

#[path = "support/csharp_project.rs"]
mod csharp_project;

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
            b.iter(|| search(black_box(&index.files[..]), &query, 200))
        });
    }
    group.finish();
}

fn symbol_queries(c: &mut Criterion) {
    let root = csharp_project::default_root();
    csharp_project::generate(&root).expect("generate the C# benchmark project");
    let cancel = CancelToken::new();
    let index = walk(&root, &WalkOptions::default(), &cancel)
        .unwrap()
        .unwrap();
    let resolve = |relative: &str| {
        relative.ends_with(".cs").then(|| SymbolSource {
            language: "csharp".into(),
            rules: None,
        })
    };
    let mut symbols = ProjectSymbols::new(root);
    symbols.sync(index.files, &resolve, &cancel).unwrap();
    assert_eq!(symbols.symbol_count(), csharp_project::SYMBOL_COUNT);

    // Gathering the names, scoring them and building the 200 results, as each keystroke does.
    let mut group = c.benchmark_group("fuzzy_symbols");
    for (name, text) in [
        ("one_letter", "c"),
        ("word", "compute"),
        ("scattered", "svc12x3"),
        ("exact", "Service12x3"),
        ("no_match", "zzz"),
    ] {
        group.bench_function(name, |b| b.iter(|| symbols.search(black_box(text), 200)));
    }
    // Go to definition: every symbol compared with one name.
    let from = Some(r"Module07\File03.cs");
    group.bench_function("definitions", |b| {
        b.iter(|| symbols.definitions(black_box("Compute3"), from, false, 200))
    });
    group.finish();
}

criterion_group!(benches, queries, symbol_queries);
criterion_main!(benches);
