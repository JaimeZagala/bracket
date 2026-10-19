//! The symbols of every file in a project, kept up to date as files change.
//!
//! Files are read and their symbols extracted on several threads at once, each thread with one
//! parser per language for all its files. Each file keeps its names in one string and its
//! symbols as small fixed-size records, so a project with hundreds of thousands of symbols stays
//! within a few megabytes.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use bracket_core::CancelToken;
use bracket_core::document::DiskStamp;
use bracket_core::encoding;

use crate::Cancelled;
use crate::fuzzy::{self, Query};
use crate::rules::RuleSet;
use crate::symbols::{Symbol, SymbolKind, SymbolParser};

/// Larger files are left out (generated code, data dumps): they keep no symbols.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// With more changed paths than this in one batch, every file is checked again instead.
const MAX_CHANGED: usize = 256;

/// Threads that read files. Each one leaves the heap a little larger after a full read (about
/// half a megabyte), and more threads gain little: 2,000 C# files take 0.95 s with 4 threads and
/// 0.51 s with 15.
const MAX_THREADS: usize = 4;

/// How to get the symbols of a file: its language (a `bracket-syntax` id) and a language
/// package's rules, which win over the built-in extractor.
#[derive(Clone, Debug)]
pub struct SymbolSource {
    /// Shared: every file of a language uses the same one.
    pub language: Arc<str>,
    pub rules: Option<Arc<RuleSet>>,
}

/// The source of a file, by its path relative to the project; `None` for files without symbols.
pub type Resolve = dyn Fn(&str) -> Option<SymbolSource> + Sync;

/// A symbol of a project file. Its name and parent are read through [`FileSymbols`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectSymbol {
    name_start: u32,
    name_end: u32,
    parent: u32,
    pub kind: SymbolKind,
    /// Where its name starts: line, 0-based.
    pub line: u32,
    /// Where its name starts: column in chars, 0-based.
    pub column: u32,
}

const NO_PARENT: u32 = u32::MAX;

/// The symbols of one file. Most files of a project have none, so they keep only their stamp.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileSymbols {
    /// The file as it was read; `None` when the symbols come from unsaved text.
    stamp: Option<DiskStamp>,
    found: Option<Box<Found>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Found {
    /// Every name, one after another.
    names: Box<str>,
    symbols: Box<[ProjectSymbol]>,
}

impl FileSymbols {
    /// In document order.
    pub fn symbols(&self) -> &[ProjectSymbol] {
        self.found.as_ref().map_or(&[], |f| &f.symbols)
    }

    pub fn name(&self, symbol: &ProjectSymbol) -> &str {
        let names = self.found.as_ref().map_or("", |f| &f.names);
        &names[symbol.name_start as usize..symbol.name_end as usize]
    }

    /// The symbol `symbol` is declared in, if any.
    pub fn parent(&self, symbol: &ProjectSymbol) -> Option<&ProjectSymbol> {
        (symbol.parent != NO_PARENT).then(|| &self.symbols()[symbol.parent as usize])
    }

    fn new(text: &str, found: &[Symbol], stamp: Option<DiskStamp>) -> Self {
        // The char offset where each line starts.
        let mut line_starts = vec![0];
        line_starts.extend(
            text.chars()
                .enumerate()
                .filter(|(_, c)| *c == '\n')
                .map(|(i, _)| i + 1),
        );
        if found.is_empty() {
            return Self { stamp, found: None };
        }
        let mut names = String::with_capacity(found.iter().map(|s| s.name.len()).sum());
        let symbols = found
            .iter()
            .map(|s| {
                let start = s.name_range.start;
                let line = line_starts.partition_point(|&l| l <= start) - 1;
                let name_start = names.len() as u32;
                names.push_str(&s.name);
                ProjectSymbol {
                    name_start,
                    name_end: names.len() as u32,
                    parent: s.parent.map_or(NO_PARENT, |p| p as u32),
                    kind: s.kind,
                    line: line as u32,
                    column: (start - line_starts[line]) as u32,
                }
            })
            .collect();
        Self {
            stamp,
            found: Some(Box::new(Found {
                names: names.into_boxed_str(),
                symbols,
            })),
        }
    }

