//! The project's symbol index, on a thread of its own: the file index tells it which files the
//! project has and which changed, and open documents with unsaved changes lend it their text.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use bracket_core::{CancelToken, Document, Rope};
use bracket_index::project_symbols::{
    FoundSymbol, Lookup, MAX_FILE_BYTES, ProjectSymbols, Resolve, SymbolSource,
};
use bracket_index::rules::RuleSet;
use bracket_index::symbols;
use bracket_syntax::LanguageRegistry;
use eframe::egui;

use crate::navigation;

/// How long a document must stay unchanged before its unsaved text is indexed.
const UNSAVED_QUIET: Duration = Duration::from_secs(1);

/// A change to the file index's list, as it passes it on.
pub struct FilesChanged {
    /// The whole list: relative paths, sorted by their lowercase form.
    pub files: Arc<Vec<String>>,
    /// Absolute paths created, deleted, renamed or modified.
    pub changed: Vec<PathBuf>,
    /// Every file must be checked: the index was built, its rules changed or notifications were
    /// lost.
    pub full: bool,
}

pub enum SymbolCommand {
    Files(FilesChanged),
    /// The unsaved text of an open document, or `None` to go back to the file on disk.
    Unsaved {
        path: PathBuf,
        text: Option<Rope>,
    },
    Query {
        text: String,
        generation: u64,
    },
    /// Where `name` may be defined, seen from the file `from` in `language`; `component`: a
    /// Razor component.
    Definitions {
        name: String,
        from: Option<PathBuf>,
        language: Option<String>,
        component: bool,
        generation: u64,
    },
}

pub enum SymbolEvent {
    /// The index is up to date and holds this many symbols.
    Ready(usize),
    Results {
        generation: u64,
        found: Vec<FoundSymbol>,
    },
    Definitions {
        generation: u64,
        found: Vec<FoundSymbol>,
    },
}

/// Results kept for one query.
const LIMIT: usize = 200;

/// The thread that owns the project's symbol index.
pub struct SymbolWorker {
    commands: Sender<SymbolCommand>,
    events: Receiver<SymbolEvent>,
    cancel: CancelToken,
}

/// Which files have symbols, and how to get them, by their path.
pub type BoxedResolve = Box<dyn Fn(&str) -> Option<SymbolSource> + Send + Sync>;

impl SymbolWorker {
    /// Waits for the file index's list. `notify` is called whenever an event is ready.
    pub fn start(root: PathBuf, resolve: BoxedResolve, notify: impl Fn() + Send + 'static) -> Self {
        let (commands, command_rx) = mpsc::channel::<SymbolCommand>();
        let (event_tx, events) = mpsc::channel();
        let cancel = CancelToken::new();
        let worker_cancel = cancel.clone();
        thread::Builder::new()
            .name("symbol-index".to_owned())
            .spawn(move || {
                let resolve: &Resolve = &*resolve;
                let mut index = ProjectSymbols::new(root);
                while let Ok(first) = command_rx.recv() {
                    // Everything waiting is taken at once: the newest list, every changed path,
                    // and the newest text of each document.
                    let mut files = None;
                    let mut full = false;
                    let mut changed = HashSet::new();
                    let mut unsaved = HashMap::new();
                    let mut query = None;
                    let mut definitions = None;
                    for command in std::iter::once(first).chain(command_rx.try_iter()) {
                        match command {
                            SymbolCommand::Files(change) => {
                                files = Some(change.files);
                                full |= change.full;
                                changed.extend(change.changed);
                            }
                            SymbolCommand::Unsaved { path, text } => {
                                unsaved.insert(path, text);
                            }
                            SymbolCommand::Query { text, generation } => {
                                query = Some((text, generation));
                            }
                            SymbolCommand::Definitions {
                                name,
                                from,
                                language,
                                component,
                                generation,
                            } => definitions = Some((name, from, language, component, generation)),
                        }
                    }
                    let updated = files.is_some() || !unsaved.is_empty();
                    let mut result = match files {
                        Some(files) if full => index.sync(files, resolve, &worker_cancel),
                        Some(files) => index.refresh(
                            files,
                            changed.iter().map(PathBuf::as_path),
                            resolve,
                            &worker_cancel,
                        ),
                        None => Ok(()),
                    };
                    for (path, text) in unsaved {
                        let text = text.map(|t| t.to_string());
                        result = result
                            .and_then(|()| index.set_unsaved(&path, text, resolve, &worker_cancel));
                    }
                    if result.is_err() {
                        return;
                    }
                    let mut events = Vec::new();
                    if updated {
                        events.push(SymbolEvent::Ready(index.symbol_count()));
                    }
                    if let Some((text, generation)) = query {
                        let found = index.search(&text, LIMIT);
                        events.push(SymbolEvent::Results { generation, found });
                    }
                    if let Some((name, from, language, component, generation)) = definitions {
                        let from = from.as_deref().and_then(|f| index.listed_path(f));
                        // Only files of related languages: never a JSON key for a C# name.
                        let files = |path: &str| {
                            let source = resolve(path);
                            language.as_deref().zip(source).is_some_and(
                                |(from_language, source)| {
                                    navigation::looks_in(
                                        from_language,
                                        &source.language,
                                        Some(path) == from,
                                    )
                                },
                            )
                        };
                        let lookup = Lookup {
                            name: &name,
                            from,
                            component,
                            match_case: navigation::references_match_case(language.as_deref()),
                            files: &files,
                        };
                        let found = index.definitions(&lookup, LIMIT);
                        events.push(SymbolEvent::Definitions { generation, found });
                    }
                    for event in events {
                        if event_tx.send(event).is_err() {
                            return;
                        }
                        notify();
                    }
                }
            })
            .expect("the symbol index thread starts");
        Self {
            commands,
            events,
            cancel,
        }
    }

