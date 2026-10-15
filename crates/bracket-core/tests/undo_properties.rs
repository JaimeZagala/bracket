//! Property tests: any sequence of edits can be fully undone and redone, and selections
//! always stay valid.

use bracket_core::{Buffer, Motion, Selection, Selections};
use proptest::prelude::*;

#[derive(Clone, Debug)]
enum Op {
    Insert(String),
    DeleteBackward,
    DeleteForward,
    /// Selections as (anchor, head) fractions of the text length, in per-mille.
    Select(Vec<(u16, u16)>),
    Seal,
    Move(Motion, bool),
}

fn text_strategy() -> impl Strategy<Value = String> {
    // Small alphabet with the cases that matter: CR, LF, CRLF, multibyte and astral chars.
    prop::collection::vec(
        prop::sample::select(vec![
            "a", "b", " ", "\n", "\r", "\r\n", "ñ", "€", "😀", "\t",
        ]),
        0..12,
    )
    .prop_map(|parts| parts.concat())
}

fn motion_strategy() -> impl Strategy<Value = Motion> {
    prop_oneof![
        Just(Motion::Left),
        Just(Motion::Right),
        Just(Motion::WordLeft),
        Just(Motion::WordRight),
        Just(Motion::LineStart),
        Just(Motion::LineEnd),
        (1usize..4).prop_map(Motion::Up),
        (1usize..4).prop_map(Motion::Down),
        Just(Motion::DocumentStart),
        Just(Motion::DocumentEnd),
    ]
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => text_strategy().prop_map(Op::Insert),
        2 => Just(Op::DeleteBackward),
        2 => Just(Op::DeleteForward),
        2 => prop::collection::vec((0u16..=1000, 0u16..=1000), 1..5).prop_map(Op::Select),
        1 => Just(Op::Seal),
        3 => (motion_strategy(), any::<bool>()).prop_map(|(m, extend)| Op::Move(m, extend)),
    ]
}

fn apply(buffer: &mut Buffer, op: &Op) {
    match op {
        Op::Insert(text) => buffer.insert(text),
        Op::DeleteBackward => buffer.delete_backward(),
        Op::DeleteForward => buffer.delete_forward(),
        Op::Select(fractions) => {
            let len = buffer.len_chars();
            let at = |per_mille: u16| len * usize::from(per_mille) / 1000;
            let ranges = fractions
                .iter()
                .map(|&(a, h)| Selection::new(at(a), at(h)))
                .collect();
            buffer.set_selections(Selections::new(ranges, 0));
        }
        Op::Seal => buffer.seal_undo_step(),
        Op::Move(motion, extend) => buffer.move_selections(*motion, *extend),
    }
}

fn assert_valid_selections(buffer: &Buffer) {
    let len = buffer.len_chars();
    let mut prev_end = None;
    for sel in buffer.selections().iter() {
        assert!(
            sel.end() <= len,
            "selection {sel:?} beyond text length {len}"
        );
        if let Some(prev) = prev_end {
            assert!(
                sel.start() > prev,
                "selections overlap or touch: {:?}",
                buffer.selections()
            );
        }
        prev_end = Some(sel.end());
    }
    assert!(buffer.selections().primary_index() < buffer.selections().len());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn undo_all_restores_original_and_redo_all_restores_final(
        initial in text_strategy(),
        ops in prop::collection::vec(op_strategy(), 0..40),
    ) {
        let mut buffer = Buffer::from_text(&initial);
        for op in &ops {
            apply(&mut buffer, op);
            assert_valid_selections(&buffer);
        }
        let edited = buffer.text().to_string();

        while buffer.undo() {
            assert_valid_selections(&buffer);
        }
        prop_assert_eq!(buffer.text().to_string(), initial);

        while buffer.redo() {
            assert_valid_selections(&buffer);
        }
        prop_assert_eq!(buffer.text().to_string(), edited);
    }

    #[test]
    fn interleaved_undo_still_restores_original(
        initial in text_strategy(),
        ops in prop::collection::vec((op_strategy(), any::<bool>()), 0..40),
    ) {
        let mut buffer = Buffer::from_text(&initial);
        for (op, undo_after) in &ops {
            apply(&mut buffer, op);
            if *undo_after {
                buffer.undo();
            }
            assert_valid_selections(&buffer);
        }
        while buffer.undo() {}
        prop_assert_eq!(buffer.text().to_string(), initial);
    }
}
