//! A generated C# project for measuring the project's symbol index: 2,000 files with 105 symbols
//! each (210,000 symbols, about 16 MB).

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Folders and files per folder: 40 x 50 = 2,000 files.
const FOLDERS: usize = 40;
const FILES: usize = 50;
const CLASSES: usize = 4;
const MEMBERS: usize = 5;

pub const FILE_COUNT: usize = FOLDERS * FILES;
/// Per file: the namespace, and per class the class itself, 5 fields, 10 properties and 10
/// methods.
pub const SYMBOL_COUNT: usize = FILE_COUNT * (1 + CLASSES * (1 + 5 * MEMBERS));

/// `target/project-cs` in the workspace.
pub fn default_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/project-cs")
}

/// Writes the project under `root` unless a previous run finished it.
pub fn generate(root: &Path) -> io::Result<()> {
    let done = root.join(".complete");
    if done.exists() {
        return Ok(());
    }
    for folder in 0..FOLDERS {
        let dir = root.join(format!("Module{folder:02}"));
        fs::create_dir_all(&dir)?;
        for file in 0..FILES {
            fs::write(dir.join(format!("File{file:02}.cs")), source(folder, file))?;
        }
    }
    fs::write(done, "")
}

fn source(folder: usize, file: usize) -> String {
    let mut text = format!("using System;\n\nnamespace Demo.Module{folder:02}\n{{\n");
    for class in 0..CLASSES {
        let name = format!("Service{file:02}x{class}");
        let _ = writeln!(text, "    /// <summary>Generated class {name}.</summary>");
        let _ = writeln!(text, "    public class {name}\n    {{");
        for m in 0..MEMBERS {
            let _ = writeln!(text, "        private int _count{m};");
        }
        for m in 0..2 * MEMBERS {
            let _ = writeln!(
                text,
                "        public string Name{m} {{ get; set; }} = \"{name}.{m}\";"
            );
        }
        for m in 0..2 * MEMBERS {
            let _ = writeln!(
                text,
                "        public int Compute{m}(int value)\n        {{\n            var total = value + _count{};\n            return total * {m};\n        }}\n",
                m % MEMBERS
            );
        }
        text.push_str("    }\n\n");
    }
    text.push_str("}\n");
    text
}
