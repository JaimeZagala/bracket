//! Find in folder benchmark: one query over the 50,000 files of the generated project
//! (`cargo run --release -p bracket-index --example generate_project`). Each file is read,
//! decoded and searched, so this measures the whole pipeline with a warm file cache.

use std::hint::black_box;
use std::path::Path;

use bracket_core::CancelToken;
use bracket_index::WalkOptions;
use bracket_search::{FindOptions, Finder, FolderQuery, search_folder};
use criterion::{Criterion, criterion_group, criterion_main};

fn folder(c: &mut Criterion) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/project-50k");
    if !root.join("node_modules").join(".complete").exists() {
        eprintln!("skipped: run the generate_project example of bracket-index first");
        return;
    }
    let query = FolderQuery {
        root,
        finder: Finder::new("x", FindOptions::default()).unwrap(),
        walk: WalkOptions::default(),
        limit: usize::MAX,
    };
    let mut group = c.benchmark_group("folder");
    group.sample_size(10);
    group.bench_function("search_50k_files", |b| {
        b.iter(|| search_folder(black_box(&query), &CancelToken::new(), |_| {}).unwrap())
    });
    group.finish();
}

criterion_group!(benches, folder);
criterion_main!(benches);
