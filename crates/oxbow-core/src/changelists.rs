//! Changelists: named groups of the unstaged changes, as in JetBrains IDEs, kept per working
//! copy in `.git/oxbow/changelists.json`. Staged changes stay the next commit; a changelist splits
//! what is left out of it. A list holds whole files, and single runs of a file's changed lines
//! (the changed lines between two unchanged ones), so one file can be shared between lists.
//!
//! A run is named by its text, not its place, so it keeps its list while the file is edited
//! elsewhere and whatever context the diff shows. Committing a list builds its commit in a
//! staging area of its own (`GIT_INDEX_FILE`), as `git commit --only` does, so the staged files
//! of other lists stay staged.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::cli::{GitCommand, literal};
use crate::commit::{DiffLine, FileChange, FileStatus, LineKind};
use crate::error::{Error, Result};
use crate::repo::Repo;
use crate::worktree::{WorkingTree, fingerprint, partial_hunk};

/// Where the lists are kept, in each working copy's own git folder.
const STORE: &str = "oxbow/changelists.json";
/// The staging area a list's commit is built in.
pub(crate) const INDEX: &str = "oxbow/changelist-index";
/// The one the secrets check builds it in first, to look at it before anything runs.
const CHECK_INDEX: &str = "oxbow/changelist-check-index";
/// Names that would read as the lists of the panel itself.
const TAKEN: [&str; 4] = ["unstaged", "staged", "untracked", "conflicted"];

/// One reader or writer of the lists at a time, in this process.
static LOCK: Mutex<()> = Mutex::new(());

/// A run of changed lines in a hunk: its key, and its lines as indexes into the hunk's lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Run {
    pub key: String,
    pub lines: Vec<usize>,
    pub additions: u32,
    pub deletions: u32,
}

/// The runs of changed lines in a hunk's `lines`. A run's key is the fingerprint of its lines;
/// the same text again in the file (`seen` counts it across the file's hunks) gets `#2`, `#3`.
pub(crate) fn runs_of(lines: &[DiffLine], seen: &mut HashMap<String, usize>) -> Vec<Run> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        if lines[at].kind == LineKind::Context {
            at += 1;
            continue;
        }
        let end = (at..lines.len())
            .find(|&i| lines[i].kind == LineKind::Context)
            .unwrap_or(lines.len());
        let mut text = String::new();
        let (mut additions, mut deletions) = (0, 0);
        for line in &lines[at..end] {
            if line.kind == LineKind::Added {
                additions += 1;
                text.push('+');
            } else {
                deletions += 1;
                text.push('-');
            }
            text.push_str(&line.text);
            if line.cr {
                text.push('\r');
            }
            text.push('\n');
        }
        let print = fingerprint(&text);
        let n = seen.entry(print.clone()).or_insert(0);
        *n += 1;
        let key = if *n == 1 { print } else { format!("{print}#{n}") };
        out.push(Run {
            key,
            lines: (at..end).collect(),
            additions,
            deletions,
        });
        at = end;
    }
    out
}

/// The lists as kept on disk.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Store {
    #[serde(default)]
    lists: Vec<StoredList>,
    /// Runs kept in another list than their file's, `None` for Unstaged.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    runs: Vec<StoredRun>,
    /// The list files that start changing go to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active: Option<String>,
    /// The files that had changes when the lists were last read with a list active: a file not
    /// among them is new to the active list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    seen: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct StoredList {
    id: String,
    name: String,
    /// Files whose changes are in this list, apart from runs kept elsewhere.
    #[serde(default)]
    files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct StoredRun {
    path: String,
    key: String,
    #[serde(default)]
    list: Option<String>,
}

impl Store {
    fn has(&self, id: &str) -> bool {
        self.lists.iter().any(|l| l.id == id)
    }

    fn list_mut(&mut self, id: &str) -> Result<&mut StoredList> {
        self.lists.iter_mut().find(|l| l.id == id).ok_or_else(gone)
    }

