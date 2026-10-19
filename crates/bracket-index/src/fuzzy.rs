//! Fuzzy matching of file paths, for Ctrl+P: the letters typed must appear in the path in the
//! same order, not necessarily together. The best alignment of those letters is scored, so
//! matches at the start of a word, letters in a row and letters in the file name rank higher.
//!
//! Matching ignores case. Spaces in the query are dropped and `/` counts as `\`.

/// Points for every matched letter.
const MATCH: i32 = 16;
/// The first letter skipped between two matched letters, and each one after it.
const GAP_START: i32 = 3;
const GAP: i32 = 1;
/// Two matched letters in a row.
const CONSECUTIVE: i32 = 6;
/// A letter at the start of a folder or file name, after `.`, `_`, `-` or a space, or where
/// a lower case letter is followed by an upper case one (`FileIndex`).
const SEGMENT_START: i32 = 12;
const WORD_START: i32 = 9;
const CAMEL: i32 = 8;
/// Each matched letter in the file name rather than in its folders.
const IN_NAME: i32 = 3;
/// The query is the whole file name, or its stem.
const WHOLE_NAME: i32 = 60;

const NONE: i32 = i32::MIN / 2;

/// Below this many candidates per thread, starting threads costs more than it saves.
const PARALLEL_SLICE: usize = 4096;

/// A query ready for matching.
#[derive(Clone, Debug)]
pub struct Query {
    chars: Vec<char>,
}

