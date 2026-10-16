//! Compiles the grammars Bracket ships into one file embedded in the executable.
//!
//! syntect's bundled grammars (from sublimehq/Packages) provide C#, CSS, JSON and XML, plus
//! HTML and JavaScript, which Razor builds on. Razor and T-SQL are Bracket's own grammars in
//! `grammars/`. The whole bundle is kept (75 languages, about 370 KB compressed): its grammars
//! refer to each other by position, so single ones cannot be taken out of it.

use std::env;
use std::fs;
use std::path::PathBuf;

use syntect::parsing::{SyntaxDefinition, SyntaxSet};

const OWN: [&str; 2] = [
    "grammars/Razor.sublime-syntax",
    "grammars/T-SQL.sublime-syntax",
];

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=grammars");

    let mut builder = SyntaxSet::load_defaults_newlines().into_builder();
    for file in OWN {
        let source = fs::read_to_string(file).unwrap_or_else(|e| panic!("{file}: {e}"));
        let syntax = SyntaxDefinition::load_from_str(&source, true, None)
            .unwrap_or_else(|e| panic!("{file}: {e}"));
        builder.add(syntax);
    }
    let set = builder.build();

    // A grammar that includes one that was left out would silently lose its highlighting.
    let unlinked = set.find_unlinked_contexts();
    assert!(
        unlinked.is_empty(),
        "grammars refer to missing ones: {unlinked:?}"
    );

    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    syntect::dumps::dump_to_file(&set, out.join("grammars.packdump"))
        .expect("writing the compiled grammars");
}