    /// The list a file is in, apart from its runs kept elsewhere; `None` for Unstaged.
    fn owner(&self, path: &str) -> Option<&str> {
        self.lists
            .iter()
            .find(|l| l.files.iter().any(|f| f == path))
            .map(|l| l.id.as_str())
    }

    fn keyed(&self, path: &str) -> bool {
        self.runs.iter().any(|r| r.path == path)
    }

    /// The list a run of `path` is in.
    fn run_list<'a>(&'a self, path: &str, key: &str) -> Option<&'a str> {
        match self.runs.iter().find(|r| r.path == path && r.key == key) {
            Some(run) => run.list.as_deref(),
            None => self.owner(path),
        }
    }

    fn set_owner(&mut self, path: &str, to: Option<&str>) {
        for list in &mut self.lists {
            list.files.retain(|f| f != path);
        }
        if let Some(list) = to.and_then(|id| self.lists.iter_mut().find(|l| l.id == id)) {
            list.files.push(path.to_owned());
        }
    }

    fn set_run(&mut self, path: &str, key: &str, to: Option<&str>) {
        match self.runs.iter_mut().find(|r| r.path == path && r.key == key) {
            Some(run) => run.list = to.map(str::to_owned),
            None => self.runs.push(StoredRun {
                path: path.to_owned(),
                key: key.to_owned(),
                list: to.map(str::to_owned),
            }),
        }
    }
}

fn gone() -> Error {
    Error::Git("That changelist is gone: another window may have deleted it".into())
}

/// The lists and what is in each, as the Changes panel shows them.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Changelists {
    pub lists: Vec<Changelist>,
    /// The list files that start changing go to; `None`: they go to Unstaged.
    pub active: Option<String>,
    /// Files whose unstaged changes are in more than one list, Unstaged counting as one.
    pub split: Vec<SplitFile>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Changelist {
    pub id: String,
    pub name: String,
    /// The unstaged files with changes in it, in the order of the working tree.
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SplitFile {
    pub path: String,
    pub parts: Vec<FilePart>,
}

/// What one list (`None`: Unstaged) holds of a split file: its runs and their lines.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FilePart {
    pub list: Option<String>,
    pub runs: Vec<String>,
    pub additions: u32,
    pub deletions: u32,
}

/// What moves to another list.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Moving {
    /// What the list `from` (`None`: Unstaged) holds of each file.
    Part { paths: Vec<String>, from: Option<String> },
    /// The whole of each file, whatever list its parts are in.
    Files { paths: Vec<String> },
    /// Single runs of one file.
    Runs { path: String, keys: Vec<String> },
}

/// A change to the lists. None of them touches the files or git.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ChangelistEdit {
    /// A new list, at the end, with `moving` in it.
    Create {
        name: String,
        #[serde(default)]
        moving: Option<Moving>,
    },
    Rename {
        id: String,
        name: String,
    },
    /// Its changes go back to Unstaged.
    Delete {
        id: String,
    },
    Move {
        moving: Moving,
        to: Option<String>,
    },
    /// Files that start changing go to this list from now on; `None`: to Unstaged.
    Activate {
        id: Option<String>,
    },
}

/// The part of the unstaged changes one list holds, for staging, discarding or committing it.
#[derive(Debug, Default)]
pub(crate) struct ListPart {
    /// Files it holds whole.
    pub whole: Vec<FileChange>,
    /// Files it holds some runs of, and the patch of those runs.
    pub partial: Vec<String>,
    pub patch: String,
}

impl ListPart {
    pub fn is_empty(&self) -> bool {
        self.whole.is_empty() && self.partial.is_empty()
    }

    pub fn paths(&self) -> Vec<String> {
        self.whole
            .iter()
            .map(|f| f.path.clone())
            .chain(self.partial.iter().cloned())
            .collect()
    }
}

/// The unstaged files a list can hold: conflicts are resolved first.
fn listable(tree: &WorkingTree) -> impl Iterator<Item = &FileChange> {
    tree.unstaged.iter().filter(|f| f.status != FileStatus::Conflicted)
}

