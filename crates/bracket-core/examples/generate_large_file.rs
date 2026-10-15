//! Generates a large text file for manual and benchmark testing.
//!
//! Usage: cargo run --release -p bracket-core --example generate_large_file -- [lines] [path]
//! Defaults: 1,000,000 lines into target/large-1m.txt. The content is deterministic and mixes
//! line lengths, indentation with tabs and spaces, non-ASCII text and CRLF line endings.

use std::fs::File;
use std::io::{BufWriter, Write};

const WORDS: &[&str] = &[
    "public",
    "class",
    "Bracket",
    "editor",
    "void",
    "return",
    "string",
    "int",
    "var",
    "await",
    "año",
    "niño",
    "señal",
    "ü",
    "日本語",
    "€",
    "=>",
    "{",
    "}",
    "(",
    ")",
    ";",
    "//",
    "SELECT",
];

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let lines: usize = args
        .next()
        .map(|s| s.parse().expect("lines must be a number"))
        .unwrap_or(1_000_000);
    let path = args
        .next()
        .unwrap_or_else(|| "target/large-1m.txt".to_owned());

    let mut out = BufWriter::new(File::create(&path)?);
    // Small linear congruential generator: deterministic without extra dependencies.
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        (state >> 33) as usize
    };

    for line in 0..lines {
        let indent = next() % 4;
        if line % 7 == 0 {
            write!(out, "{}", "\t".repeat(indent))?;
        } else {
            write!(out, "{}", "    ".repeat(indent))?;
        }
        write!(out, "{line:07}:")?;
        let words = next() % 16;
        for _ in 0..words {
            write!(out, " {}", WORDS[next() % WORDS.len()])?;
        }
        out.write_all(b"\r\n")?;
    }
    out.flush()?;
    println!("Wrote {lines} lines to {path}");
    Ok(())
}