    /// Where the file index sends its changes.
    pub fn sender(&self) -> Sender<SymbolCommand> {
        self.commands.clone()
    }

    pub fn query(&self, text: String, generation: u64) {
        let _ = self
            .commands
            .send(SymbolCommand::Query { text, generation });
    }

    pub fn definitions(
        &self,
        name: String,
        from: Option<PathBuf>,
        language: Option<String>,
        component: bool,
        generation: u64,
    ) {
        let _ = self.commands.send(SymbolCommand::Definitions {
            name,
            from,
            language,
            component,
            generation,
        });
    }

    pub fn events(&self) -> Vec<SymbolEvent> {
        self.events.try_iter().collect()
    }
}

impl Drop for SymbolWorker {
    /// Reading files stops.
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Files have symbols when their language has a built-in extractor or a package's rules.
pub fn resolver(
    languages: &LanguageRegistry,
    rules: &HashMap<String, Arc<RuleSet>>,
) -> BoxedResolve {
    // One source per language, shared by all its files.
    let sources: HashMap<String, SymbolSource> = languages
        .languages()
        .iter()
        .filter_map(|language| {
            let rules = rules.get(&language.id).cloned();
            (rules.is_some() || symbols::has_symbols(&language.id)).then(|| {
                let source = SymbolSource {
                    language: language.id.as_str().into(),
                    rules,
                };
                (language.id.clone(), source)
            })
        })
        .collect();
    let languages = languages.clone();
    Box::new(move |relative| {
        let language = languages.for_path(Path::new(relative))?;
        sources.get(&language.id).cloned()
    })
}

/// Lends the symbol index the text of documents with unsaved changes, once typing stops, and
/// takes it back when they are saved, reverted or closed.
#[derive(Default)]
pub struct UnsavedTexts {
    /// The version of each document whose text was sent.
    sent: HashMap<PathBuf, u64>,
    /// Documents changed since: their version, and since when it has not changed.
    waiting: HashMap<PathBuf, (u64, Instant)>,
}

impl UnsavedTexts {
    /// Call every frame with the loaded documents that have a file.
    pub fn update<'a>(
        &mut self,
        ctx: &egui::Context,
        worker: &SymbolWorker,
        documents: impl Iterator<Item = (&'a Path, &'a Document)>,
    ) {
        let mut dirty = HashSet::new();
        for (path, doc) in documents {
            // Too large for the index: its file on disk is used.
            if !doc.is_dirty() || doc.buffer.text().len_bytes() as u64 > MAX_FILE_BYTES {
                continue;
            }
            dirty.insert(path.to_path_buf());
            let version = doc.buffer.version();
            if self.sent.get(path) == Some(&version) {
                self.waiting.remove(path);
                continue;
            }
            let since = match self.waiting.get(path) {
                Some((v, since)) if *v == version => *since,
                _ => {
                    let now = Instant::now();
                    self.waiting.insert(path.to_path_buf(), (version, now));
                    now
                }
            };
            let waited = since.elapsed();
            if waited < UNSAVED_QUIET {
                ctx.request_repaint_after(UNSAVED_QUIET - waited);
                continue;
            }
            let _ = worker.commands.send(SymbolCommand::Unsaved {
                path: path.to_path_buf(),
                text: Some(doc.buffer.text().clone()),
            });
            self.sent.insert(path.to_path_buf(), version);
            self.waiting.remove(path);
        }
        self.waiting.retain(|path, _| dirty.contains(path));
        let clean: Vec<PathBuf> = self
            .sent
            .keys()
            .filter(|path| !dirty.contains(*path))
            .cloned()
            .collect();
        for path in clean {
            self.sent.remove(&path);
            let _ = worker
                .commands
                .send(SymbolCommand::Unsaved { path, text: None });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn wait_for(worker: &SymbolWorker, symbols: usize) {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(10) {
            if worker
                .events()
                .iter()
                .any(|e| matches!(e, SymbolEvent::Ready(n) if *n == symbols))
            {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("the index never held {symbols} symbols");
    }

    #[test]
    fn the_index_follows_file_changes_and_unsaved_text() {
        let root =
            std::env::temp_dir().join(format!("bracket-symbol-index-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("A.cs"), "class A { }").unwrap();
        fs::write(root.join("notes.md"), "class Nope").unwrap();
        let resolve = resolver(&LanguageRegistry::builtin(), &HashMap::new());
        let worker = SymbolWorker::start(root.clone(), resolve, || {});
        let files = |names: &[&str]| Arc::new(names.iter().map(|n| n.to_string()).collect());

        let send = worker.sender();
        send.send(SymbolCommand::Files(FilesChanged {
            files: files(&["A.cs", "notes.md"]),
            changed: Vec::new(),
            full: true,
        }))
        .unwrap();
        wait_for(&worker, 1);

        fs::write(root.join("B.cs"), "class B { void M() { } }").unwrap();
        send.send(SymbolCommand::Files(FilesChanged {
            files: files(&["A.cs", "B.cs", "notes.md"]),
            changed: vec![root.join("B.cs")],
            full: false,
        }))
        .unwrap();
        wait_for(&worker, 3);

        send.send(SymbolCommand::Unsaved {
            path: root.join("A.cs"),
            text: Some(Rope::from_str("class A { int X; int Y; }")),
        })
        .unwrap();
        wait_for(&worker, 5);
        send.send(SymbolCommand::Unsaved {
            path: root.join("A.cs"),
            text: None,
        })
        .unwrap();
        wait_for(&worker, 3);

        worker.query("m".to_owned(), 7);
        let started = Instant::now();
        let found = loop {
            assert!(started.elapsed() < Duration::from_secs(10), "no results");
            let results = worker.events().into_iter().find_map(|e| match e {
                SymbolEvent::Results { generation, found } => Some((generation, found)),
                _ => None,
            });
            if let Some(results) = results {
                break results;
            }
            thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(found.0, 7);
        assert_eq!(found.1[0].name, "M");
        assert_eq!(found.1[0].relative, "B.cs");

        let csharp = Some("csharp".to_owned());
        worker.definitions(
            "B".to_owned(),
            Some(root.join("A.cs")),
            csharp.clone(),
            false,
            8,
        );
        let started = Instant::now();
        let found = loop {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "no definitions"
            );
            let found = worker.events().into_iter().find_map(|e| match e {
                SymbolEvent::Definitions {
                    generation: 8,
                    found,
                } => Some(found),
                _ => None,
            });
            if let Some(found) = found {
                break found;
            }
            thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].relative.as_str(), found[0].line), ("B.cs", 0));
        drop(worker);
        fs::remove_dir_all(&root).unwrap();
    }
}