/// The runs of each hunk of a file, with each hunk's text; nothing for a file taken whole.
type FileRuns = (String, Vec<(String, Vec<Run>)>);

impl Repo {
    fn store_file(&self) -> Result<std::path::PathBuf> {
        self.git_path(STORE)
    }

    fn read_store(&self) -> Result<Store> {
        let file = self.store_file()?;
        match std::fs::read_to_string(&file) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|err| Error::Git(format!("The changelists can't be read from {}: {err}", file.display()))),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
            Err(err) => Err(Error::Git(format!("The changelists can't be read: {err}"))),
        }
    }

    /// Written whole to a new file that then takes the old one's place; no lists, no file.
    fn write_store(&self, store: &Store) -> Result<()> {
        let io = |err: std::io::Error| Error::Git(format!("The changelists can't be saved: {err}"));
        let file = self.store_file()?;
        if store.lists.is_empty() && store.runs.is_empty() {
            return match std::fs::remove_file(&file) {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(io(err)),
                _ => Ok(()),
            };
        }
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let text = serde_json::to_string_pretty(store).map_err(|err| Error::Git(err.to_string()))?;
        let new = file.with_extension(format!("json.{}", std::process::id()));
        std::fs::write(&new, text).map_err(io)?;
        std::fs::rename(&new, &file).map_err(io)
    }

    /// The runs of `file`'s unstaged hunks. A file that can't be split (new, binary, a
    /// submodule, kept in LFS, too big to show) has none: it goes whole.
    fn file_runs(&self, file: &FileChange) -> FileRuns {
        if file.status == FileStatus::Untracked || file.binary || file.submodule.is_some() || file.lfs.is_some() {
            return (String::new(), Vec::new());
        }
        let Ok((header, hunks)) = self.unstaged_hunks(file) else {
            return (String::new(), Vec::new());
        };
        let mut seen = HashMap::new();
        let hunks = hunks
            .into_iter()
            .map(|(text, hunk)| {
                let runs = runs_of(&hunk.lines, &mut seen);
                (text, runs)
            })
            .collect();
        (header, hunks)
    }

    /// Whether any list is kept: without, there is nothing to read the files for.
    pub fn has_changelists(&self) -> bool {
        self.store_file().is_ok_and(|file| file.exists())
    }

    /// The lists, with what each holds of `tree`, a reading of the working tree made a moment
    /// ago. With a list active, files that started changing since the last reading go to it.
    pub fn changelists(&self, tree: &WorkingTree) -> Result<Changelists> {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut store = self.read_store()?;
        if store.lists.is_empty() {
            return Ok(Changelists::default());
        }
        // Two steps, so both run: `|` and not `||`.
        if drop_unchanged(&mut store, tree) | take_new_files(&mut store, tree) {
            self.write_store(&store)?;
        }
        Ok(self.resolve(&store, tree))
    }

    /// Change the lists, then read them as [`Repo::changelists`] does.
    pub fn edit_changelists(&self, edit: &ChangelistEdit) -> Result<Changelists> {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tree = self.working_tree()?;
        let mut store = self.read_store()?;
        apply_edit(&mut store, edit, &tree, &|path| self.keys_of(&tree, path))?;
        self.normalize(&mut store, &tree);
        self.write_store(&store)?;
        Ok(self.resolve(&store, &tree))
    }

    /// The keys of the runs of `path`'s unstaged changes, in order.
    fn keys_of(&self, tree: &WorkingTree, path: &str) -> Vec<String> {
        listable(tree)
            .find(|f| f.path == path)
            .map(|file| {
                self.file_runs(file)
                    .1
                    .into_iter()
                    .flat_map(|(_, runs)| runs.into_iter().map(|r| r.key))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Drop what no longer has changes, runs kept where their file is anyway, and runs that
    /// are gone; a file whose runs are all in one list goes there whole.
    fn normalize(&self, store: &mut Store, tree: &WorkingTree) {
        let changed: HashSet<&str> = tree
            .staged
            .iter()
            .chain(&tree.unstaged)
            .chain(&tree.conflicted)
            .map(|f| f.path.as_str())
            .collect();
        let mut taken = HashSet::new();
        for list in &mut store.lists {
            list.files
                .retain(|f| changed.contains(f.as_str()) && taken.insert(f.clone()));
        }
        let ids: HashSet<String> = store.lists.iter().map(|l| l.id.clone()).collect();
        let unstaged: HashMap<&str, &FileChange> = listable(tree).map(|f| (f.path.as_str(), f)).collect();
        let mut runs: Vec<StoredRun> = Vec::new();
        let mut keyed: Vec<String> = Vec::new();
        for run in std::mem::take(&mut store.runs) {
            if !unstaged.contains_key(run.path.as_str())
                || run.list.as_ref().is_some_and(|id| !ids.contains(id))
                || runs.iter().any(|r| r.path == run.path && r.key == run.key)
            {
                continue;
            }
            if !keyed.contains(&run.path) {
                keyed.push(run.path.clone());
            }
            runs.push(run);
        }
        store.runs = runs;
        for path in keyed {
            let keys: Vec<String> = self
                .file_runs(unstaged[path.as_str()])
                .1
                .into_iter()
                .flat_map(|(_, runs)| runs.into_iter().map(|r| r.key))
                .collect();
            let owner = store.owner(&path).map(str::to_owned);
            store
                .runs
                .retain(|r| r.path != path || (keys.contains(&r.key) && r.list != owner));
            let lists: HashSet<Option<&str>> = keys.iter().map(|k| store.run_list(&path, k)).collect();
            if lists.len() == 1
                && let Some(only) = lists.into_iter().next().map(|l| l.map(str::to_owned))
            {
                store.set_owner(&path, only.as_deref());
                store.runs.retain(|r| r.path != path);
            }
        }
        if store.active.as_ref().is_some_and(|id| !store.has(id)) {
            store.active = None;
        }
        if store.active.is_none() {
            store.seen.clear();
        }
    }

    /// Which list holds what of each unstaged file.
    fn resolve(&self, store: &Store, tree: &WorkingTree) -> Changelists {
        let mut lists: Vec<Changelist> = store
            .lists
            .iter()
            .map(|l| Changelist {
                id: l.id.clone(),
                name: l.name.clone(),
                paths: Vec::new(),
            })
            .collect();
        let mut add = |list: Option<&str>, path: &str| {
            if let Some(entry) = list.and_then(|id| lists.iter_mut().find(|l| l.id == id)) {
                entry.paths.push(path.to_owned());
            }
        };
        let mut split = Vec::new();
        for file in listable(tree) {
            let owner = store.owner(&file.path);
            if !store.keyed(&file.path) {
                add(owner, &file.path);
                continue;
            }
            let mut parts: Vec<FilePart> = Vec::new();
            for run in self.file_runs(file).1.into_iter().flat_map(|(_, runs)| runs) {
                let list = store.run_list(&file.path, &run.key).map(str::to_owned);
                let part = match parts.iter_mut().position(|p| p.list == list) {
                    Some(at) => &mut parts[at],
                    None => {
                        parts.push(FilePart {
                            list,
                            ..FilePart::default()
                        });
                        parts.last_mut().expect("just pushed")
                    }
                };
                part.runs.push(run.key);
                part.additions += run.additions;
                part.deletions += run.deletions;
            }
            match parts.as_slice() {
                [] => add(owner, &file.path),
                [one] => add(one.list.as_deref(), &file.path),
                _ => {
                    for part in &parts {
                        add(part.list.as_deref(), &file.path);
                    }
                    split.push(SplitFile {
                        path: file.path.clone(),
                        parts,
                    });
                }
            }
        }
        Changelists {
            lists,
            active: store.active.clone(),
            split,
        }
    }

    /// What `list` (`None`: Unstaged) holds of the unstaged files, only of `only` when given,
    /// with the patch of its runs for `git apply` going `forward` or in reverse.
    pub(crate) fn list_part(&self, list: Option<&str>, only: Option<&[String]>, forward: bool) -> Result<ListPart> {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let store = self.read_store()?;
        if let Some(id) = list
            && !store.has(id)
        {
            return Err(gone());
        }
        let tree = self.working_tree()?;
        let mut part = ListPart::default();
        for file in listable(&tree) {
            if only.is_some_and(|only| !only.contains(&file.path)) {
                continue;
            }
            let owner = store.owner(&file.path);
            let (header, hunks) = if store.keyed(&file.path) {
                self.file_runs(file)
            } else {
                (String::new(), Vec::new())
            };
            if hunks.is_empty() {
                if owner == list {
                    part.whole.push(file.clone());
                }
                continue;
            }
            let mut patch = String::new();
            let (mut mine, mut all) = (0, 0);
            for (text, runs) in &hunks {
                let picked: Vec<usize> = runs
                    .iter()
                    .filter(|r| store.run_list(&file.path, &r.key) == list)
                    .flat_map(|r| r.lines.iter().copied())
                    .collect();
                all += runs.len();
                mine += runs
                    .iter()
                    .filter(|r| store.run_list(&file.path, &r.key) == list)
                    .count();
                if let Some(hunk) = partial_hunk(text, &picked, forward) {
                    patch.push_str(&hunk);
                }
            }
            if mine == 0 {
                continue;
            }
            if mine == all {
                part.whole.push(file.clone());
            } else {
                part.partial.push(file.path.clone());
                part.patch.push_str(&header);
                part.patch.push_str(&patch);
            }
        }
        Ok(part)
    }

    /// The name of list `id`.
    pub(crate) fn changelist_name(&self, id: &str) -> Result<String> {
        let store = self.read_store()?;
        store
            .lists
            .iter()
            .find(|l| l.id == id)
            .map(|l| l.name.clone())
            .ok_or_else(gone)
    }

    /// The commands of a commit of list `id`: `commit`, the `git commit` itself, run on a
    /// staging area of the list's own.
    pub(crate) fn plan_changelist_commit(&self, id: &str, commit: GitCommand) -> Result<Vec<GitCommand>> {
        let name = self.changelist_name(id)?;
        // Its own commit would finish a merge, a cherry-pick or a rebase without what they bring.
        if let Some(op) = self.operation()?
            && op.kind != crate::operation::OperationKind::StashApply
        {
            let what = match op.kind {
                crate::operation::OperationKind::Merge | crate::operation::OperationKind::Squash => "a merge",
                crate::operation::OperationKind::Rebase => "a rebase",
                crate::operation::OperationKind::CherryPick => "a cherry-pick",
                crate::operation::OperationKind::Revert => "a revert",
                _ => "applying patches",
            };
            return Err(Error::Git(format!(
                "“{name}” can’t be committed in the middle of {what}: finish or abort it first, or commit the staged changes"
            )));
        }
        let part = self.list_part(Some(id), None, true)?;
        if part.is_empty() {
            return Err(Error::Git(format!("“{name}” has no changes to commit")));
        }
        let index = self.git_path(INDEX)?;
        if let Some(dir) = index.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|err| Error::Git(format!("could not commit the changelist: {err}")))?;
        }
        let index = index.to_string_lossy().into_owned();
        let own = |cmd: GitCommand| cmd.env("GIT_INDEX_FILE", index.as_str());
        let paths = |args: &[&str], paths: &[String]| {
            GitCommand::new(
                args.iter()
                    .map(|s| s.to_string())
                    .chain(["--".to_owned()])
                    .chain(paths.iter().map(|p| literal(p))),
            )
        };
        let mut commands = Vec::new();
        // The checks before commit run before anything: shown before the first command.
        let mut commit = commit;
        let checks = commit.before.take();
        let start: &[&str] = if self.head()?.commit.is_some() {
            &["read-tree", "HEAD"]
        } else {
            &["read-tree", "--empty"]
        };
        let mut first = own(GitCommand::new(start.iter().copied())).comment(format!(
            "GIT_INDEX_FILE={INDEX}: a staging area of its own for “{name}”, from the last commit; yours stays as it is"
        ));
        first.before = checks;
        commands.push(first);
        // The staged lines of a file it shares with other lists go with the file.
        let carried: Vec<String> = {
            let tree = self.working_tree()?;
            let staged: HashSet<&str> = tree.staged.iter().map(|f| f.path.as_str()).collect();
            part.partial
                .iter()
                .filter(|p| staged.contains(p.as_str()))
                .cloned()
                .collect()
        };
        if !carried.is_empty() {
            let mut args: Vec<String> = ["ls-files", "-s", "-z", "--"].map(str::to_owned).into();
            args.extend(carried.iter().map(|p| literal(p)));
            let entries = self.run(&GitCommand::new(args))?.stdout;
            commands.push(
                own(GitCommand::new(["update-index", "-z", "--index-info"]))
                    .comment("with what is staged of the files it shares with other lists")
                    .input(entries),
            );
        }
        if !part.whole.is_empty() {
            let whole: Vec<String> = part.whole.iter().map(|f| f.path.clone()).collect();
            commands.push(own(paths(&["add", "-A"], &whole)).comment(format!("the files of “{name}” as they are now")));
        }
        if !part.patch.is_empty() {
            commands.push(
                own(GitCommand::new(["apply", "--cached", "-"]))
                    .comment(format!("only the changes of “{name}” in {}", part.partial.join(", ")))
                    .input(part.patch.clone()),
            );
        }
        commands.push(own(commit));
        commands.push(
            paths(&["reset", "-q"], &part.paths())
                .comment("your staging area takes these files from the new commit; other staged files stay staged"),
        );
        Ok(commands)
    }

    /// The commands of staging (`discard` off) or discarding what `list` holds of `paths`.
    pub(crate) fn plan_list_part(
        &self,
        list: Option<&str>,
        paths: &[String],
        discard: bool,
    ) -> Result<Vec<GitCommand>> {
        let name = match list {
            Some(id) => format!("“{}”", self.changelist_name(id)?),
            None => "Unstaged".to_owned(),
        };
        let part = self.list_part(list, Some(paths), !discard)?;
        if part.is_empty() {
            return Err(Error::Git(format!(
                "{name} holds nothing of {} any more; look again",
                if paths.len() == 1 {
                    paths[0].clone()
                } else {
                    "these files".to_owned()
                }
            )));
        }
        let with_paths = |args: &[&str], paths: Vec<String>| {
            GitCommand::new(
                args.iter()
                    .map(|s| s.to_string())
                    .chain(["--".to_owned()])
                    .chain(paths.iter().map(|p| literal(p))),
            )
        };
        let mut commands = Vec::new();
        let whole = |untracked: bool| -> Vec<String> {
            part.whole
                .iter()
                .filter(|f| (f.status == FileStatus::Untracked) == untracked)
                .map(|f| f.path.clone())
                .collect()
        };
        if discard {
            if !whole(false).is_empty() {
                commands.push(
                    with_paths(&["restore", "--worktree"], whole(false))
                        .comment("put back the staged or committed version"),
                );
            }
            if !whole(true).is_empty() {
                commands
                    .push(with_paths(&["clean", "-f"], whole(true)).comment("delete the new files git does not track"));
            }
            if !part.patch.is_empty() {
                commands.push(
                    GitCommand::new(["apply", "--reverse", "-"])
                        .comment(format!(
                            "undo only the changes of {name} in {}",
                            part.partial.join(", ")
                        ))
                        .input(part.patch),
                );
            }
        } else {
            let all: Vec<String> = part.whole.iter().map(|f| f.path.clone()).collect();
            if !all.is_empty() {
                commands.push(with_paths(&["add"], all).comment("add the files to the next commit"));
            }
            if !part.patch.is_empty() {
                commands.push(
                    GitCommand::new(["apply", "--cached", "-"])
                        .comment(format!(
                            "stage only the changes of {name} in {}",
                            part.partial.join(", ")
                        ))
                        .input(part.patch),
                );
            }
        }
        Ok(commands)
    }

    /// Before a list's commit runs: what the secrets check finds in the lines it adds, read
    /// from a staging area built as the commit builds its own.
    pub(crate) fn changelist_secrets(&self, commands: &[GitCommand]) -> Result<Vec<crate::secrets::SecretFinding>> {
        let index = self.git_path(CHECK_INDEX)?;
        let scratch = index.to_string_lossy().into_owned();
        let result = (|| {
            for command in commands {
                let own = command.env.iter().any(|(k, _)| k == "GIT_INDEX_FILE");
                if !own || command.args.first().is_some_and(|a| a == "commit") {
                    continue;
                }
                let mut command = command.clone();
                command.env = vec![("GIT_INDEX_FILE".to_owned(), scratch.clone())];
                match &command.input {
                    Some(input) => self.run_with_input(&command, Some(input.as_bytes()))?,
                    None => self.run(&command)?,
                };
            }
            self.staged_secrets_with_index(&scratch)
        })();
        let _ = std::fs::remove_file(&index);
        result
    }

    /// After list `id` was committed: the commit's staging area goes, and so does the list
    /// when nothing is left in it, unless it is the active one.
    pub(crate) fn after_changelist_commit(&self, id: &str, committed: bool) -> Result<()> {
        if let Ok(index) = self.git_path(INDEX) {
            let _ = std::fs::remove_file(index);
        }
        if !committed {
            return Ok(());
        }
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tree = self.working_tree()?;
        let mut store = self.read_store()?;
        self.normalize(&mut store, &tree);
        let empty = store
            .lists
            .iter()
            .find(|l| l.id == id)
            .is_some_and(|l| l.files.is_empty())
            && !store.runs.iter().any(|r| r.list.as_deref() == Some(id))
            && store.active.as_deref() != Some(id);
        if empty {
            store.lists.retain(|l| l.id != id);
        }
        self.write_store(&store)
    }
}