    fn heap_bytes(&self) -> usize {
        self.found.as_ref().map_or(0, |f| {
            size_of::<Found>() + f.names.len() + size_of_val(&*f.symbols)
        })
    }
}

/// A project symbol found by a search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundSymbol {
    pub name: String,
    pub kind: SymbolKind,
    /// The name of the symbol it is declared in, if any.
    pub container: Option<String>,
    /// Its file, relative to the project.
    pub relative: String,
    /// Where its name starts: line and column in chars, 0-based.
    pub line: u32,
    pub column: u32,
    /// The matched characters of the name (char indices).
    pub positions: Vec<usize>,
}

/// A symbol as the fuzzy search sees it: its name, and where to find it again.
struct Candidate<'a> {
    name: &'a str,
    file: u32,
    symbol: u32,
}

impl AsRef<str> for Candidate<'_> {
    fn as_ref(&self) -> &str {
        self.name
    }
}

/// The symbols of a project's files.
pub struct ProjectSymbols {
    root: PathBuf,
    /// The project's files as the file index lists them: relative, sorted by their lowercase form.
    files: Arc<Vec<String>>,
    /// Every listed file with a symbol source that could be read, by its listed path.
    by_file: HashMap<Box<str>, FileSymbols>,
    /// The unsaved text of open documents, by relative path: it wins over the file on disk.
    unsaved: HashMap<String, String>,
    symbol_count: usize,
}

impl ProjectSymbols {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            files: Arc::default(),
            by_file: HashMap::new(),
            unsaved: HashMap::new(),
            symbol_count: 0,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Files read, with or without symbols.
    pub fn file_count(&self) -> usize {
        self.by_file.len()
    }

    pub fn symbol_count(&self) -> usize {
        self.symbol_count
    }

    /// The symbols of a file, by its path relative to the project.
    pub fn get(&self, relative: &str) -> Option<&FileSymbols> {
        self.by_file.get(relative)
    }

