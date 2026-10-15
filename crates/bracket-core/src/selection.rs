//! Selections: one or more ranges over the text, each with an anchor and a head (the caret).
//!
//! All positions are char indices into the buffer, as used by `ropey`.

/// A range of text between `anchor` and `head`.
///
/// `head` is where the caret is drawn and what moves when the selection is extended;
/// `anchor` is the fixed end. When both are equal the selection is a plain caret.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
    /// Visual column the caret tries to keep when moving up and down.
    /// `None` means "use the caret's current column".
    pub goal_column: Option<usize>,
}

impl Selection {
    pub fn new(anchor: usize, head: usize) -> Self {
        Self {
            anchor,
            head,
            goal_column: None,
        }
    }

    /// An empty selection (a caret) at `pos`.
    pub fn caret(pos: usize) -> Self {
        Self::new(pos, pos)
    }

    pub fn start(&self) -> usize {
        self.anchor.min(self.head)
    }

    pub fn end(&self) -> usize {
        self.anchor.max(self.head)
    }

    /// Number of selected chars.
    pub fn len(&self) -> usize {
        self.end() - self.start()
    }

    /// True when nothing is selected. Same as [`Selection::is_caret`].
    pub fn is_empty(&self) -> bool {
        self.is_caret()
    }

    pub fn is_caret(&self) -> bool {
        self.anchor == self.head
    }

    /// True when the head is before the anchor (the selection was made right to left).
    pub fn is_backward(&self) -> bool {
        self.head < self.anchor
    }

    /// Keeps both ends within `0..=max`.
    pub fn clamped(&self, max: usize) -> Self {
        Self {
            anchor: self.anchor.min(max),
            head: self.head.min(max),
            goal_column: self.goal_column,
        }
    }
}

/// A non-empty set of selections, sorted by position and without overlaps.
/// One of them is the primary selection (the one the view follows).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selections {
    ranges: Vec<Selection>,
    primary: usize,
}

impl Selections {
    /// Builds a set from any list of selections, sorting them and merging overlaps.
    ///
    /// # Panics
    /// Panics if `ranges` is empty or `primary` is out of bounds.
    pub fn new(ranges: Vec<Selection>, primary: usize) -> Self {
        assert!(!ranges.is_empty(), "a selection set cannot be empty");
        assert!(primary < ranges.len(), "primary selection out of bounds");
        let mut set = Self { ranges, primary };
        set.normalize();
        set
    }

    pub fn single(selection: Selection) -> Self {
        Self {
            ranges: vec![selection],
            primary: 0,
        }
    }

    pub fn caret(pos: usize) -> Self {
        Self::single(Selection::caret(pos))
    }

    pub fn primary(&self) -> &Selection {
        &self.ranges[self.primary]
    }

    pub fn primary_index(&self) -> usize {
        self.primary
    }

    pub fn as_slice(&self) -> &[Selection] {
        &self.ranges
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Selection> {
        self.ranges.iter()
    }

    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    /// Always false: a selection set holds at least one selection.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Applies `f` to every selection and normalizes the result.
    pub fn map(&self, f: impl FnMut(&Selection) -> Selection) -> Self {
        Self::new(self.ranges.iter().map(f).collect(), self.primary)
    }

    /// Keeps every selection within `0..=max`.
    pub fn clamped(&self, max: usize) -> Self {
        self.map(|s| s.clamped(max))
    }

    /// Sorts by start and merges selections that overlap or touch, keeping track of the primary.
    fn normalize(&mut self) {
        let mut tagged: Vec<(Selection, bool)> = self
            .ranges
            .iter()
            .enumerate()
            .map(|(i, s)| (*s, i == self.primary))
            .collect();
        // Stable sort keeps the original order for equal starts.
        tagged.sort_by_key(|(s, _)| (s.start(), s.end()));

        let mut merged: Vec<(Selection, bool)> = Vec::with_capacity(tagged.len());
        for (sel, is_primary) in tagged {
            match merged.last_mut() {
                Some((last, last_primary)) if sel.start() <= last.end() => {
                    let start = last.start();
                    let end = last.end().max(sel.end());
                    // Keep the direction of the selection that wins the primary flag, else the first one.
                    let backward = if is_primary && !*last_primary {
                        sel.is_backward()
                    } else {
                        last.is_backward()
                    };
                    *last = if backward {
                        Selection::new(end, start)
                    } else {
                        Selection::new(start, end)
                    };
                    *last_primary |= is_primary;
                }
                _ => merged.push((sel, is_primary)),
            }
        }

        self.primary = merged.iter().position(|(_, p)| *p).unwrap_or(0);
        self.ranges = merged.into_iter().map(|(s, _)| s).collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_start_end_and_direction() {
        let s = Selection::new(7, 3);
        assert_eq!((s.start(), s.end(), s.len()), (3, 7, 4));
        assert!(s.is_backward());
        assert!(!s.is_caret());
        assert!(Selection::caret(5).is_caret());
    }

    #[test]
    fn new_sorts_selections() {
        let set = Selections::new(vec![Selection::caret(9), Selection::caret(2)], 0);
        let heads: Vec<usize> = set.iter().map(|s| s.head).collect();
        assert_eq!(heads, vec![2, 9]);
        assert_eq!(
            set.primary().head,
            9,
            "primary follows its selection after sorting"
        );
    }

    #[test]
    fn overlapping_and_touching_selections_merge() {
        let set = Selections::new(
            vec![
                Selection::new(0, 4),
                Selection::new(3, 6),
                Selection::caret(6),
                Selection::new(10, 12),
            ],
            3,
        );
        assert_eq!(
            set.as_slice(),
            &[Selection::new(0, 6), Selection::new(10, 12)]
        );
        assert_eq!(set.primary_index(), 1);
    }

    #[test]
    fn duplicate_carets_merge_and_keep_primary() {
        let set = Selections::new(vec![Selection::caret(4), Selection::caret(4)], 1);
        assert_eq!(set.len(), 1);
        assert_eq!(set.primary_index(), 0);
    }

    #[test]
    fn merge_keeps_direction_of_primary() {
        let set = Selections::new(vec![Selection::new(0, 5), Selection::new(8, 3)], 1);
        assert_eq!(set.as_slice(), &[Selection::new(8, 0)]);
    }

    #[test]
    fn clamped_limits_positions() {
        let set = Selections::new(vec![Selection::new(2, 50)], 0).clamped(10);
        assert_eq!(set.primary(), &Selection::new(2, 10));
    }

    #[test]
    #[should_panic(expected = "cannot be empty")]
    fn empty_set_panics() {
        let _ = Selections::new(Vec::new(), 0);
    }
}
