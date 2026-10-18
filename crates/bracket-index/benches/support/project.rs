//! A generated project for measuring walks: shared by the `walk` benchmark and the
//! `generate_project` example.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Folders, subfolders per folder and files per subfolder: 50 x 20 x 50 = 50,000 files.
const FOLDERS: usize = 50;
const SUBFOLDERS: usize = 20;
const FILES: usize = 50;
/// Ignored by `.gitignore`: the walk must skip them without reading them.
const IGNORED_FOLDERS: usize = 100;

pub const FILE_COUNT: usize = FOLDERS * SUBFOLDERS * FILES + 1; // + .gitignore

const EXTENSIONS: [&str; 6] = ["cs", "razor", "json", "xml", "md", "sql"];

/// `target/project-50k` in the workspace.
pub fn default_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/project-50k")
}

/// Writes the project under `root` unless a previous run finished it.
pub fn generate(root: &Path) -> io::Result<()> {
    let done = root.join("node_modules").join(".complete");
    if done.exists() {
        return Ok(());
    }
    fs::create_dir_all(root)?;
    fs::write(root.join(".gitignore"), "node_modules/\nbin/\n")?;
    for folder in 0..FOLDERS {
        for sub in 0..SUBFOLDERS {
            let dir = root
                .join(format!("src{folder:02}"))
                .join(format!("module{sub:02}"));
            fs::create_dir_all(&dir)?;
            for file in 0..FILES {
                let ext = EXTENSIONS[(folder + sub + file) % EXTENSIONS.len()];
                fs::write(dir.join(format!("File{file:02}.{ext}")), "x")?;
            }
        }
    }
    for folder in 0..IGNORED_FOLDERS {
        let dir = root
            .join("node_modules")
            .join(format!("package{folder:03}"));
        fs::create_dir_all(&dir)?;
        for file in 0..FILES {
            fs::write(dir.join(format!("index{file:02}.js")), "x")?;
        }
    }
    fs::write(done, "")
}