/// Files that have no changes any more leave their lists, and runs whose file has no unstaged
/// changes go, as when a file is committed without its list: changed again, it is new. Whether
/// anything changed.
fn drop_unchanged(store: &mut Store, tree: &WorkingTree) -> bool {
    let changed: HashSet<&str> = tree
        .staged
        .iter()
        .chain(&tree.unstaged)
        .chain(&tree.conflicted)
        .map(|f| f.path.as_str())
        .collect();
    let unstaged: HashSet<&str> = listable(tree).map(|f| f.path.as_str()).collect();
    let held = |store: &Store| store.lists.iter().map(|l| l.files.len()).sum::<usize>() + store.runs.len();
    let before = held(store);
    for list in &mut store.lists {
        list.files.retain(|f| changed.contains(f.as_str()));
    }
    store.runs.retain(|r| unstaged.contains(r.path.as_str()));
    held(store) != before
}

/// With a list active, the files that started changing since the last reading go to it.
/// Whether anything changed.
fn take_new_files(store: &mut Store, tree: &WorkingTree) -> bool {
    let Some(active) = store.active.clone().filter(|id| store.has(id)) else {
        return false;
    };
    let seen: HashSet<&str> = store.seen.iter().map(String::as_str).collect();
    let fresh: Vec<String> = listable(tree)
        .map(|f| f.path.clone())
        .filter(|p| !seen.contains(p.as_str()) && store.owner(p).is_none() && !store.keyed(p))
        .collect();
    let mut now: Vec<String> = tree
        .staged
        .iter()
        .chain(&tree.unstaged)
        .chain(&tree.conflicted)
        .map(|f| f.path.clone())
        .collect();
    now.sort();
    now.dedup();
    let changed = !fresh.is_empty() || now != store.seen;
    if let Some(list) = store.lists.iter_mut().find(|l| l.id == active) {
        list.files.extend(fresh);
    }
    store.seen = now;
    changed
}