    /// Every file read, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &FileSymbols)> {
        self.by_file.iter().map(|(path, file)| (&**path, file))
    }

    /// Roughly the heap memory held: names, symbol records and the table of files.
    pub fn heap_bytes(&self) -> usize {
        let entry = size_of::<Box<str>>() + size_of::<FileSymbols>() + 1;
        let table = self.by_file.capacity() * entry;
        table
            + self
                .by_file
                .iter()
                .map(|(path, file)| path.len() + file.heap_bytes())
                .sum::<usize>()
    }

    /// The best `limit` symbols whose names match `query` fuzzily (as Ctrl+P matches paths),
    /// best first; among equals, in the order of the files and of the symbols in them. The list
    /// of names is gathered for each query: it costs a few milliseconds and no memory between
    /// queries.
    pub fn search(&self, query: &str, limit: usize) -> Vec<FoundSymbol> {
        let query = Query::new(query);
        if query.is_empty() {
            return Vec::new();
        }
        let mut files = Vec::new();
        let mut candidates = Vec::with_capacity(self.symbol_count);
        for path in self.files.iter() {
            let Some(file) = self.by_file.get(path.as_str()) else {
                continue;
            };
            if file.symbols().is_empty() {
                continue;
            }
            let number = files.len() as u32;
            files.push((path, file));
            candidates.extend(file.symbols().iter().enumerate().map(|(i, s)| Candidate {
                name: file.name(s),
                file: number,
                symbol: i as u32,
            }));
        }
        fuzzy::search(&candidates, &query, limit)
            .into_iter()
            .map(|m| {
                let candidate = &candidates[m.index];
                let (path, file) = files[candidate.file as usize];
                let symbol = &file.symbols()[candidate.symbol as usize];
                FoundSymbol {
                    name: candidate.name.to_owned(),
                    kind: symbol.kind,
                    container: file.parent(symbol).map(|p| file.name(p).to_owned()),
                    relative: path.clone(),
                    line: symbol.line,
                    column: symbol.column,
                    positions: m.positions,
                }
            })
            .collect()
    }

    /// Brings every file in line with `files`, the whole project as the file index lists it:
    /// files no longer listed are dropped, and those new or changed on disk since they were read
    /// are read again. For when the index is built or notifications were lost.
    pub fn sync(
        &mut self,
        files: Arc<Vec<String>>,
        resolve: &Resolve,
        cancel: &CancelToken,
    ) -> Result<(), Cancelled> {
        self.files = files;
        let listed: HashSet<&str> = self.files.iter().map(String::as_str).collect();
        let gone: Vec<Box<str>> = self
            .by_file
            .keys()
            .filter(|path| !listed.contains(&***path))
            .cloned()
            .collect();
        for path in gone {
            self.remove(&path);
        }
        let jobs = self
            .files
            .iter()
            .filter_map(|path| Some(self.job(path, resolve(path)?, false)))
            .collect();
        self.run(jobs, cancel)
    }

    /// Reads again what is at `changed` (absolute paths of files, or of folders and the files
    /// listed in them), once the file index has applied those changes: `files` is its new list.
    pub fn refresh<'a>(
        &mut self,
        files: Arc<Vec<String>>,
        changed: impl ExactSizeIterator<Item = &'a Path>,
        resolve: &Resolve,
        cancel: &CancelToken,
    ) -> Result<(), Cancelled> {
        if changed.len() > MAX_CHANGED {
            return self.sync(files, resolve, cancel);
        }
        self.files = files;
        let mut targets = HashSet::new();
        // Lowercase relative paths that are not listed: deleted (or renamed) files and folders.
        let mut gone = HashSet::new();
        for path in changed {
            let Some(relative) = self.relative(path) else {
                continue;
            };
            if let Some(file) = listed(&self.files, &relative) {
                targets.insert(file.clone());
            } else {
                targets.extend(listed_inside(&self.files, &relative).iter().cloned());
                gone.insert(relative.to_lowercase());
            }
        }
        if !gone.is_empty() {
            for path in self.by_file.keys() {
                let lower = path.to_lowercase();
                // The file itself, or one of the folders it is in.
                let hit = gone.contains(&lower)
                    || lower
                        .match_indices('\\')
                        .any(|(i, _)| gone.contains(&lower[..i]));
                if hit {
                    targets.insert(path.to_string());
                }
            }
        }
        let mut jobs = Vec::new();
        for path in targets {
            match resolve(&path).filter(|_| listed(&self.files, &path).is_some()) {
                Some(source) => jobs.push(self.job(&path, source, true)),
                None => self.remove(&path),
            }
        }
        self.run(jobs, cancel)
    }

    /// Sets the unsaved text of the open document at `path` (absolute), or with `None` clears it,
    /// and reads the file's symbols from that text (or from disk again).
    pub fn set_unsaved(
        &mut self,
        path: &Path,
        text: Option<String>,
        resolve: &Resolve,
        cancel: &CancelToken,
    ) -> Result<(), Cancelled> {
        let Some(relative) = self.relative(path) else {
            return Ok(());
        };
        let listed = listed(&self.files, &relative).cloned();
        let key = listed.clone().unwrap_or(relative);
        match text {
            Some(text) => self.unsaved.insert(key.clone(), text),
            None => self.unsaved.remove(&key),
        };
        // A file not listed yet is read once the file index lists it.
        match (listed, resolve(&key)) {
            (Some(_), Some(source)) => {
                let job = self.job(&key, source, true);
                self.run(vec![job], cancel)
            }
            _ => Ok(()),
        }
    }

    fn relative(&self, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(&self.root).ok()?.to_str()?;
        (!relative.is_empty()).then(|| relative.to_owned())
    }

    /// `force`: read the file even if its stamp did not change.
    fn job(&self, relative: &str, source: SymbolSource, force: bool) -> Job {
        let unsaved = self.unsaved.get(relative).cloned();
        let known = match force || unsaved.is_some() {
            true => None,
            false => self.by_file.get(relative).and_then(|f| f.stamp),
        };
        Job {
            relative: relative.to_owned(),
            source,
            unsaved,
            known,
        }
    }

    fn run(&mut self, jobs: Vec<Job>, cancel: &CancelToken) -> Result<(), Cancelled> {
        let outcomes = read_all(&self.root, &jobs, cancel)?;
        for (job, outcome) in jobs.into_iter().zip(outcomes) {
            match outcome {
                Outcome::Unchanged => {}
                Outcome::Gone => self.remove(&job.relative),
                Outcome::Read(file) => {
                    self.symbol_count += file.symbols().len();
                    if let Some(old) = self.by_file.insert(job.relative.into(), file) {
                        self.symbol_count -= old.symbols().len();
                    }
                }
            }
        }
        Ok(())
    }

    fn remove(&mut self, relative: &str) {
        if let Some(old) = self.by_file.remove(relative) {
            self.symbol_count -= old.symbols().len();
        }
    }
}

