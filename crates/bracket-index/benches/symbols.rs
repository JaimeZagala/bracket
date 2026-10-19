//! Symbol extraction benchmark: a synthetic C# file of about 1.5 MB (4,000 classes with
//! fields, properties and methods), parsed with tree-sitter and queried for its symbols.

use std::fmt::Write;
use std::hint::black_box;

use bracket_index::symbols::extract;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};

fn csharp_file() -> String {
    let mut text = String::from("namespace Demo.App;\n\n");
    for c in 0..4000 {
        let _ = write!(
            text,
            "public sealed class Service{c} : IService\n{{\n    private readonly int _value{c} = {c};\n    \
             public string Name{c} {{ get; init; }} = \"n{c}\";\n    \
             public int Compute{c}(int x, string y) => x + _value{c} + y.Length;\n    \
             public async Task<int> RunAsync{c}(CancellationToken token)\n    {{\n        \
             await Task.Delay({}, token);\n        return Compute{c}({c}, \"s\");\n    }}\n}}\n\n",
            c % 10
        );
    }
    text
}

fn symbols(c: &mut Criterion) {
    let text = csharp_file();
    let found = extract("csharp", &text).unwrap();
    assert_eq!(found.len(), 1 + 4000 * 5);
    let mut group = c.benchmark_group("symbols");
    group.sample_size(10);
    group.throughput(Throughput::Bytes(text.len() as u64));
    group.bench_function("csharp_1_5mb", |b| {
        b.iter(|| extract("csharp", black_box(&text)))
    });
    group.finish();
}

criterion_group!(benches, symbols);
criterion_main!(benches);