/// A list's name, trimmed, refused when empty, taken by another list, or the name of one of
/// the panel's own lists.
fn checked_name(store: &Store, name: &str, except: Option<&str>) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Git("A changelist needs a name".into()));
    }
    if TAKEN.contains(&name.to_lowercase().as_str()) {
        return Err(Error::Git(format!(
            "“{name}” is the name of one of the lists of Changes; pick another"
        )));
    }
    if store
        .lists
        .iter()
        .any(|l| Some(l.id.as_str()) != except && l.name.to_lowercase() == name.to_lowercase())
    {
        return Err(Error::Git(format!("There is a changelist called “{name}” already")));
    }
    Ok(name.to_owned())
}

/// A new list's id, not taken in `store`.
fn new_id(store: &Store) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let mut n = 0;
    loop {
        let id = format!("{now:x}{}", if n == 0 { String::new() } else { format!("-{n}") });
        if !store.has(&id) {
            return id;
        }
        n += 1;
    }
}

/// `edit` made to `store`; `keys` gives the run keys of a file's unstaged changes.
fn apply_edit(
    store: &mut Store,
    edit: &ChangelistEdit,
    tree: &WorkingTree,
    keys: &dyn Fn(&str) -> Vec<String>,
) -> Result<()> {
    match edit {
        ChangelistEdit::Create { name, moving } => {
            let name = checked_name(store, name, None)?;
            let id = new_id(store);
            store.lists.push(StoredList {
                id: id.clone(),
                name,
                files: Vec::new(),
            });
            if let Some(moving) = moving {
                move_to(store, moving, Some(&id), keys)?;
            }
        }
        ChangelistEdit::Rename { id, name } => {
            let name = checked_name(store, name, Some(id))?;
            store.list_mut(id)?.name = name;
        }
        ChangelistEdit::Delete { id } => {
            if !store.has(id) {
                return Err(gone());
            }
            store.lists.retain(|l| &l.id != id);
            for run in &mut store.runs {
                if run.list.as_ref() == Some(id) {
                    run.list = None;
                }
            }
            if store.active.as_ref() == Some(id) {
                store.active = None;
            }
        }
        ChangelistEdit::Move { moving, to } => {
            if let Some(id) = to
                && !store.has(id)
            {
                return Err(gone());
            }
            move_to(store, moving, to.as_deref(), keys)?;
        }
        ChangelistEdit::Activate { id } => {
            if let Some(id) = id
                && !store.has(id)
            {
                return Err(gone());
            }
            store.active = id.clone();
            // What has changes now stays where it is: only files that start changing go there.
            let mut now: Vec<String> = tree
                .staged
                .iter()
                .chain(&tree.unstaged)
                .chain(&tree.conflicted)
                .map(|f| f.path.clone())
                .collect();
            now.sort();
            now.dedup();
            store.seen = if id.is_some() { now } else { Vec::new() };
        }
    }
    Ok(())
}

