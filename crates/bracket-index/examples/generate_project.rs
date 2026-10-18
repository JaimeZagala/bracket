//! Generates a project with 50,000 files (and 5,000 ignored ones) for manual and benchmark
//! testing of the project walk.
//!
//! Usage: cargo run --release -p bracket-index --example generate_project -- [path]
//! Default: target/project-50k. A finished project is not written again.

#[path = "../benches/support/project.rs"]
mod project;

fn main() -> std::io::Result<()> {
    let root = std::env::args()
        .nth(1)
        .map_or_else(project::default_root, Into::into);
    project::generate(&root)?;
    println!("{} files in {}", project::FILE_COUNT, root.display());
    Ok(())
}