impl Query {
    pub fn new(text: &str) -> Self {
        Self {
            chars: text
                .chars()
                .filter(|c| !c.is_whitespace())
                .map(|c| if c == '/' { '\\' } else { lower(c) })
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }
}

/// A candidate that matched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    /// Position in the candidate list.
    pub index: usize,
    pub score: i32,
    /// The matched characters (char indices into the candidate), ascending.
    pub positions: Vec<usize>,
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// The best `limit` matches, best first. Ties go to the shorter candidate, then alphabetical
/// order, then the earlier one in the list.
pub fn search<S: AsRef<str> + Sync>(candidates: &[S], query: &Query, limit: usize) -> Vec<Match> {
    if query.is_empty() {
        return Vec::new();
    }
    let text = |index: usize| candidates[index].as_ref();
    let score_range = |range: std::ops::Range<usize>| -> Vec<(i32, usize)> {
        range
            .filter(|&index| is_subsequence(text(index), &query.chars))
            .filter_map(|index| score(text(index), query).map(|s| (s, index)))
            .collect()
    };
    // Large lists are scored on several threads, in equal slices.
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(8)
        .min(candidates.len() / PARALLEL_SLICE)
        .max(1);
    let mut scored: Vec<(i32, usize)> = if threads == 1 {
        score_range(0..candidates.len())
    } else {
        let slice = candidates.len().div_ceil(threads);
        std::thread::scope(|scope| {
            let workers: Vec<_> = (0..threads)
                .map(|t| {
                    let range = t * slice..((t + 1) * slice).min(candidates.len());
                    scope.spawn(move || score_range(range))
                })
                .collect();
            workers
                .into_iter()
                .flat_map(|w| w.join().expect("scoring does not panic"))
                .collect()
        })
    };
    let order = |a: &(i32, usize), b: &(i32, usize)| {
        b.0.cmp(&a.0)
            .then_with(|| text(a.1).len().cmp(&text(b.1).len()))
            .then_with(|| text(a.1).cmp(text(b.1)))
            .then_with(|| a.1.cmp(&b.1))
    };
    if scored.len() > limit && limit > 0 {
        scored.select_nth_unstable_by(limit - 1, order);
        scored.truncate(limit);
    }
    scored.sort_unstable_by(order);
    scored
        .into_iter()
        .map(|(score, index)| Match {
            index,
            score,
            positions: positions(text(index), query).unwrap_or_default(),
        })
        .collect()
}

/// A quick check before scoring: are the query's letters in the candidate, in order?
fn is_subsequence(candidate: &str, query: &[char]) -> bool {
    let mut wanted = query.iter();
    let mut next = wanted.next();
    for c in candidate.chars() {
        match next {
            Some(&q) if lower(c) == q => next = wanted.next(),
            Some(_) => {}
            None => break,
        }
    }
    next.is_none()
}

/// What the scoring needs to know about a candidate.
struct Prepared {
    chars: Vec<char>,
    bonus: Vec<i32>,
    /// Where the file name starts.
    name_start: usize,
}

fn prepare(candidate: &str) -> Prepared {
    let original: Vec<char> = candidate.chars().collect();
    let name_start = original
        .iter()
        .rposition(|&c| c == '\\' || c == '/')
        .map_or(0, |i| i + 1);
    let bonus = original
        .iter()
        .enumerate()
        .map(|(i, &c)| {
            let here = match i.checked_sub(1).map(|p| original[p]) {
                None | Some('\\' | '/') => SEGMENT_START,
                Some('.' | '_' | '-' | ' ') => WORD_START,
                Some(prev) if prev.is_lowercase() && c.is_uppercase() => CAMEL,
                _ => 0,
            };
            here + if i >= name_start { IN_NAME } else { 0 }
        })
        .collect();
    Prepared {
        chars: original
            .iter()
            .map(|&c| if c == '/' { '\\' } else { lower(c) })
            .collect(),
        bonus,
        name_start,
    }
}

/// The candidate's score, or `None` if the query's letters are not all in it, in order.
pub fn score(candidate: &str, query: &Query) -> Option<i32> {
    let prepared = prepare(candidate);
    let rows = align(&prepared, &query.chars, false)?;
    Some(finish(&prepared, &query.chars, rows.best))
}

/// The positions of the best alignment, for highlighting.
pub fn positions(candidate: &str, query: &Query) -> Option<Vec<usize>> {
    let prepared = prepare(candidate);
    let rows = align(&prepared, &query.chars, true)?;
    let n = prepared.chars.len();
    let mut positions = vec![0; query.chars.len()];
    let mut at = rows.best_end;
    for j in (0..query.chars.len()).rev() {
        positions[j] = at;
        if j > 0 {
            at = rows.from[j * n + at];
        }
    }
    Some(positions)
}

/// Bonuses that depend on the whole candidate, and a small penalty for long paths.
fn finish(prepared: &Prepared, query: &[char], best: i32) -> i32 {
    let name: String = prepared.chars[prepared.name_start..].iter().collect();
    let stem = name
        .rsplit_once('.')
        .map_or(name.as_str(), |(stem, _)| stem);
    let query: String = query.iter().collect();
    let whole = if query == name || query == stem {
        WHOLE_NAME
    } else {
        0
    };
    best + whole - (prepared.chars.len() as i32) / 4
}

struct Alignment {
    best: i32,
    best_end: usize,
    /// For each query letter after the first and each candidate position, the position of
    /// the previous letter in the best alignment ending there (only when asked for).
    from: Vec<usize>,
}

/// The best alignment of the query in the candidate (Smith-Waterman style, with linear gaps):
/// one row per query letter, keeping only the previous row's scores.
fn align(prepared: &Prepared, query: &[char], keep_path: bool) -> Option<Alignment> {
    let (n, m) = (prepared.chars.len(), query.len());
    if m == 0 || m > n {
        return None;
    }
    let mut previous = vec![NONE; n];
    let mut current = vec![NONE; n];
    let mut from = if keep_path {
        vec![0; m * n]
    } else {
        Vec::new()
    };
    for (i, &c) in prepared.chars.iter().enumerate() {
        if c == query[0] {
            // The first letter's bonus counts double: where a match starts matters most.
            previous[i] = MATCH + 2 * prepared.bonus[i] - (i as i32).min(8);
        }
    }
    for j in 1..m {
        // Best `previous[k] + GAP * k` for k <= i - 2 (a gap of at least one letter).
        let mut gapped = NONE;
        let mut gapped_at = 0;
        for i in 0..n {
            current[i] = NONE;
            if i >= 2 && previous[i - 2] > NONE {
                let value = previous[i - 2] + GAP * (i as i32 - 2);
                if value > gapped {
                    gapped = value;
                    gapped_at = i - 2;
                }
            }
            if prepared.chars[i] != query[j] {
                continue;
            }
            let gain = MATCH + prepared.bonus[i];
            let mut best = NONE;
            let mut best_from = 0;
            if gapped > NONE {
                best = gapped - GAP_START - GAP * (i as i32 - 2) + gain;
                best_from = gapped_at;
            }
            if i >= 1 && previous[i - 1] > NONE {
                let consecutive = previous[i - 1] + gain + CONSECUTIVE;
                if consecutive >= best {
                    best = consecutive;
                    best_from = i - 1;
                }
            }
            current[i] = best;
            if keep_path {
                from[j * n + i] = best_from;
            }
        }
        std::mem::swap(&mut previous, &mut current);
    }
    let (best_end, &best) = previous
        .iter()
        .enumerate()
        .max_by_key(|&(i, &s)| (s, std::cmp::Reverse(i)))?;
    (best > NONE).then_some(Alignment {
        best,
        best_end,
        from,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|p| (*p).to_owned()).collect()
    }

    fn ranked(list: &[&str], query: &str) -> Vec<String> {
        let candidates = paths(list);
        search(&candidates, &Query::new(query), 10)
            .into_iter()
            .map(|m| candidates[m.index].clone())
            .collect()
    }

    #[test]
    fn letters_must_appear_in_order() {
        let list = [r"src\Program.cs", r"docs\readme.md"];
        assert_eq!(ranked(&list, "prg"), [r"src\Program.cs"]);
        assert_eq!(ranked(&list, "gpr"), Vec::<String>::new());
        assert_eq!(
            ranked(&list, "PROGRAM"),
            [r"src\Program.cs"],
            "case is ignored"
        );
        assert!(ranked(&list, "").is_empty());
    }

    #[test]
    fn the_file_name_and_word_starts_rank_higher() {
        let list = [
            r"src\Models\program\Notes.txt",
            r"src\Program.cs",
            r"tests\ProgramTests.cs",
        ];
        assert_eq!(ranked(&list, "program")[0], r"src\Program.cs");
        let list = [r"src\Services\FileIndex.cs", r"src\Fifteen.cs"];
        assert_eq!(ranked(&list, "fi")[0], r"src\Fifteen.cs");
        assert_eq!(
            ranked(&list, "fidx")[0],
            r"src\Services\FileIndex.cs",
            "camel case starts"
        );
    }

    #[test]
    fn folders_can_be_typed_with_either_slash_and_spaces_are_ignored() {
        let list = [r"src\Models\User.cs", r"tests\User.cs"];
        assert_eq!(ranked(&list, "models/user")[0], r"src\Models\User.cs");
        assert_eq!(ranked(&list, "mod user")[0], r"src\Models\User.cs");
    }

    #[test]
    fn ties_go_to_the_shorter_path() {
        let list = [r"a\b\c\User.cs", r"a\User.cs"];
        assert_eq!(ranked(&list, "user.cs")[0], r"a\User.cs");
    }

    #[test]
    fn positions_mark_the_best_alignment() {
        // F of File, then "ind" of Index (a camel case start, then letters in a row) rather
        // than the i of File.
        let found = positions(r"src\FileIndex.cs", &Query::new("find")).unwrap();
        assert_eq!(found, [4, 8, 9, 10]);
        let query = Query::new("program");
        let found = positions(r"src\Program.cs", &query).unwrap();
        assert_eq!(found, (4..11).collect::<Vec<_>>(), "in a row, in the name");
    }

    #[test]
    fn only_the_best_ones_are_returned() {
        let candidates: Vec<String> = (0..100).map(|i| format!(r"src\File{i:02}.cs")).collect();
        let found = search(&candidates, &Query::new("file"), 5);
        assert_eq!(found.len(), 5);
        assert!(found.windows(2).all(|w| w[0].score >= w[1].score));
    }

    #[test]
    fn non_ascii_names_work() {
        let list = [r"docs\Diseño\Señal.md"];
        assert_eq!(ranked(&list, "señal"), list);
        assert_eq!(positions(list[0], &Query::new("ñ")).unwrap().len(), 1);
    }
}