fn move_to(store: &mut Store, moving: &Moving, to: Option<&str>, keys: &dyn Fn(&str) -> Vec<String>) -> Result<()> {
    match moving {
        Moving::Files { paths } => {
            for path in paths {
                store.runs.retain(|r| &r.path != path);
                store.set_owner(path, to);
            }
        }
        Moving::Part { paths, from } => {
            if let Some(id) = from
                && !store.has(id)
            {
                return Err(gone());
            }
            for path in paths {
                let owner = store.owner(path).map(str::to_owned);
                if store.keyed(path) {
                    // Each run goes where it is now; those of `from` go to `to`.
                    let all = keys(path);
                    let wheres: Vec<(String, Option<String>)> = all
                        .into_iter()
                        .map(|k| {
                            let at = store.run_list(path, &k).map(str::to_owned);
                            (k, at)
                        })
                        .collect();
                    for (key, at) in wheres {
                        if at == *from {
                            store.set_run(path, &key, to);
                        } else {
                            store.set_run(path, &key, at.as_deref());
                        }
                    }
                } else if owner == *from {
                    store.set_owner(path, to);
                }
            }
        }
        Moving::Runs { path, keys: moved } => {
            for key in moved {
                store.set_run(path, key, to);
            }
        }
    }
    Ok(())
}