/// The listed file at `relative`, written as it is listed (Windows paths ignore case).
fn listed<'a>(files: &'a [String], relative: &str) -> Option<&'a String> {
    let lower = relative.to_lowercase();
    let i = files
        .binary_search_by(|f| f.to_lowercase().cmp(&lower))
        .ok()?;
    Some(&files[i])
}

/// The listed files inside the folder `relative`: one block of the sorted list.
fn listed_inside<'a>(files: &'a [String], relative: &str) -> &'a [String] {
    let folder = format!("{}\\", relative.to_lowercase());
    let start = files.partition_point(|f| f.to_lowercase() < folder);
    let len = files[start..].partition_point(|f| f.to_lowercase().starts_with(&folder));
    &files[start..start + len]
}

/// A file to read.
struct Job {
    relative: String,
    source: SymbolSource,
    /// Read this instead of the file.
    unsaved: Option<String>,
    /// The file is left as it was if it still has this stamp.
    known: Option<DiskStamp>,
}

enum Outcome {
    Unchanged,
    /// Missing or unreadable.
    Gone,
    Read(FileSymbols),
}

/// Runs the jobs on all cores but one (the interface keeps one), up to [`MAX_THREADS`], each
/// thread taking the next job as it finishes the previous one. The outcomes are in the order of the jobs.
fn read_all(root: &Path, jobs: &[Job], cancel: &CancelToken) -> Result<Vec<Outcome>, Cancelled> {
    let threads = thread::available_parallelism()
        .map_or(1, NonZero::get)
        .saturating_sub(1)
        .clamp(1, MAX_THREADS.min(jobs.len()).max(1));
    if threads == 1 {
        let mut parsers = Parsers::new();
        let mut outcomes = Vec::with_capacity(jobs.len());
        for job in jobs {
            if cancel.is_cancelled() {
                return Err(Cancelled);
            }
            outcomes.push(read(root, job, &mut parsers));
        }
        return Ok(outcomes);
    }
    let next = AtomicUsize::new(0);
    let done: Vec<Vec<(usize, Outcome)>> = thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut parsers = Parsers::new();
                    let mut mine = Vec::new();
                    while !cancel.is_cancelled() {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(job) = jobs.get(i) else { break };
                        mine.push((i, read(root, job, &mut parsers)));
                    }
                    mine
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("a symbol thread finishes"))
            .collect()
    });
    if cancel.is_cancelled() {
        return Err(Cancelled);
    }
    let mut outcomes: Vec<Option<Outcome>> = (0..jobs.len()).map(|_| None).collect();
    for (i, outcome) in done.into_iter().flatten() {
        outcomes[i] = Some(outcome);
    }
    Ok(outcomes
        .into_iter()
        .map(|o| o.expect("every job ran"))
        .collect())
}

/// One parser per language (`None`: it has no symbols), made the first time a thread needs it.
type Parsers = HashMap<Arc<str>, Option<SymbolParser>>;

