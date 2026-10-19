//! Find benchmarks on a 50 MB document: what a search on the worker thread costs, from turning
//! the rope into one string to the last match.

use std::hint::black_box;

use bracket_core::{CancelToken, Rope};
use bracket_search::{FindOptions, Finder};
use criterion::{Criterion, criterion_group, criterion_main};

/// About 50 MB of code-like lines; one line in 100 mentions "Customer".
fn document() -> String {
    let mut text = String::with_capacity(52 << 20);
    let mut line = 0usize;
    while text.len() < 50 << 20 {
        if line.is_multiple_of(100) {
            text.push_str(&format!(
                "    var customer{line} = new Customer(\"c{line}\");\r\n"
            ));
        } else {
            text.push_str(&format!(
                "    total += item{line}.Price * item{line}.Quantity;\r\n"
            ));
        }
        line += 1;
    }
    text
}

fn searches(c: &mut Criterion) {
    let text = document();
    let rope = Rope::from_str(&text);
    let mut group = c.benchmark_group("find");
    group.sample_size(10);
    group.bench_function("rope_to_string_50mb", |b| {
        b.iter(|| black_box(&rope).to_string())
    });
    for (name, query, options) in [
        ("plain_50mb", "customer", FindOptions::default()),
        (
            "whole_word_50mb",
            "customer",
            FindOptions {
                whole_word: true,
                ..FindOptions::default()
            },
        ),
        (
            "regex_50mb",
            r"item\d+5\.Price",
            FindOptions {
                regex: true,
                ..FindOptions::default()
            },
        ),
    ] {
        let finder = Finder::new(query, options).unwrap();
        group.bench_function(name, |b| {
            b.iter(|| finder.find_all(black_box(&text), 100_000, &CancelToken::new()))
        });
    }
    group.finish();
}

criterion_group!(benches, searches);
criterion_main!(benches);
