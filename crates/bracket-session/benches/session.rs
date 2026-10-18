//! Cost of reading the session at startup (it is read before the window opens).

use std::path::PathBuf;

use bracket_core::LineEnding;
use bracket_core::encoding::TextEncoding;
use bracket_session::session::{self, Session, SessionTab, Unsaved};
use criterion::{Criterion, criterion_group, criterion_main};

fn ten_tabs() -> String {
    let tabs = (0..10)
        .map(|i| SessionTab {
            path: Some(PathBuf::from(format!(
                r"C:\Users\someone\source\repos\Project\src\Folder{i}\File{i}.cs"
            ))),
            anchor: 1000 * i,
            head: 1000 * i + 5,
            top_line: 40 * i,
            unsaved: (i % 3 == 0).then(|| Unsaved {
                backup: format!("1760600000000-1234-{i}.txt"),
                encoding: TextEncoding::UTF8,
                line_ending: LineEnding::CrLf,
            }),
            ..SessionTab::default()
        })
        .collect();
    session::to_toml(&Session {
        tabs,
        active: 3,
        project: Some(PathBuf::from(r"C:\src\Project")),
        ..Session::default()
    })
}

fn parse(c: &mut Criterion) {
    let text = ten_tabs();
    c.bench_function("session/parse_10_tabs", |b| {
        b.iter(|| session::parse(std::hint::black_box(&text)).unwrap())
    });
}

criterion_group!(benches, parse);
criterion_main!(benches);
