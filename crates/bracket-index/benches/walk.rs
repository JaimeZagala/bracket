//! Project walk benchmarks: the whole-project file index and listing one folder, on a generated
//! project with 50,000 files.

use std::hint::black_box;

use bracket_core::CancelToken;
use bracket_index::{WalkOptions, list_dir, walk};
use criterion::{Criterion, criterion_group, criterion_main};

#[path = "support/project.rs"]
mod project;

fn walks(c: &mut Criterion) {
    let root = project::default_root();
    project::generate(&root).expect("generate the benchmark project");
    let options = WalkOptions::default();
    let index = walk(&root, &options, &CancelToken::new()).unwrap().unwrap();
    assert_eq!(index.files.len(), project::FILE_COUNT);

    let mut group = c.benchmark_group("walk");
    group.sample_size(10);
    group.bench_function("index_50k_files", |b| {
        b.iter(|| walk(black_box(&root), &options, &CancelToken::new()))
    });
    group.bench_function("list_root", |b| {
        b.iter(|| list_dir(&root, black_box(&root), &options))
    });
    // The worst case of an update: a change at the root rewrites the whole sorted list.
    let mut resynced = index.clone();
    group.bench_function("resync_root", |b| {
        b.iter(|| resynced.resync_dir(black_box(&root), &options))
    });
    let deep = root.join(r"src07\module03");
    group.bench_function("resync_deep_folder", |b| {
        b.iter(|| resynced.resync_dir(black_box(&deep), &options))
    });
    assert_eq!(resynced.files, index.files);
    group.finish();
}

criterion_group!(benches, walks);
criterion_main!(benches);
