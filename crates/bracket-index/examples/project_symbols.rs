//! Measures the project's symbol index on a generated C# project (2,000 files, 210,000 symbols):
//! building it, checking it again with nothing changed, reading one file again, and its memory.
//!
//! Usage: cargo run --release -p bracket-index --example project_symbols -- [path]
//! Default: target/project-cs. A finished project is not written again.

use std::time::Instant;

use bracket_core::CancelToken;
use bracket_index::project_symbols::{ProjectSymbols, SymbolSource};
use bracket_index::{WalkOptions, walk};

#[path = "../benches/support/csharp_project.rs"]
mod project;

fn main() -> std::io::Result<()> {
    let root = std::env::args()
        .nth(1)
        .map_or_else(project::default_root, Into::into);
    project::generate(&root)?;
    let cancel = CancelToken::new();
    let index = walk(&root, &WalkOptions::default(), &cancel)
        .expect("no exclusions")
        .expect("not cancelled");
    let files = index.files;
    let resolve = |relative: &str| {
        relative.ends_with(".cs").then(|| SymbolSource {
            language: "csharp".into(),
            rules: None,
        })
    };
    let bytes: u64 = files
        .iter()
        .filter_map(|f| std::fs::metadata(root.join(f)).ok())
        .map(|m| m.len())
        .sum();

    let mut symbols = ProjectSymbols::new(root.clone());
    let started = Instant::now();
    symbols
        .sync(files.clone(), &resolve, &cancel)
        .expect("not cancelled");
    let built = started.elapsed();
    assert_eq!(symbols.symbol_count(), project::SYMBOL_COUNT);

    let started = Instant::now();
    symbols
        .sync(files.clone(), &resolve, &cancel)
        .expect("not cancelled");
    let checked = started.elapsed();

    let one = [root.join(&files[files.len() / 2])];
    let started = Instant::now();
    symbols
        .refresh(
            files.clone(),
            one.iter().map(|p| p.as_path()),
            &resolve,
            &cancel,
        )
        .expect("not cancelled");
    let refreshed = started.elapsed();

    let mib = |b: f64| b / (1024.0 * 1024.0);
    println!(
        "{} files, {:.1} MiB, {} symbols in {}",
        symbols.file_count(),
        mib(bytes as f64),
        symbols.symbol_count(),
        root.display()
    );
    println!(
        "built:      {:7.1} ms ({:.1} MiB/s)",
        built.as_secs_f64() * 1000.0,
        mib(bytes as f64) / built.as_secs_f64()
    );
    println!("unchanged:  {:7.1} ms", checked.as_secs_f64() * 1000.0);
    println!("one file:   {:7.1} ms", refreshed.as_secs_f64() * 1000.0);
    println!(
        "memory:     {:7.1} MiB (estimated heap)",
        mib(symbols.heap_bytes() as f64)
    );
    Ok(())
}