fn read(root: &Path, job: &Job, parsers: &mut Parsers) -> Outcome {
    let (text, stamp): (Cow<str>, _) = match &job.unsaved {
        Some(text) => (Cow::Borrowed(text), None),
        None => {
            let path = root.join(&job.relative);
            let Ok(metadata) = fs::metadata(&path) else {
                return Outcome::Gone;
            };
            let Ok(stamp) = DiskStamp::from_metadata(&metadata) else {
                return Outcome::Gone;
            };
            if job.known == Some(stamp) {
                return Outcome::Unchanged;
            }
            if metadata.len() > MAX_FILE_BYTES {
                return Outcome::Read(FileSymbols {
                    stamp: Some(stamp),
                    ..FileSymbols::default()
                });
            }
            let Ok(bytes) = fs::read(&path) else {
                return Outcome::Gone;
            };
            (Cow::Owned(encoding::decode(bytes).text), Some(stamp))
        }
    };
    if text.len() as u64 > MAX_FILE_BYTES {
        return Outcome::Read(FileSymbols {
            stamp,
            ..FileSymbols::default()
        });
    }
    let source = &job.source;
    let parser = parsers
        .entry(source.language.clone())
        .or_insert_with(|| SymbolParser::new(&source.language, source.rules.clone()));
    let found = parser
        .as_mut()
        .map(|p| p.extract(&text))
        .unwrap_or_default();
    Outcome::Read(FileSymbols::new(&text, &found, stamp))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{WalkOptions, walk};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bracket-project-symbols-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn csharp(relative: &str) -> Option<SymbolSource> {
        relative.ends_with(".cs").then(|| SymbolSource {
            language: "csharp".into(),
            rules: None,
        })
    }

    fn files(root: &Path) -> Arc<Vec<String>> {
        let cancel = CancelToken::new();
        let index = walk(root, &WalkOptions::default(), &cancel)
            .unwrap()
            .unwrap();
        index.files
    }

    /// The names in a file, each with its parent's name.
    fn names(symbols: &ProjectSymbols, relative: &str) -> Vec<String> {
        let Some(file) = symbols.get(relative) else {
            return Vec::new();
        };
        file.symbols()
            .iter()
            .map(|s| match file.parent(s) {
                Some(parent) => format!("{}.{}", file.name(parent), file.name(s)),
                None => file.name(s).to_owned(),
            })
            .collect()
    }

    #[test]
    fn every_file_with_a_source_is_read_with_lines_and_parents() {
        let root = temp_dir("sync");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join(r"src\Invoice.cs"),
            "class Invoice\n{\n    void Pay() { }\n}\n",
        )
        .unwrap();
        fs::write(root.join("README.md"), "# class NotCode").unwrap();
        let big = "class Big { }\n".repeat(MAX_FILE_BYTES as usize / 10);
        fs::write(root.join("Big.cs"), big).unwrap();

        let mut symbols = ProjectSymbols::new(root.clone());
        symbols
            .sync(files(&root), &csharp, &CancelToken::new())
            .unwrap();
        assert_eq!(
            names(&symbols, r"src\Invoice.cs"),
            ["Invoice", "Invoice.Pay"]
        );
        let file = symbols.get(r"src\Invoice.cs").unwrap();
        let pay = file.symbols()[1];
        assert_eq!((pay.line, pay.column), (2, 9));
        assert_eq!(pay.kind, SymbolKind::Method);
        assert!(symbols.get("README.md").is_none(), "no source, not read");
        assert_eq!(names(&symbols, "Big.cs"), Vec::<String>::new(), "too large");
        assert_eq!((symbols.file_count(), symbols.symbol_count()), (2, 2));
        assert!(symbols.heap_bytes() > 0);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn changes_read_again_only_what_they_touch() {
        let root = temp_dir("refresh");
        fs::create_dir_all(root.join(r"src\Models")).unwrap();
        fs::write(root.join(r"src\Models\User.cs"), "class User { }").unwrap();
        fs::write(root.join(r"src\Models\Role.cs"), "class Role { }").unwrap();
        fs::write(root.join("Program.cs"), "class Program { }").unwrap();
        let cancel = CancelToken::new();
        let mut symbols = ProjectSymbols::new(root.clone());
        symbols.sync(files(&root), &csharp, &cancel).unwrap();
        assert_eq!(symbols.symbol_count(), 3);

        // A content change.
        fs::write(root.join("Program.cs"), "class Program { void Main() { } }").unwrap();
        let changed = [root.join("Program.cs")];
        symbols
            .refresh(
                files(&root),
                changed.iter().map(PathBuf::as_path),
                &csharp,
                &cancel,
            )
            .unwrap();
        assert_eq!(names(&symbols, "Program.cs"), ["Program", "Program.Main"]);

        // A folder renamed: its files leave under the old name and come back under the new one.
        fs::rename(root.join(r"src\Models"), root.join(r"src\Domain")).unwrap();
        let changed = [root.join(r"src\Models"), root.join(r"src\Domain")];
        symbols
            .refresh(
                files(&root),
                changed.iter().map(PathBuf::as_path),
                &csharp,
                &cancel,
            )
            .unwrap();
        assert!(symbols.get(r"src\Models\User.cs").is_none());
        assert_eq!(names(&symbols, r"src\Domain\User.cs"), ["User"]);
        assert_eq!(symbols.symbol_count(), 4);

        // A file deleted.
        fs::remove_file(root.join(r"src\Domain\Role.cs")).unwrap();
        let changed = [root.join(r"src\Domain\Role.cs")];
        symbols
            .refresh(
                files(&root),
                changed.iter().map(PathBuf::as_path),
                &csharp,
                &cancel,
            )
            .unwrap();
        assert!(symbols.get(r"src\Domain\Role.cs").is_none());
        assert_eq!((symbols.file_count(), symbols.symbol_count()), (2, 3));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn unsaved_text_wins_until_it_is_cleared() {
        let root = temp_dir("unsaved");
        fs::write(root.join("Order.cs"), "class Order { }").unwrap();
        let cancel = CancelToken::new();
        let mut symbols = ProjectSymbols::new(root.clone());
        symbols.sync(files(&root), &csharp, &cancel).unwrap();

        let path = root.join("Order.cs");
        let text = "class Order { int Total; }".to_owned();
        symbols
            .set_unsaved(&path, Some(text), &csharp, &cancel)
            .unwrap();
        assert_eq!(names(&symbols, "Order.cs"), ["Order", "Order.Total"]);
        // A full check keeps the unsaved text.
        symbols.sync(files(&root), &csharp, &cancel).unwrap();
        assert_eq!(names(&symbols, "Order.cs"), ["Order", "Order.Total"]);

        symbols.set_unsaved(&path, None, &csharp, &cancel).unwrap();
        assert_eq!(names(&symbols, "Order.cs"), ["Order"]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn files_that_did_not_change_are_not_read_again() {
        let root = temp_dir("stamp");
        fs::write(root.join("A.cs"), "class A { }").unwrap();
        let stamp = DiskStamp::of(&root.join("A.cs")).ok();
        let job = |known| Job {
            relative: "A.cs".to_owned(),
            source: csharp("A.cs").unwrap(),
            unsaved: None,
            known,
        };
        let mut parsers = Parsers::new();
        let mut read = |job| read(&root, &job, &mut parsers);
        assert!(matches!(read(job(stamp)), Outcome::Unchanged));
        assert!(matches!(read(job(None)), Outcome::Read(_)));
        fs::remove_file(root.join("A.cs")).unwrap();
        assert!(matches!(read(job(None)), Outcome::Gone));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn symbols_are_searched_across_files() {
        let root = temp_dir("search");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join(r"src\Invoice.cs"),
            "class Invoice\n{\n  void ComputeTotal() { }\n}\n",
        )
        .unwrap();
        fs::write(root.join(r"src\Order.cs"), "class Order { int Total; }").unwrap();
        let mut symbols = ProjectSymbols::new(root.clone());
        symbols
            .sync(files(&root), &csharp, &CancelToken::new())
            .unwrap();

        let found = symbols.search("total", 10);
        let names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Total", "ComputeTotal"], "the whole name first");
        assert_eq!(found[0].relative, r"src\Order.cs");
        assert_eq!(found[0].container.as_deref(), Some("Order"));
        assert_eq!(found[0].kind, SymbolKind::Field);
        let compute = &found[1];
        assert_eq!((compute.line, compute.column), (2, 7));
        assert_eq!(compute.positions, [7, 8, 9, 10, 11]);
        assert!(symbols.search("", 10).is_empty());
        assert_eq!(symbols.search("total", 1).len(), 1, "the limit holds");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_cancelled_sync_stops() {
        let root = temp_dir("cancel");
        fs::write(root.join("A.cs"), "class A { }").unwrap();
        let cancel = CancelToken::new();
        cancel.cancel();
        let mut symbols = ProjectSymbols::new(root.clone());
        assert_eq!(symbols.sync(files(&root), &csharp, &cancel), Err(Cancelled));
        fs::remove_dir_all(&root).unwrap();
    }
}
