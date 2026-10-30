//! Settings › Repository Files: the files committed with the repository that tell git and the
//! tools around it how to treat it (.gitignore, .gitattributes, .mailmap, …). Each is read,
//! checked as it is typed and written back; Save only writes the file, which then shows up in
//! Changes like any other edit.
//!
//! A check finds errors, which keep the file from being saved because git (or GitHub, or
//! git-lfs) would read a line other than meant or stop, and warnings, which are only shown. Errors
//! come from the text alone; warnings and the gray notes beside each line also look at the
//! working copy and the history, read once when the file is opened ([`Repo::check_repo_file`]
//! with `fresh`) and kept while it is edited. Reading them takes a while in a big repository, so
//! [`Repo::check_repo_file_now`] checks the text alone until they are in.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gix::bstr::ByteSlice;
use serde::{Deserialize, Serialize};

use crate::cli::{GitCommand, literal};
use crate::error::{Error, Result};
use crate::repo::Repo;

/// One of the shared files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RepoFileKind {
    Gitignore,
    Gitattributes,
    Mailmap,
    BlameIgnoreRevs,
    Lfsconfig,
    Gitmodules,
    Codeowners,
    Editorconfig,
}

impl RepoFileKind {
    pub const ALL: [RepoFileKind; 8] = [
        RepoFileKind::Gitignore,
        RepoFileKind::Gitattributes,
        RepoFileKind::Mailmap,
        RepoFileKind::BlameIgnoreRevs,
        RepoFileKind::Lfsconfig,
        RepoFileKind::Gitmodules,
        RepoFileKind::Codeowners,
        RepoFileKind::Editorconfig,
    ];

    /// Where the file can be, the one read first first; a new one goes to the first.
    fn places(self, gitlab: bool) -> &'static [&'static str] {
        match self {
            RepoFileKind::Gitignore => &[".gitignore"],
            RepoFileKind::Gitattributes => &[".gitattributes"],
            RepoFileKind::Mailmap => &[".mailmap"],
            RepoFileKind::BlameIgnoreRevs => &[crate::filehistory::IGNORE_REVS_FILE],
            RepoFileKind::Lfsconfig => &[".lfsconfig"],
            RepoFileKind::Gitmodules => &[".gitmodules"],
            // The order each forge reads them in.
            RepoFileKind::Codeowners if gitlab => &["CODEOWNERS", "docs/CODEOWNERS", ".gitlab/CODEOWNERS"],
            RepoFileKind::Codeowners => &[".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"],
            RepoFileKind::Editorconfig => &[".editorconfig"],
        }
    }
}

/// A row of the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoFile {
    pub kind: RepoFileKind,
    /// From the top of the repository.
    pub path: String,
    pub exists: bool,
    /// Lines that say something: not blank, not a comment.
    pub rules: usize,
    /// Changed since the last commit, or new and not committed.
    pub uncommitted: bool,
    /// The first error in it, if any.
    pub problem: Option<Problem>,
}

/// A file's text for the editor, or the starter of a new one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoFileText {
    pub kind: RepoFileKind,
    pub path: String,
    pub exists: bool,
    pub text: String,
    /// Why it can only be read, e.g. .gitmodules, which the submodule commands keep.
    pub read_only: Option<String>,
}

/// What Save did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Saved {
    /// The file is written, at this path.
    Written { path: String },
    /// The file on disk is not the one the text was typed over: something else changed or
    /// removed it since. Nothing is written; `text` is what is there now.
    Changed { text: Option<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    /// Git would read the line other than meant, or stop: the file can't be saved.
    Error,
    /// Worth a look; saving goes ahead.
    Warn,
}

/// Something wrong in a line. `from`..`to` are the part to underline, in UTF-16 units as the
/// editor counts them; line 0 is the file as a whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub line: usize,
    pub from: usize,
    pub to: usize,
    pub level: Level,
    /// Plain words; `code` in backticks.
    pub message: String,
    pub fix: Option<Fix>,
}

/// What a problem's button does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Fix {
    /// Put `text` in place of line `line`; none takes the line out.
    Replace {
        label: String,
        line: usize,
        text: Option<String>,
    },
    /// Stop tracking committed files a rule covers (`git rm --cached`).
    Untrack { label: String, paths: Vec<String> },
}

/// The gray words beside a line: what it matches, the commit it names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineNote {
    pub line: usize,
    pub text: String,
    pub level: Option<Level>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileCheck {
    pub problems: Vec<Problem>,
    pub notes: Vec<LineNote>,
    /// Checked by the text alone: the working copy or the history is still being read, and what
    /// comes from them (warnings, notes) is missing.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
}

impl FileCheck {
    pub fn errors(&self) -> usize {
        self.problems.iter().filter(|p| p.level == Level::Error).count()
    }

    fn error(&mut self, line: usize, text: &str, range: (usize, usize), message: impl Into<String>, fix: Option<Fix>) {
        self.push(Level::Error, line, text, range, message, fix);
    }

    fn warn(&mut self, line: usize, text: &str, range: (usize, usize), message: impl Into<String>, fix: Option<Fix>) {
        self.push(Level::Warn, line, text, range, message, fix);
    }

    /// `range` in bytes of `text`, the line.
    fn push(
        &mut self,
        level: Level,
        line: usize,
        text: &str,
        range: (usize, usize),
        message: impl Into<String>,
        fix: Option<Fix>,
    ) {
        let at = |byte: usize| text.get(..byte.min(text.len())).map_or(0, |s| s.encode_utf16().count());
        self.problems.push(Problem {
            line,
            from: at(range.0),
            to: at(range.1),
            level,
            message: message.into(),
            fix,
        });
    }

    fn note(&mut self, line: usize, text: impl Into<String>, level: Option<Level>) {
        self.notes.push(LineNote {
            line,
            text: text.into(),
            level,
        });
    }

    /// Errors first, then by line.
    fn sorted(mut self) -> Self {
        self.problems.sort_by_key(|p| (p.level != Level::Error, p.line));
        self
    }
}

/// More than anyone writes by hand; a bigger file is not taken.
const MAX_BYTES: usize = 1024 * 1024;
/// GitHub doesn't read a CODEOWNERS file bigger than this.
const GITHUB_CODEOWNERS_BYTES: usize = 3 * 1024 * 1024;
/// Entries of the working copy read for the notes; a bigger one is counted in part.
const MAX_ENTRIES: usize = 300_000;
/// Commits read for .mailmap.
const MAX_COMMITS: usize = 200_000;

impl Repo {
    /// The shared files, created or not. `gitlab` picks where CODEOWNERS goes.
    pub fn repo_files(&self, gitlab: bool) -> Result<Vec<RepoFile>> {
        let paths: Vec<String> = RepoFileKind::ALL
            .iter()
            .map(|&kind| self.repo_file_path(kind, gitlab))
            .collect();
        let changed = self.uncommitted(&paths);
        Ok(RepoFileKind::ALL
            .iter()
            .zip(paths)
            .map(|(&kind, path)| {
                let text = read_file(&self.workdir().join(&path), limit(kind, gitlab));
                let exists = text.is_some();
                let text = text.unwrap_or(Ok(String::new()));
                let problem = match &text {
                    Ok(text) => self
                        .check_text(kind, gitlab, text, None)
                        .problems
                        .into_iter()
                        .find(|p| p.level == Level::Error),
                    Err(why) => Some(Problem {
                        line: 0,
                        from: 0,
                        to: 0,
                        level: Level::Error,
                        message: why.clone(),
                        fix: None,
                    }),
                };
                RepoFile {
                    kind,
                    rules: text.as_deref().map_or(0, |t| rules(kind, t)),
                    uncommitted: changed.contains(&path),
                    path,
                    exists,
                    problem,
                }
            })
            .collect())
    }

    /// The file's text, or a starter for a new one.
    pub fn repo_file_text(&self, kind: RepoFileKind, gitlab: bool) -> Result<RepoFileText> {
        let path = self.repo_file_path(kind, gitlab);
        let read = read_file(&self.workdir().join(&path), limit(kind, gitlab));
        let exists = read.is_some();
        let (text, unreadable) = match read {
            Some(Ok(text)) => (text, None),
            Some(Err(why)) => (String::new(), Some(why)),
            None => (self.starter(kind), None),
        };
        let read_only = unreadable.or_else(|| {
            (kind == RepoFileKind::Gitmodules).then(|| {
                "The submodule commands keep this file: add, move and remove submodules in the Submodules list."
                    .to_owned()
            })
        });
        Ok(RepoFileText {
            kind,
            path,
            exists,
            text,
            read_only,
        })
    }

    /// What is wrong in `text` as `kind`, with the notes beside its lines. `fresh` reads the
    /// working copy and the history again instead of using what was read when the file opened.
    /// Reading them stops with the read's Stop ([`crate::cli::stoppable`]).
    pub fn check_repo_file(&self, kind: RepoFileKind, gitlab: bool, text: &str, fresh: bool) -> Result<FileCheck> {
        let context = Context::of(self, kind, fresh)?;
        Ok(self.check_text(kind, gitlab, text, Some(&context)))
    }

    /// [`Repo::check_repo_file`] without waiting for the working copy and the history: while they
    /// are not read (or `fresh` asks for them again), the text alone, with `pending` set.
    pub fn check_repo_file_now(&self, kind: RepoFileKind, gitlab: bool, text: &str, fresh: bool) -> FileCheck {
        let context = if fresh { None } else { Context::ready(self, kind) };
        let pending = context.is_none();
        FileCheck {
            pending,
            ..self.check_text(kind, gitlab, text, context.as_ref())
        }
    }

    /// Write `text` as the file, unless it has errors or is no longer `over`: the text it was
    /// typed over, nothing for a new file. Nothing is staged.
    pub fn save_repo_file(&self, kind: RepoFileKind, gitlab: bool, text: &str, over: Option<&str>) -> Result<Saved> {
        if kind == RepoFileKind::Gitmodules {
            return Err(Error::Git(".gitmodules is kept by the submodule commands.".into()));
        }
        let errors = self.check_text(kind, gitlab, text, None).errors();
        if errors > 0 {
            return Err(Error::Git(format!(
                "Fix {} to save.",
                if errors == 1 {
                    "the error".to_owned()
                } else {
                    format!("{errors} errors")
                }
            )));
        }
        let path = self.repo_file_path(kind, gitlab);
        let file = self.workdir().join(&path);
        let io = |err: std::io::Error| Error::Git(format!("{path} can't be saved: {err}"));
        // A committed link could point anywhere on the disk; write only a plain file.
        if let Ok(seen) = std::fs::symlink_metadata(&file)
            && !seen.file_type().is_file()
        {
            return Err(Error::Git(format!(
                "{path} is not a plain file here, so Oxbow doesn't write it."
            )));
        }
        if let Some(dir) = file.parent() {
            if let Ok(seen) = std::fs::symlink_metadata(dir)
                && seen.file_type().is_symlink()
            {
                return Err(Error::Git(format!(
                    "{path} is in a linked folder, so Oxbow doesn't write it."
                )));
            }
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        // One save at a time, so none writes between another's look and its write.
        let _saving = SAVING.lock().unwrap_or_else(|e| e.into_inner());
        let there = match read_file(&file, limit(kind, gitlab)) {
            None => None,
            Some(Ok(there)) => Some(there),
            Some(Err(why)) => return Err(Error::Git(format!("{path} changed on disk: {why}"))),
        };
        if there.as_deref() != over {
            return Ok(Saved::Changed { text: there });
        }
        // A new file of a unique name, made here and never one that was there before, so a
        // link put in its place can't take the text elsewhere; then it takes the file's place.
        let name = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let prefix = format!(".{name}.");
        let mut temp = tempfile::Builder::new();
        temp.prefix(&prefix).suffix(".oxbow");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).map_or(0o666, |seen| seen.permissions().mode() & 0o7777);
            temp.permissions(std::fs::Permissions::from_mode(mode));
        }
        let mut new = temp.tempfile_in(file.parent().unwrap_or(self.workdir())).map_err(io)?;
        new.write_all(text.as_bytes()).map_err(io)?;
        new.persist(&file).map_err(|err| io(err.error))?;
        forget_context(self.workdir());
        Ok(Saved::Written { path })
    }

    /// Where `kind` is, or goes when it is created.
    fn repo_file_path(&self, kind: RepoFileKind, gitlab: bool) -> String {
        let places = kind.places(gitlab);
        places
            .iter()
            .find(|p| std::fs::symlink_metadata(self.workdir().join(p)).is_ok())
            .unwrap_or(&places[0])
            .to_string()
    }

    /// Which of `paths` differ from the last commit or are new.
    fn uncommitted(&self, paths: &[String]) -> HashSet<String> {
        let mut args = vec![
            "status".to_owned(),
            "--porcelain=v1".to_owned(),
            "-z".to_owned(),
            "--untracked-files=all".to_owned(),
            "--".to_owned(),
        ];
        args.extend(paths.iter().map(|p| literal(p)));
        let Ok(out) = self.run(&GitCommand::new(args)) else {
            return HashSet::new();
        };
        out.stdout
            .split('\0')
            .filter_map(|entry| entry.get(3..))
            .map(str::to_owned)
            .collect()
    }

    fn check_text(&self, kind: RepoFileKind, gitlab: bool, text: &str, context: Option<&Context>) -> FileCheck {
        let mut check = FileCheck::default();
        if text.contains('\0') {
            check.error(
                0,
                "",
                (0, 0),
                "The text has a NUL character, so it isn't a text file.",
                None,
            );
            return check;
        }
        let (limit, message) = limit(kind, gitlab);
        if text.len() > limit {
            check.error(0, "", (0, 0), message, None);
            return check;
        }
        match kind {
            RepoFileKind::Gitignore => self.check_ignore(text, context, &mut check),
            RepoFileKind::Gitattributes => self.check_attributes(text, context, &mut check),
            RepoFileKind::Mailmap => check_mailmap(text, context.and_then(|c| c.people.as_deref()), &mut check),
            RepoFileKind::BlameIgnoreRevs => self.check_ignore_revs(text, &mut check),
            RepoFileKind::Lfsconfig => self.check_lfsconfig(text, &mut check),
            RepoFileKind::Gitmodules => check_gitmodules(text, context.and_then(|c| c.tree.as_deref()), &mut check),
            RepoFileKind::Codeowners => {
                self.check_codeowners(text, gitlab, context.and_then(|c| c.tree.as_deref()), &mut check)
            }
            RepoFileKind::Editorconfig => check_editorconfig(text, context.and_then(|c| c.tree.as_deref()), &mut check),
        }
        check.sorted()
    }
}

/// The most the file can be, the same for reading, checking and saving it, and what is said
/// when it is bigger.
fn limit(kind: RepoFileKind, gitlab: bool) -> (usize, &'static str) {
    if kind == RepoFileKind::Codeowners && !gitlab {
        (
            GITHUB_CODEOWNERS_BYTES,
            "GitHub doesn't read a CODEOWNERS file over 3 MB.",
        )
    } else {
        (MAX_BYTES, "The file is over 1 MB, more than Oxbow takes.")
    }
}

/// The file at `path` when it is there: its text, or why it can't be shown.
fn read_file(path: &Path, (limit, too_big): (usize, &str)) -> Option<std::result::Result<String, String>> {
    std::fs::symlink_metadata(path).ok()?;
    let Some(bytes) = crate::lfs::read_regular(path, limit as u64 + 1) else {
        return Some(Err("It is not a plain file here, so Oxbow doesn't open it.".into()));
    };
    if bytes.len() > limit {
        return Some(Err(too_big.into()));
    }
    Some(
        String::from_utf8(bytes)
            .map_err(|_| "The file is not UTF-8 text, so Oxbow only shows it can't edit it.".to_owned()),
    )
}

/// Lines that say something.
fn rules(kind: RepoFileKind, text: &str) -> usize {
    text.lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with('#')
                && !(matches!(
                    kind,
                    RepoFileKind::Lfsconfig | RepoFileKind::Gitmodules | RepoFileKind::Editorconfig
                ) && line.starts_with(';'))
        })
        .filter(|line| {
            !(matches!(
                kind,
                RepoFileKind::Lfsconfig | RepoFileKind::Gitmodules | RepoFileKind::Editorconfig
            ) && line.starts_with('['))
        })
        .count()
}

/// The lines of `text`, numbered from 1, without their `\r`.
fn numbered(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.split('\n')
        .enumerate()
        .map(|(i, line)| (i + 1, line.strip_suffix('\r').unwrap_or(line)))
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

// --- What the checks read besides the text --------------------------------------------------

/// The working copy and the history as read when the file was opened.
struct Context {
    tree: Option<Arc<Tree>>,
    people: Option<Arc<People>>,
}

/// What was read of one repository. Each part is read under its own lock, outside the list of
/// repositories: another repository's file is checked meanwhile, and a second check of this one
/// waits for the reading and takes it.
struct Cached {
    workdir: PathBuf,
    made: Instant,
    tree: Mutex<Option<Arc<Tree>>>,
    people: Mutex<Option<Arc<People>>>,
}

/// What was read of the repositories whose files were checked last, the newest last.
static CACHE: Mutex<Vec<Arc<Cached>>> = Mutex::new(Vec::new());
static SAVING: Mutex<()> = Mutex::new(());

/// How long what was read stays without `fresh`.
const KEEP: Duration = Duration::from_secs(10 * 60);
/// Repositories whose working copy and history are kept: a window each, about.
const KEPT_REPOS: usize = 8;
/// Entries of working copies kept at most, all repositories together: two of the biggest.
const KEPT_ENTRIES: usize = 2 * MAX_ENTRIES;

fn forget_context(workdir: &Path) {
    if let Ok(mut cache) = CACHE.lock() {
        cache.retain(|c| c.workdir != workdir);
    }
}

/// What was read of the repository at `workdir`, or a new place for it when there is none, it
/// is too old, or `fresh` asks for it afresh.
fn cached(workdir: &Path, fresh: bool) -> Arc<Cached> {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let at = cache.iter().position(|c| c.workdir == workdir);
    let kept = at
        .map(|i| cache.remove(i))
        .filter(|c| !fresh && c.made.elapsed() <= KEEP);
    let entry = kept.unwrap_or_else(|| {
        Arc::new(Cached {
            workdir: workdir.to_path_buf(),
            made: Instant::now(),
            tree: Mutex::default(),
            people: Mutex::default(),
        })
    });
    cache.push(entry.clone());
    // The ones checked longest ago go first, past the budget; one being read counts as none.
    let entries = |c: &Cached| taken(&c.tree).map_or(0, |t| t.entries.len());
    while cache.len() > 1
        && (cache.len() > KEPT_REPOS || cache.iter().map(|c| entries(c)).sum::<usize>() > KEPT_ENTRIES)
    {
        cache.remove(0);
    }
    entry
}

/// What `part` holds, unless it is being read now or not read yet.
fn taken<T>(part: &Mutex<Option<Arc<T>>>) -> Option<Arc<T>> {
    part.try_lock().ok().and_then(|p| p.clone())
}

/// What `kind`'s checks read besides the text.
fn wants(kind: RepoFileKind) -> (bool, bool) {
    let tree = !matches!(
        kind,
        RepoFileKind::Mailmap | RepoFileKind::BlameIgnoreRevs | RepoFileKind::Lfsconfig
    );
    (tree, kind == RepoFileKind::Mailmap)
}

impl Context {
    /// Read what `kind` needs, unless it was read already. A Stop leaves nothing read.
    fn of(repo: &Repo, kind: RepoFileKind, fresh: bool) -> Result<Context> {
        let (wants_tree, wants_people) = wants(kind);
        let cached = cached(repo.workdir(), fresh);
        let mut context = Context {
            tree: None,
            people: None,
        };
        if wants_tree {
            let mut tree = cached.tree.lock().unwrap_or_else(|e| e.into_inner());
            if tree.is_none() {
                *tree = Some(Arc::new(Tree::read(repo)?));
            }
            context.tree = tree.clone();
        }
        if wants_people {
            let mut people = cached.people.lock().unwrap_or_else(|e| e.into_inner());
            if people.is_none() {
                *people = Some(Arc::new(People::read(repo)?));
            }
            context.people = people.clone();
        }
        Ok(context)
    }

    /// What `kind` needs when it is read already and not being read now; nothing otherwise.
    fn ready(repo: &Repo, kind: RepoFileKind) -> Option<Context> {
        let (wants_tree, wants_people) = wants(kind);
        if !wants_tree && !wants_people {
            return Some(Context {
                tree: None,
                people: None,
            });
        }
        let cached = {
            let cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
            cache
                .iter()
                .find(|c| c.workdir == repo.workdir() && c.made.elapsed() <= KEEP)
                .cloned()?
        };
        let tree = if wants_tree { Some(taken(&cached.tree)?) } else { None };
        let people = if wants_people {
            Some(taken(&cached.people)?)
        } else {
            None
        };
        Some(Context { tree, people })
    }
}

/// Small LRU caches also bound intermediate rules typed during a long editing session.
struct Memo<T> {
    entries: std::collections::VecDeque<(String, T, usize)>,
    bytes: usize,
}

impl<T> Default for Memo<T> {
    fn default() -> Self {
        Self {
            entries: Default::default(),
            bytes: 0,
        }
    }
}

impl<T: Clone> Memo<T> {
    const LIMIT: usize = 2 * 1024 * 1024;
    const COUNT: usize = 128;

    fn get(&mut self, key: &str) -> Option<T> {
        let at = self.entries.iter().position(|(k, ..)| k == key)?;
        let entry = self.entries.remove(at)?;
        let value = entry.1.clone();
        self.entries.push_back(entry);
        Some(value)
    }

    fn insert(&mut self, key: String, value: T, size: usize) {
        let size = size + key.capacity() + std::mem::size_of::<(String, T, usize)>();
        if size > Self::LIMIT {
            return;
        }
        if let Some(at) = self.entries.iter().position(|(k, ..)| k == &key) {
            self.bytes -= self.entries.remove(at).expect("entry exists").2;
        }
        while self.entries.len() >= Self::COUNT || self.bytes + size > Self::LIMIT {
            let Some((.., old)) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= old;
        }
        self.entries.push_back((key, value, size));
        self.bytes += size;
    }
}

/// The working copy's files and folders, ignored ones too, with what the index tracks.
struct Tree {
    /// Depth first, a folder before what is in it.
    entries: Vec<Entry>,
    tracked: HashSet<String>,
    /// Submodules in the index.
    gitlinks: HashSet<String>,
    /// More than [`MAX_ENTRIES`]: counts are at least what they say.
    partial: bool,
    fold: bool,
    /// What each .gitignore line matched, by its text: a check as each key is typed counts
    /// only the line that changed.
    matched: Mutex<Memo<Arc<Matched>>>,
    /// Files each pattern of the other files covers, by kind and text.
    counted: Mutex<Memo<usize>>,
}

struct Entry {
    path: String,
    dir: bool,
    /// The index after everything in the folder.
    end: usize,
    /// Files in the folder, at any depth.
    files: usize,
}

impl Tree {
    fn read(repo: &Repo) -> Result<Tree> {
        let local = repo.local();
        let fold = local.config_snapshot().boolean("core.ignorecase").unwrap_or(false);
        let mut tracked = HashSet::new();
        let mut gitlinks = HashSet::new();
        if let Ok(index) = local.index_or_empty() {
            for entry in index.entries() {
                let path = entry.path(&index).to_str_lossy().into_owned();
                if entry.mode == gix::index::entry::Mode::COMMIT {
                    gitlinks.insert(path.clone());
                }
                tracked.insert(path);
            }
        }
        let mut tree = Tree {
            entries: Vec::new(),
            tracked,
            gitlinks,
            partial: false,
            fold,
            matched: Mutex::default(),
            counted: Mutex::default(),
        };
        tree.walk(repo.workdir(), "")?;
        Ok(tree)
    }

    /// Add what is in `dir` (`prefix` from the top), returning how many files.
    fn walk(&mut self, dir: &Path, prefix: &str) -> Result<usize> {
        crate::cli::check_stop()?;
        let Ok(read) = std::fs::read_dir(dir) else {
            return Ok(0);
        };
        let mut names: Vec<(String, bool)> = read
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_str()?.to_owned();
                // Links are files to git, whatever they point at.
                let dir = e.file_type().ok()?.is_dir();
                Some((name, dir))
            })
            .filter(|(name, _)| prefix.is_empty() && name != ".git" || !prefix.is_empty())
            .collect();
        names.sort();
        let mut files = 0;
        for (name, is_dir) in names {
            if self.entries.len() >= MAX_ENTRIES {
                self.partial = true;
                break;
            }
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let at = self.entries.len();
            self.entries.push(Entry {
                path: path.clone(),
                dir: is_dir,
                end: at + 1,
                files: 0,
            });
            if is_dir {
                let full = dir.join(&name);
                // Another repository inside: git doesn't look in it.
                let inner = if full.join(".git").exists() {
                    0
                } else {
                    self.walk(&full, &path)?
                };
                self.entries[at].files = inner;
                self.entries[at].end = self.entries.len();
                files += inner;
            } else {
                files += 1;
            }
        }
        Ok(files)
    }

    fn case(&self) -> gix::glob::pattern::Case {
        if self.fold {
            gix::glob::pattern::Case::Fold
        } else {
            gix::glob::pattern::Case::Sensitive
        }
    }

    /// What the .gitignore line `rule` matches, remembered.
    fn ignore_matches(&self, rule: &str, pattern: &gix::glob::Pattern) -> Arc<Matched> {
        if let Some(known) = self.matched.lock().ok().and_then(|mut m| m.get(rule)) {
            return known;
        }
        let matched = Arc::new(self.ignore_walk(pattern));
        if let Ok(mut memo) = self.matched.lock() {
            memo.insert(
                rule.to_owned(),
                matched.clone(),
                std::mem::size_of::<Matched>() + matched.within.capacity() * std::mem::size_of::<(usize, usize)>(),
            );
        }
        matched
    }

    /// What `pattern` matches as a .gitignore line: a matched folder is everything in it.
    fn ignore_walk(&self, pattern: &gix::glob::Pattern) -> Matched {
        let mut matched = Matched::default();
        let mut i = 0;
        while i < self.entries.len() {
            let entry = &self.entries[i];
            if glob_matches(pattern, &entry.path, entry.dir, self.case()) {
                if entry.dir {
                    matched.folders += 1;
                    matched.files += entry.files;
                    matched.within.push((i, entry.end));
                    i = entry.end;
                    continue;
                }
                matched.files += 1;
                matched.within.push((i, i + 1));
            }
            i += 1;
        }
        matched
    }

    /// Tracked files in what was matched; a tracked folder is given as itself.
    fn tracked_in(&self, matched: &Matched) -> Vec<String> {
        let mut paths = Vec::new();
        for &(from, _) in &matched.within {
            let entry = &self.entries[from];
            if entry.dir {
                let inside = format!("{}/", entry.path);
                if self.tracked.iter().any(|t| t.starts_with(&inside)) {
                    paths.push(entry.path.clone());
                }
            } else if self.tracked.contains(&entry.path) {
                paths.push(entry.path.clone());
            }
        }
        paths
    }

    /// Files (not folders) that `matches` takes, remembered by `key`.
    fn count_files(&self, key: String, matches: impl Fn(&str) -> bool) -> usize {
        if let Some(known) = self.counted.lock().ok().and_then(|mut m| m.get(&key)) {
            return known;
        }
        let count = self.entries.iter().filter(|e| !e.dir && matches(&e.path)).count();
        if let Ok(mut memo) = self.counted.lock() {
            memo.insert(key, count, 0);
        }
        count
    }

    fn more(&self) -> &'static str {
        if self.partial { "+" } else { "" }
    }
}

#[derive(Default)]
struct Matched {
    folders: usize,
    files: usize,
    /// Entries matched, as ranges of [`Tree::entries`].
    within: Vec<(usize, usize)>,
}

impl Matched {
    fn note(&self, more: &str) -> String {
        match (self.folders, self.files) {
            (0, files) => plural(files, "file", "files") + more,
            (folders, files) => format!(
                "{} · {}{more}",
                plural(folders, "folder", "folders"),
                plural(files, "file", "files")
            ),
        }
    }

    fn any(&self) -> bool {
        self.folders + self.files > 0
    }
}

fn glob_matches(pattern: &gix::glob::Pattern, path: &str, dir: bool, case: gix::glob::pattern::Case) -> bool {
    pattern.matches_repo_relative_path(
        path.as_bytes().as_bstr(),
        path.rfind('/').map(|i| i + 1),
        Some(dir),
        case,
        gix::glob::wildmatch::Mode::NO_MATCH_SLASH_LITERAL,
    )
}

/// Who wrote and committed the history: name and e-mail as written, with their commits.
#[derive(Default)]
struct People {
    /// (name, e-mail) → commits.
    ids: HashMap<(String, String), usize>,
}

impl People {
    fn read(repo: &Repo) -> Result<People> {
        let mut ids: HashMap<(String, String), usize> = HashMap::new();
        let local = repo.local();
        let mut tips: Vec<gix::ObjectId> = repo
            .refs()
            .unwrap_or_default()
            .iter()
            .filter_map(|r| gix::ObjectId::from_hex(r.target.as_bytes()).ok())
            .collect();
        if let Some(head) = repo
            .head()
            .ok()
            .and_then(|h| h.commit)
            .and_then(|h| gix::ObjectId::from_hex(h.as_bytes()).ok())
        {
            tips.push(head);
        }
        tips.sort();
        tips.dedup();
        if tips.is_empty() {
            return Ok(People { ids });
        }
        let Ok(walk) = local.rev_walk(tips).all() else {
            return Ok(People { ids });
        };
        for (n, info) in walk.take(MAX_COMMITS).flatten().enumerate() {
            if n % 1024 == 0 {
                crate::cli::check_stop()?;
            }
            let Ok(commit) = local.find_commit(info.id) else {
                continue;
            };
            let mut seen = |sig: gix::actor::SignatureRef<'_>, count: usize| {
                let key = (
                    sig.name.to_str_lossy().trim().to_owned(),
                    sig.email.to_str_lossy().trim().to_owned(),
                );
                *ids.entry(key).or_default() += count;
            };
            if let Ok(author) = commit.author() {
                seen(author, 1);
            }
            // A committer is someone the map should know too, but the commits are the author's.
            if let Ok(committer) = commit.committer() {
                seen(committer, 0);
            }
        }
        Ok(People { ids })
    }

    fn by_email(&self, email: &str) -> impl Iterator<Item = (&(String, String), &usize)> {
        let email = email.to_lowercase();
        self.ids.iter().filter(move |((_, e), _)| e.to_lowercase() == email)
    }

    /// The known e-mail closest to `email`, when it is a typo away.
    fn closest_email(&self, email: &str) -> Option<String> {
        let email = email.to_lowercase();
        let mut known: Vec<&String> = self.ids.keys().map(|(_, e)| e).collect();
        known.sort();
        known.dedup();
        known
            .into_iter()
            .map(|e| (distance(&email, &e.to_lowercase()), e))
            .filter(|(d, _)| (1..=2).contains(d))
            .min_by_key(|(d, _)| *d)
            .map(|(_, e)| e.clone())
    }
}

/// Edits between `a` and `b`.
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let next = (row[j + 1] + 1).min(row[j] + 1).min(prev + usize::from(ca != cb));
            prev = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

// --- .gitignore --------------------------------------------------------------------------------

/// A .gitignore line as git reads it: without the spaces at its end that aren't escaped.
fn ignore_text(line: &str) -> &str {
    let mut end = line.len();
    while line[..end].ends_with(' ') {
        let before = &line[..end - 1];
        let escapes = before.len() - before.trim_end_matches('\\').len();
        if escapes % 2 == 1 {
            break;
        }
        end -= 1;
    }
    &line[..end]
}

impl Repo {
    fn check_ignore(&self, text: &str, context: Option<&Context>, check: &mut FileCheck) {
        let tree = context.and_then(|c| c.tree.as_deref());
        let workdir = self.workdir().to_string_lossy().trim_start_matches('/').to_owned();
        let lines: Vec<(usize, &str)> = numbered(text).collect();
        // Each line's pattern, for the lines after it.
        let mut patterns: Vec<(usize, &str, gix::glob::Pattern)> = Vec::new();
        let mut seen: HashMap<&str, usize> = HashMap::new();
        for &(no, line) in &lines {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let rule = ignore_text(line);
            let mut bad = false;
            if rule.len() < line.len() && !rule.is_empty() {
                check.warn(
                    no,
                    line,
                    (rule.len(), line.len()),
                    "Git drops the spaces at the end of a line.",
                    Some(replace("Remove Spaces", no, rule)),
                );
            }
            let escapes = rule.len() - rule.trim_end_matches('\\').len();
            if escapes % 2 == 1 {
                check.error(
                    no,
                    line,
                    (rule.len() - 1, rule.len()),
                    "A `\\` at the end makes git skip the line.",
                    Some(replace("Remove \\", no, &rule[..rule.len() - 1])),
                );
                bad = true;
            }
            let body = rule.strip_prefix('!').unwrap_or(rule);
            let lead = rule.len() - body.len();
            if let Some(rest) = body.strip_prefix("./") {
                check.error(
                    no,
                    line,
                    (lead, lead + 2),
                    "Git never matches a line that starts with `./`. The top folder is `/`.",
                    Some(replace(
                        &format!("Use /{rest}"),
                        no,
                        &format!("{}/{rest}", &rule[..lead]),
                    )),
                );
                bad = true;
            } else if body.starts_with("../") {
                check.error(
                    no,
                    line,
                    (lead, lead + 3),
                    "A .gitignore can't reach outside the repository, so git never matches this.",
                    None,
                );
                bad = true;
            } else if let Some(rest) = body
                .strip_prefix('/')
                .and_then(|b| b.strip_prefix(&workdir))
                .filter(|_| !workdir.is_empty())
                && (rest.is_empty() || rest.starts_with('/'))
            {
                let inside = if rest.is_empty() {
                    "/".to_owned()
                } else {
                    rest.to_owned()
                };
                check.error(
                    no,
                    line,
                    (lead, rule.len()),
                    "This is a path on your disk. A .gitignore takes paths from the top of the repository.",
                    Some(replace(
                        &format!("Use {inside}"),
                        no,
                        &format!("{}{inside}", &rule[..lead]),
                    )),
                );
                bad = true;
            } else if body.starts_with("~/")
                || body.as_bytes().get(1..3) == Some(b":\\")
                || body.as_bytes().get(1..3) == Some(b":/")
            {
                check.error(
                    no,
                    line,
                    (lead, rule.len()),
                    "This is a path on your disk. A .gitignore takes paths from the top of the repository.",
                    None,
                );
                bad = true;
            } else if let Some(at) = windows_separator(body) {
                let fixed = body.replace('\\', "/");
                check.error(
                    no,
                    line,
                    (lead + at, lead + at + 1),
                    "`\\` keeps the next character as it is, so this matches nothing. Use `/` between folders.",
                    Some(replace(
                        &format!("Use {fixed}"),
                        no,
                        &format!("{}{fixed}", &rule[..lead]),
                    )),
                );
                bad = true;
            }
            if let Some(&first) = seen.get(rule) {
                check.warn(
                    no,
                    line,
                    (0, rule.len()),
                    format!("Line {first} says the same."),
                    Some(Fix::Replace {
                        label: "Remove Line".into(),
                        line: no,
                        text: None,
                    }),
                );
            } else {
                seen.insert(rule, no);
            }
            let Some(pattern) = gix::glob::Pattern::from_bytes(rule.as_bytes()) else {
                continue;
            };
            if pattern.is_negative() {
                self.check_negation(no, line, &pattern, &patterns, check);
            } else if (rule == "*" || rule == "/*") && !lines.iter().any(|&(n, l)| n > no && l.starts_with('!')) {
                check.warn(
                    no,
                    line,
                    (0, rule.len()),
                    "This leaves out every file in the repository.",
                    None,
                );
            }
            if let Some(tree) = tree {
                let matched = tree.ignore_matches(rule, &pattern);
                if bad {
                    if !matched.any() {
                        check.note(no, "matches nothing", Some(Level::Error));
                    }
                } else if pattern.is_negative() {
                    if matched.any() {
                        check.note(no, matched.note(tree.more()), None);
                    }
                } else if matched.any() {
                    let later: Vec<gix::glob::Pattern> = lines
                        .iter()
                        .filter(|&&(n, _)| n > no)
                        .filter_map(|&(_, l)| gix::glob::Pattern::from_bytes(ignore_text(l).as_bytes()))
                        .filter(|p| p.is_negative())
                        .collect();
                    let tracked: Vec<String> = tree
                        .tracked_in(&matched)
                        .into_iter()
                        .filter(|path| !later.iter().any(|p| glob_matches(p, path, false, tree.case())))
                        .collect();
                    if tracked.is_empty() {
                        check.note(no, matched.note(tree.more()), None);
                    } else {
                        let (what, it) = match tracked.as_slice() {
                            [one] => (format!("`{one}` is"), "it"),
                            many => (format!("{} matching files are", many.len()), "them"),
                        };
                        check.warn(
                            no,
                            line,
                            (0, rule.len()),
                            format!("{what} already committed: git keeps tracking {it} until you stop."),
                            Some(Fix::Untrack {
                                label: "Stop Tracking…".into(),
                                paths: tracked.iter().take(1000).cloned().collect(),
                            }),
                        );
                        check.note(
                            no,
                            format!("{}, already committed", plural(tracked.len(), "file", "files")),
                            Some(Level::Warn),
                        );
                    }
                } else if !pattern.has_wildcard()
                    && !rule.contains('\\')
                    && rule.trim_end_matches('/').trim_start_matches('/').contains('/')
                    && !tree.partial
                {
                    check.warn(
                        no,
                        line,
                        (0, rule.len()),
                        format!(
                            "Nothing in the repository is called `{}`.",
                            rule.trim_start_matches('/')
                        ),
                        None,
                    );
                    check.note(no, "not found", Some(Level::Warn));
                } else {
                    check.note(no, "nothing yet", None);
                }
            }
            patterns.push((no, rule, pattern));
        }
    }

    /// A `!` line: something above must leave out what it brings back, and not its folder.
    fn check_negation(
        &self,
        no: usize,
        line: &str,
        pattern: &gix::glob::Pattern,
        before: &[(usize, &str, gix::glob::Pattern)],
        check: &mut FileCheck,
    ) {
        let positive: Vec<&(usize, &str, gix::glob::Pattern)> =
            before.iter().filter(|(_, _, p)| !p.is_negative()).collect();
        if positive.is_empty() {
            check.warn(
                no,
                line,
                (0, line.trim_end().len()),
                "Nothing above leaves anything out, so `!` has nothing to bring back.",
                None,
            );
            return;
        }
        if pattern.has_wildcard() {
            return;
        }
        let path = pattern.text.to_str_lossy();
        let path = path.trim_end_matches('/');
        let mut folders = Vec::new();
        let mut at = 0;
        while let Some(slash) = path[at..].find('/') {
            folders.push(&path[..at + slash]);
            at += slash + 1;
        }
        for folder in folders {
            if let Some((other, text)) = positive
                .iter()
                .find(|(_, _, p)| glob_matches(p, folder, true, gix::glob::pattern::Case::Sensitive))
                .map(|e| (e.0, e.1))
            {
                let name = folder.rsplit('/').next().unwrap_or(folder);
                let fix = text
                    .strip_suffix('/')
                    .filter(|t| t.trim_start_matches('/') == name || t.trim_start_matches('/') == folder)
                    .map(|t| replace(&format!("Use {t}/* on line {other}"), other, &format!("{t}/*")));
                check.warn(no, line, (0, line.trim_end().len()), format!("`{text}` on line {other} leaves out the whole folder, so git can't bring back a file inside it."), fix);
                return;
            }
        }
    }
}

fn replace(label: &str, line: usize, text: &str) -> Fix {
    Fix::Replace {
        label: label.to_owned(),
        line,
        text: Some(text.to_owned()),
    }
}

/// Where a `\` stands between folder names (`build\out`), as on Windows: escaping a letter or
/// a dot means nothing to git.
fn windows_separator(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let name = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-');
    (1..bytes.len().saturating_sub(1))
        .find(|&i| bytes[i] == b'\\' && (name(bytes[i - 1]) || bytes[i - 1] == b'*') && name(bytes[i + 1]))
}

// --- .gitattributes ----------------------------------------------------------------------------

/// Diff drivers git has built in (userdiff.c).
const BUILTIN_DIFF: &[&str] = &[
    "ada", "bash", "bibtex", "cpp", "csharp", "css", "dts", "elixir", "fortran", "fountain", "golang", "html", "java",
    "kotlin", "markdown", "matlab", "objc", "pascal", "perl", "php", "python", "r", "ruby", "rust", "scheme", "tex",
];
const BUILTIN_MERGE: &[&str] = &["text", "binary", "union"];

fn attribute_name_ok(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_'))
}

impl Repo {
    fn check_attributes(&self, text: &str, context: Option<&Context>, check: &mut FileCheck) {
        let tree = context.and_then(|c| c.tree.as_deref());
        let config = self.local().config_snapshot().clone();
        let has = |key: String| config.string(key.as_str()).is_some();
        // pattern → attribute → (state, line)
        let mut set: HashMap<String, HashMap<String, (String, usize)>> = HashMap::new();
        for (no, line) in numbered(text) {
            let start = line.len() - line.trim_start_matches([' ', '\t']).len();
            let rest = &line[start..];
            if rest.trim().is_empty() || rest.starts_with('#') {
                continue;
            }
            let (pattern, after) = if let Some(quoted) = rest.strip_prefix('"') {
                match quoted.find('"') {
                    Some(end) => (&rest[..end + 2], &rest[end + 2..]),
                    None => {
                        check.error(
                            no,
                            line,
                            (start, line.len()),
                            "The `\"` is never closed, so git skips the line.",
                            None,
                        );
                        continue;
                    }
                }
            } else {
                let end = rest.find([' ', '\t']).unwrap_or(rest.len());
                (&rest[..end], &rest[end..])
            };
            let after_at = line.len() - after.len();
            let is_macro = pattern.starts_with("[attr]");
            if let Some(name) = pattern.strip_prefix("[attr]")
                && !attribute_name_ok(name)
            {
                check.error(
                    no,
                    line,
                    (start, start + pattern.len()),
                    format!("`{name}` is not a name git takes for a macro, so it skips the line."),
                    None,
                );
                continue;
            }
            if pattern.starts_with('!') {
                check.error(no, line, (start, start + 1), "Git doesn't take `!` in .gitattributes and skips this line. To match a name that starts with `!`, write `\\!`.", None);
                continue;
            }
            let mut attrs: Vec<(&str, String, usize)> = Vec::new();
            let mut offset = after_at;
            let mut broken = false;
            for word in after.split([' ', '\t', '\r']) {
                let at = offset;
                offset += word.len() + 1;
                if word.is_empty() {
                    continue;
                }
                let (name, state) = if let Some(n) = word.strip_prefix('-') {
                    (n, "-".to_owned())
                } else if let Some(n) = word.strip_prefix('!') {
                    (n, "!".to_owned())
                } else if let Some((n, v)) = word.split_once('=') {
                    (n, format!("={v}"))
                } else {
                    (word, "+".to_owned())
                };
                if !attribute_name_ok(name) || name.starts_with("builtin_") {
                    check.error(
                        no,
                        line,
                        (at, at + word.len()),
                        format!("`{name}` is not an attribute name git takes, so it skips the line."),
                        None,
                    );
                    broken = true;
                    continue;
                }
                attrs.push((name, state, at));
            }
            if broken {
                continue;
            }
            if attrs.is_empty() {
                if !is_macro {
                    check.warn(
                        no,
                        line,
                        (start, start + pattern.len()),
                        "A pattern alone sets nothing.",
                        None,
                    );
                }
                continue;
            }
            for (name, state, at) in &attrs {
                let value = state.strip_prefix('=');
                let span = (*at, *at + name.len() + state.len() - usize::from(state == "+"));
                match (*name, value) {
                    ("eol", Some(v)) if v != "lf" && v != "crlf" => {
                        check.error(
                            no,
                            line,
                            span,
                            format!("`eol={v}` is not `lf` or `crlf`, so git skips that setting."),
                            None,
                        );
                    }
                    ("text", Some(v)) if v != "auto" => {
                        check.error(
                            no,
                            line,
                            span,
                            format!("`text={v}`: git takes `text`, `-text` or `text=auto`."),
                            None,
                        );
                    }
                    ("working-tree-encoding", Some("")) => {
                        check.error(
                            no,
                            line,
                            span,
                            "`working-tree-encoding=` needs an encoding, such as `UTF-16`.",
                            None,
                        );
                    }
                    ("diff", Some(d))
                        if !BUILTIN_DIFF.contains(&d)
                            && ![
                                "textconv",
                                "command",
                                "xfuncname",
                                "funcname",
                                "binary",
                                "wordregex",
                                "cachetextconv",
                            ]
                            .iter()
                            .any(|k| has(format!("diff.{d}.{k}")))
                            && d != "lfs" =>
                    {
                        check.warn(
                            no,
                            line,
                            span,
                            format!("No `diff.{d}` in git’s settings here, so git diffs these files as text."),
                            None,
                        );
                    }
                    ("merge", Some(m))
                        if !BUILTIN_MERGE.contains(&m) && !has(format!("merge.{m}.driver")) && m != "lfs" =>
                    {
                        check.warn(
                            no,
                            line,
                            span,
                            format!("No `merge.{m}.driver` in git’s settings here, so git merges these files as text."),
                            None,
                        );
                    }
                    ("filter", Some("lfs")) => {
                        if self.lfs_version().is_none() {
                            check.warn(
                                no,
                                line,
                                span,
                                "Git LFS is not installed here, so these files are committed as they are.",
                                None,
                            );
                        }
                    }
                    ("filter", Some(f))
                        if !["clean", "smudge", "process"]
                            .iter()
                            .any(|k| has(format!("filter.{f}.{k}"))) =>
                    {
                        check.warn(
                            no,
                            line,
                            span,
                            format!("No `filter.{f}` in git’s settings here, so git stores these files as they are."),
                            None,
                        );
                    }
                    _ => {}
                }
            }
            if attrs.iter().any(|(n, s, _)| *n == "filter" && s == "=lfs") {
                let missing: Vec<&str> = [
                    ("diff", "=lfs", "diff=lfs"),
                    ("merge", "=lfs", "merge=lfs"),
                    ("text", "-", "-text"),
                ]
                .into_iter()
                .filter(|(n, s, _)| !attrs.iter().any(|(an, as_, _)| an == n && as_ == s))
                .map(|(_, _, w)| w)
                .collect();
                if !missing.is_empty() {
                    let fixed = format!("{} {}", line.trim_end(), missing.join(" "));
                    check.warn(
                        no,
                        line,
                        (start, line.trim_end().len()),
                        format!(
                            "Git LFS rules also say {}, so diffs and merges leave these files alone.",
                            missing.iter().map(|m| format!("`{m}`")).collect::<Vec<_>>().join(", ")
                        ),
                        Some(replace(&format!("Add {}", missing.join(" ")), no, &fixed)),
                    );
                }
            }
            if !is_macro {
                let known = set.entry(pattern.to_owned()).or_default();
                for (name, state, at) in &attrs {
                    match known.get(*name) {
                        Some((before, other)) if before != state => {
                            check.warn(
                                no,
                                line,
                                (*at, *at + name.len()),
                                format!("Line {other} sets `{name}` for the same files another way; this line wins."),
                                None,
                            );
                        }
                        _ => {}
                    }
                    known.insert((*name).to_owned(), (state.clone(), no));
                }
                if let Some(tree) = tree {
                    let unquoted = pattern.trim_matches('"');
                    if let Some(glob) = gix::glob::Pattern::from_bytes(unquoted.as_bytes()) {
                        let files = tree.count_files(format!("attributes:{unquoted}"), |path| {
                            glob_matches(&glob, path, false, tree.case())
                        });
                        if unquoted.ends_with('/') {
                            check.warn(no, line, (start, start + pattern.len()), "In .gitattributes a pattern that ends with `/` matches nothing. To cover a folder, write `folder/**`.", None);
                        } else if files == 0 {
                            check.note(no, "nothing yet", None);
                        } else {
                            check.note(no, plural(files, "file", "files") + tree.more(), None);
                        }
                    }
                }
            }
        }
    }
}

// --- .mailmap ----------------------------------------------------------------------------------

/// A .mailmap line: `Proper Name <proper@e-mail> Commit Name <commit@e-mail>`, any part but one
/// e-mail left out.
#[derive(Debug, Default, PartialEq, Eq)]
struct MailmapLine {
    name: Option<String>,
    email: String,
    old_name: Option<String>,
    old_email: Option<String>,
}

/// What is wrong with a line: the message, the part of the line, and the line put right.
type LineError = (String, (usize, usize), Option<String>);

/// The parts of a line, or what is wrong with it.
fn parse_mailmap(line: &str) -> std::result::Result<MailmapLine, LineError> {
    let content = line.split('#').next().unwrap_or("");
    let Some(open) = content.find('<') else {
        // An address written without its brackets.
        let words: Vec<(usize, &str)> = content
            .split(' ')
            .scan(0, |at, w| {
                let start = *at;
                *at += w.len() + 1;
                Some((start, w))
            })
            .collect();
        if let Some(&(at, word)) = words.iter().find(|(_, w)| w.contains('@')) {
            let fixed = format!("{}<{word}>{}", &line[..at], &line[at + word.len()..]);
            return Err((
                format!("An e-mail goes in angle brackets: `<{word}>`."),
                (at, at + word.len()),
                Some(fixed),
            ));
        }
        return Err((
            "Each line needs an e-mail in `< >`.".into(),
            (0, content.trim_end().len()),
            None,
        ));
    };
    let Some(close) = content[open..].find('>').map(|c| open + c) else {
        let end = content.trim_end().len();
        return Err((
            "The `<` is never closed.".into(),
            (open, end),
            Some(format!("{}>{}", &line[..end], &line[end..])),
        ));
    };
    let name = content[..open].trim();
    let email = content[open + 1..close].trim();
    let rest = &content[close + 1..];
    let (old_name, old_email) = match rest.find('<') {
        Some(o) => {
            let o = close + 1 + o;
            let Some(c) = content[o..].find('>').map(|c| o + c) else {
                let end = content.trim_end().len();
                return Err((
                    "The `<` is never closed.".into(),
                    (o, end),
                    Some(format!("{}>{}", &line[..end], &line[end..])),
                ));
            };
            let old = content[close + 1..o].trim();
            (
                Some(old).filter(|n| !n.is_empty()).map(str::to_owned),
                Some(content[o + 1..c].trim().to_owned()),
            )
        }
        None => (None, None),
    };
    if name.is_empty() && old_email.is_none() {
        return Err((
            "Give a name before the e-mail, or the e-mail used in commits after it in `< >`.".into(),
            (open, close + 1),
            None,
        ));
    }
    if !email.contains('@') {
        return Err((format!("`{email}` is not an e-mail address."), (open, close + 1), None));
    }
    Ok(MailmapLine {
        name: Some(name.to_owned()).filter(|n| !n.is_empty()),
        email: email.to_owned(),
        old_name,
        old_email,
    })
}

/// One person of a .mailmap: their first line, the identities it joins and their commits.
struct Person {
    first: usize,
    ids: HashSet<(String, String)>,
    commits: usize,
}

fn check_mailmap(text: &str, people: Option<&People>, check: &mut FileCheck) {
    // The commit identity a line maps → (line, proper identity).
    let mut mapped: HashMap<(Option<String>, String), (usize, String)> = HashMap::new();
    let mut groups: HashMap<String, Person> = HashMap::new();
    let mut lines_of: Vec<(usize, String, usize)> = Vec::new();
    for (no, line) in numbered(text) {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let entry = match parse_mailmap(line) {
            Ok(entry) => entry,
            Err((message, range, fixed)) => {
                let fix = fixed.map(|f| replace(&format!("Use {}", f.trim()), no, &f));
                check.error(no, line, range, message, fix);
                continue;
            }
        };
        let commit_email = entry.old_email.clone().unwrap_or_else(|| entry.email.clone());
        let key = (
            entry.old_name.as_ref().map(|n| n.to_lowercase()),
            commit_email.to_lowercase(),
        );
        let proper = format!(
            "{}\u{0}{}",
            entry.name.clone().unwrap_or_default(),
            entry.email.to_lowercase()
        );
        let at = line.rfind('<').unwrap_or(0);
        let range = (at, line[at..].find('>').map_or(line.len(), |c| at + c + 1));
        if let Some((other, before)) = mapped.get(&key)
            && *before != proper
        {
            check.warn(
                no,
                line,
                range,
                format!("Line {other} maps the same commits; this line wins."),
                None,
            );
        }
        mapped.insert(key, (no, proper.clone()));
        let Some(people) = people else { continue };
        let covered: Vec<(&(String, String), &usize)> = people
            .by_email(&commit_email)
            .filter(|((name, _), _)| entry.old_name.as_ref().is_none_or(|n| n.eq_ignore_ascii_case(name)))
            .collect();
        if covered.is_empty() {
            let any = people.by_email(&commit_email).next().is_some();
            let (message, fix) = match (&entry.old_name, any) {
                (Some(old), true) => (format!("No commit is signed `{old} <{commit_email}>`."), None),
                _ => match people.closest_email(&commit_email) {
                    Some(close) => {
                        let who = people
                            .by_email(&close)
                            .max_by_key(|(_, n)| **n)
                            .map(|((name, _), _)| name.clone())
                            .unwrap_or_default();
                        (
                            format!("No commit uses `{commit_email}`. {who} commits as `{close}`."),
                            Some(replace(
                                &format!("Use {close}"),
                                no,
                                &line.replacen(&format!("<{commit_email}>"), &format!("<{close}>"), 1),
                            )),
                        )
                    }
                    None => (format!("No commit uses `{commit_email}`."), None),
                },
            };
            check.warn(no, line, range, message, fix);
            check.note(no, "no commit uses this e-mail", Some(Level::Warn));
            continue;
        }
        let commits: usize = covered.iter().map(|(_, n)| **n).sum();
        let person = groups.entry(proper.clone()).or_insert(Person {
            first: no,
            ids: HashSet::new(),
            commits: 0,
        });
        person
            .ids
            .extend(covered.iter().map(|(id, _)| (id.0.clone(), id.1.to_lowercase())));
        person.commits += commits;
        lines_of.push((no, proper, commits));
    }
    // What each line joins, the first line of a person saying how many spellings in all.
    for (no, proper, commits) in lines_of {
        let person = &groups[&proper];
        if no == person.first && person.ids.len() > 1 {
            check.note(
                no,
                format!(
                    "joins {} spellings · {}",
                    person.ids.len(),
                    plural(person.commits, "commit", "commits")
                ),
                None,
            );
        } else {
            check.note(no, plural(commits, "commit", "commits"), None);
        }
    }
}

impl Repo {
    /// A new .mailmap: the people the history shows under more than one e-mail or name, each
    /// put under the one they use most.
    fn mailmap_starter(&self) -> String {
        let mut text = String::from(
            "# One person under one name and e-mail in History and Blame.\n# Proper Name <proper@e-mail> Name in Commits <e-mail in commits>\n",
        );
        let people = Context::of(self, RepoFileKind::Mailmap, false)
            .ok()
            .and_then(|c| c.people)
            .unwrap_or_default();
        let mut by_name: HashMap<String, Vec<(&String, &String, usize)>> = HashMap::new();
        for ((name, email), &n) in &people.ids {
            if n > 0 && !name.is_empty() {
                by_name.entry(name.to_lowercase()).or_default().push((name, email, n));
            }
        }
        let mut lines = Vec::new();
        let mut names: Vec<&String> = by_name.keys().collect();
        names.sort();
        for key in names {
            let ids = &by_name[key];
            let mut emails: Vec<(&String, usize)> = Vec::new();
            for (_, email, n) in ids {
                match emails.iter_mut().find(|(e, _)| e.eq_ignore_ascii_case(email)) {
                    Some(found) => found.1 += n,
                    None => emails.push((email, *n)),
                }
            }
            if emails.len() < 2 {
                continue;
            }
            emails.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            let name = ids
                .iter()
                .max_by_key(|(_, _, n)| *n)
                .map(|(name, _, _)| *name)
                .expect("one at least");
            for (email, _) in &emails[1..] {
                lines.push(format!("{name} <{}> <{email}>", emails[0].0));
            }
        }
        if !lines.is_empty() {
            text.push('\n');
            for line in lines.into_iter().take(40) {
                text.push_str(&line);
                text.push('\n');
            }
        }
        text
    }
}

// --- .git-blame-ignore-revs --------------------------------------------------------------------

impl Repo {
    fn check_ignore_revs(&self, text: &str, check: &mut FileCheck) {
        let local = self.local();
        let mut seen: HashMap<String, usize> = HashMap::new();
        for (no, line) in numbered(text) {
            // As git reads it: what follows `#` is a comment, and spaces around don't count.
            let content = line.split('#').next().unwrap_or("");
            let hash = content.trim();
            if hash.is_empty() {
                continue;
            }
            let at = content.find(hash).unwrap_or(0);
            let range = (at, at + hash.len());
            let hex = hash.bytes().all(|b| b.is_ascii_hexdigit());
            if hex && hash.len() == 40 {
                let lower = hash.to_lowercase();
                if let Some(first) = seen.get(&lower) {
                    check.warn(
                        no,
                        line,
                        range,
                        format!("Line {first} lists the same commit."),
                        Some(Fix::Replace {
                            label: "Remove Line".into(),
                            line: no,
                            text: None,
                        }),
                    );
                } else {
                    seen.insert(lower, no);
                }
                match gix::ObjectId::from_hex(hash.as_bytes())
                    .ok()
                    .and_then(|id| local.find_commit(id).ok())
                {
                    Some(commit) => check.note(no, commit_words(&commit), None),
                    None => {
                        check.warn(no, line, range, "No such commit here. Blame skips the line; the commit may be on a branch you haven’t fetched.", None);
                        check.note(no, "not in this repository", Some(Level::Warn));
                    }
                }
                continue;
            }
            let found = (hex && hash.len() >= 4 && hash.len() < 40)
                .then(|| local.rev_parse_single(hash).ok())
                .flatten()
                .and_then(|id| local.find_commit(id.detach()).ok());
            match found {
                Some(commit) => {
                    let full = commit.id.to_string();
                    check.error(
                        no,
                        line,
                        range,
                        "A short hash: `git blame` stops with an error for everyone who uses this file.",
                        Some(replace("Use Full Hash", no, &line.replacen(hash, &full, 1))),
                    );
                    check.note(no, commit_words(&commit), None);
                }
                None => check.error(
                    no,
                    line,
                    range,
                    "Not a full commit hash: `git blame` stops with an error for everyone who uses this file.",
                    None,
                ),
            }
        }
    }
}

/// A commit's summary and day, e.g. "Format all Rust code · 12 Sep 2026".
fn commit_words(commit: &gix::Commit<'_>) -> String {
    let message = commit.message_raw_sloppy();
    let summary = message
        .lines()
        .next()
        .unwrap_or_default()
        .to_str_lossy()
        .trim()
        .to_owned();
    match commit.time() {
        Ok(time) => format!("{summary} · {}", day(time.seconds)),
        Err(_) => summary,
    }
}

/// "12 Sep 2026" for seconds since 1970, in UTC.
fn day(seconds: i64) -> String {
    // Howard Hinnant's days-to-civil.
    let z = seconds.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!("{d} {} {y}", MONTHS[(m - 1) as usize])
}

// --- .lfsconfig and .gitmodules ----------------------------------------------------------------

/// A git-config line: a section header, a key with its value, or nothing.
enum ConfigLine<'a> {
    Section(String),
    Key {
        name: String,
        value: Option<&'a str>,
        at: usize,
    },
    Other,
}

/// Each line of a git-config file with the section it is in; `None` where git can't read it.
fn config_lines(text: &str) -> std::result::Result<Vec<(usize, &str, String, ConfigLine<'_>)>, usize> {
    if let Err(err) = <gix::config::File as std::str::FromStr>::from_str(text) {
        return Err(err.line_number());
    }
    let mut section = String::new();
    let mut out = Vec::new();
    for (no, line) in numbered(text) {
        let trimmed = line.trim_start();
        let at = line.len() - trimmed.len();
        let parsed = if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            ConfigLine::Other
        } else if let Some(head) = trimmed.strip_prefix('[') {
            let head = head.split(']').next().unwrap_or("");
            let name = match head.split_once(' ') {
                Some((name, sub)) => format!("{}.{}", name.to_lowercase(), sub.trim().trim_matches('"')),
                None => match head.split_once('.') {
                    Some((name, sub)) => format!("{}.{}", name.to_lowercase(), sub.to_lowercase()),
                    None => head.to_lowercase(),
                },
            };
            section = name.clone();
            ConfigLine::Section(name)
        } else {
            let (key, value) = match trimmed.split_once('=') {
                Some((k, v)) => (k.trim(), Some(v.trim().trim_matches('"'))),
                None => (trimmed.trim(), None),
            };
            ConfigLine::Key {
                name: key.to_lowercase(),
                value,
                at,
            }
        };
        out.push((no, line, section.clone(), parsed));
    }
    Ok(out)
}

/// What git-lfs reads from .lfsconfig; anything else there it leaves alone (git-lfs
/// `safeKeys` and the patterns beside it).
fn lfs_reads(full: &str) -> bool {
    const KEYS: &[&str] = &[
        "lfs.allowincompletepush",
        "lfs.fetchexclude",
        "lfs.fetchinclude",
        "lfs.gitprotocol",
        "lfs.locksverify",
        "lfs.pushurl",
        "lfs.skipdownloaderrors",
        "lfs.url",
    ];
    if KEYS.contains(&full) {
        return true;
    }
    let (first, rest) = full.split_once('.').unwrap_or((full, ""));
    (first == "lfs" && (rest.ends_with(".access") || rest.ends_with(".locksverify")))
        || (first == "remote" && (rest.ends_with(".lfsurl") || rest.ends_with(".lfspushurl")))
}

impl Repo {
    fn check_lfsconfig(&self, text: &str, check: &mut FileCheck) {
        let lines = match config_lines(text) {
            Ok(lines) => lines,
            Err(no) => {
                let line = text.split('\n').nth(no.saturating_sub(1)).unwrap_or("");
                check.error(
                    no,
                    line,
                    (0, line.trim_end().len()),
                    "Git can’t read this line, so git-lfs ignores the whole file.",
                    None,
                );
                return;
            }
        };
        for (no, line, section, parsed) in &lines {
            let ConfigLine::Key { name, value, at } = parsed else {
                continue;
            };
            let full = format!("{section}.{name}");
            let span = (*at, line.trim_end().len());
            if !lfs_reads(&full) {
                check.warn(
                    *no,
                    line,
                    span,
                    format!("git-lfs doesn’t read `{full}` from .lfsconfig; it belongs in git’s settings."),
                    None,
                );
                continue;
            }
            let is_url = full == "lfs.url"
                || full == "lfs.pushurl"
                || full.ends_with(".lfsurl")
                || full.ends_with(".lfspushurl");
            let Some(value) = value.filter(|_| is_url) else {
                continue;
            };
            let Some((scheme, rest)) = value.split_once("://") else {
                check.error(
                    *no,
                    line,
                    span,
                    "git-lfs needs an address that starts with `https://` or `ssh://`.",
                    None,
                );
                continue;
            };
            if !["https", "http", "ssh", "file"].contains(&scheme.to_lowercase().as_str()) {
                check.error(
                    *no,
                    line,
                    span,
                    "git-lfs needs an address that starts with `https://` or `ssh://`.",
                    None,
                );
                continue;
            }
            let host_part = rest.split('/').next().unwrap_or("");
            if let Some((user, _)) = host_part.rsplit_once('@')
                && user.contains(':')
            {
                let clean = value.replacen(&format!("{user}@"), "", 1);
                check.error(
                    *no,
                    line,
                    span,
                    "This puts a password in the repository for everyone who clones it.",
                    Some(replace("Remove the Password", *no, &line.replacen(value, &clean, 1))),
                );
            }
        }
        let attributes = crate::lfs::read_attributes(self.workdir()).unwrap_or_default();
        if crate::lfs::lfs_patterns(&attributes).is_empty()
            && lines.iter().any(|(_, _, _, p)| matches!(p, ConfigLine::Key { .. }))
        {
            check.warn(
                0,
                "",
                (0, 0),
                "No rule in .gitattributes sends files to Git LFS, so this file changes nothing yet.",
                None,
            );
        }
    }
}

fn check_gitmodules(text: &str, tree: Option<&Tree>, check: &mut FileCheck) {
    let lines = match config_lines(text) {
        Ok(lines) => lines,
        Err(no) => {
            let line = text.split('\n').nth(no.saturating_sub(1)).unwrap_or("");
            check.error(
                no,
                line,
                (0, line.trim_end().len()),
                "Git can’t read this line, so the submodule commands stop.",
                None,
            );
            return;
        }
    };
    // section → (header line, path, url)
    let mut modules: Vec<(String, usize, &str, Option<String>, bool)> = Vec::new();
    for (no, line, section, parsed) in &lines {
        match parsed {
            ConfigLine::Section(name) if name.starts_with("submodule.") => {
                modules.push((name.clone(), *no, line, None, false))
            }
            ConfigLine::Key { name, value, .. } => {
                if let Some(module) = modules.iter_mut().rev().find(|m| m.0 == *section) {
                    match name.as_str() {
                        "path" => module.3 = value.map(str::to_owned),
                        "url" => module.4 = value.is_some_and(|v| !v.is_empty()),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    for (name, no, line, path, url) in modules {
        let span = (0, line.trim_end().len());
        let name = name.trim_start_matches("submodule.");
        match path {
            None => check.warn(no, line, span, format!("Submodule `{name}` has no `path`."), None),
            Some(path) => {
                if let Some(tree) = tree
                    && !tree.gitlinks.contains(path.trim_end_matches('/'))
                {
                    check.warn(
                        no,
                        line,
                        span,
                        format!("No submodule at `{path}` in the repository."),
                        None,
                    );
                }
            }
        }
        if !url {
            check.warn(
                no,
                line,
                span,
                format!("Submodule `{name}` has no `url`, so it can’t be cloned."),
                None,
            );
        }
    }
}

// --- CODEOWNERS --------------------------------------------------------------------------------

fn owner_ok(owner: &str, gitlab: bool) -> bool {
    let name = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    if let Some(account) = owner.strip_prefix('@') {
        if gitlab {
            return account
                .strip_prefix('@')
                .map_or_else(|| account.split('/').all(name), name);
        }
        return match account.split_once('/') {
            Some((org, team)) => name(org) && name(team),
            None => name(account) && !account.contains(['.', '_']),
        };
    }
    owner.contains('@') && !owner.starts_with('@') && owner.rsplit('@').next().is_some_and(|d| d.contains('.'))
}

impl Repo {
    fn check_codeowners(&self, text: &str, gitlab: bool, tree: Option<&Tree>, check: &mut FileCheck) {
        let forge = if gitlab { "GitLab" } else { "GitHub" };
        let path = self.repo_file_path(RepoFileKind::Codeowners, gitlab);
        let others: Vec<&str> = RepoFileKind::Codeowners
            .places(gitlab)
            .iter()
            .copied()
            .filter(|p| *p != path && self.workdir().join(p).is_file())
            .collect();
        if let Some(other) = others.first() {
            check.warn(
                0,
                "",
                (0, 0),
                format!("`{other}` is there too; {forge} reads only `{path}`."),
                None,
            );
        }
        let mut defaults = false;
        for (no, line) in numbered(text) {
            let content = strip_owner_comment(line);
            let trimmed = content.trim();
            if trimmed.is_empty() {
                continue;
            }
            let start = content.len() - content.trim_start().len();
            let header = trimmed.strip_prefix('^').unwrap_or(trimmed);
            if header.starts_with('[') {
                if !gitlab {
                    check.error(
                        no,
                        line,
                        (start, content.trim_end().len()),
                        "Sections in `[ ]` are GitLab’s; GitHub skips this line.",
                        None,
                    );
                    continue;
                }
                let Some(close) = header.find(']') else {
                    check.error(
                        no,
                        line,
                        (start, content.trim_end().len()),
                        "The `[` is never closed.",
                        None,
                    );
                    continue;
                };
                let after = header[close + 1..].trim_start();
                let after = match after.strip_prefix('[').and_then(|a| a.split_once(']')) {
                    Some((count, rest)) if count.chars().all(|c| c.is_ascii_digit()) => rest,
                    _ => after,
                };
                defaults = !after.trim().is_empty();
                self.check_owner_words(no, line, content, after, gitlab, check);
                continue;
            }
            let pattern = trimmed.split_whitespace().next().unwrap_or("");
            let owners = &trimmed[pattern.len()..];
            if pattern.starts_with('!') {
                check.error(
                    no,
                    line,
                    (start, start + 1),
                    format!("{forge} doesn’t take `!` in CODEOWNERS and skips the line."),
                    None,
                );
                continue;
            }
            if !gitlab && (pattern.contains('[') && !pattern.contains("\\[")) {
                check.error(
                    no,
                    line,
                    (start, start + pattern.len()),
                    "GitHub doesn’t take `[ ]` ranges in CODEOWNERS and skips the line.",
                    None,
                );
                continue;
            }
            if self.check_owner_words(no, line, content, owners, gitlab, check) {
                continue;
            }
            if owners.trim().is_empty() && !defaults {
                check.note(no, "no owners", None);
                continue;
            }
            if let Some(tree) = tree {
                let files = tree
                    .tracked
                    .iter()
                    .filter(|p| crate::pulls::owner_pattern_matches(pattern, p))
                    .count();
                if files == 0 {
                    check.warn(
                        no,
                        line,
                        (start, start + pattern.len()),
                        "No committed file matches this.",
                        None,
                    );
                    check.note(no, "matches nothing", Some(Level::Warn));
                } else {
                    check.note(no, plural(files, "file", "files"), None);
                }
            }
        }
    }

    /// Errors for words that aren't owners; whether there were any.
    fn check_owner_words(
        &self,
        no: usize,
        line: &str,
        content: &str,
        owners: &str,
        gitlab: bool,
        check: &mut FileCheck,
    ) -> bool {
        let base = content.len() - content.trim_start().len()
            + content
                .trim_start()
                .find(owners.trim_start())
                .filter(|_| !owners.trim().is_empty())
                .unwrap_or(0);
        let mut bad = false;
        let mut offset = 0;
        for word in owners.trim_start().split([' ', '\t']) {
            let at = base + offset;
            offset += word.len() + 1;
            if word.is_empty() || owner_ok(word, gitlab) {
                continue;
            }
            let how = if gitlab {
                "`@user`, `@group`, `@@role`"
            } else {
                "`@login`, `@org/team`"
            };
            check.error(
                no,
                line,
                (at, at + word.len()),
                format!("`{word}` is not an owner: write {how} or an e-mail."),
                None,
            );
            bad = true;
        }
        bad
    }
}

/// A CODEOWNERS line without its comment; `\#` is a `#` in a name.
fn strip_owner_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let end = (0..bytes.len())
        .find(|&i| bytes[i] == b'#' && (i == 0 || bytes[i - 1] != b'\\'))
        .unwrap_or(line.len());
    &line[..end]
}

// --- .editorconfig -----------------------------------------------------------------------------

/// Known properties and the values each takes (besides `unset`); none: a number.
const EDITORCONFIG: &[(&str, Option<&[&str]>)] = &[
    ("indent_style", Some(&["tab", "space"])),
    ("indent_size", None),
    ("tab_width", None),
    ("end_of_line", Some(&["lf", "cr", "crlf"])),
    (
        "charset",
        Some(&["latin1", "utf-8", "utf-8-bom", "utf-16be", "utf-16le"]),
    ),
    ("trim_trailing_whitespace", Some(&["true", "false"])),
    ("insert_final_newline", Some(&["true", "false"])),
    ("max_line_length", None),
    ("spelling_language", Some(&[])),
];

/// An .editorconfig section's glob as a regular expression over paths from the top.
fn editorconfig_regex(glob: &str) -> Option<regex::Regex> {
    let anchored = glob.contains('/');
    let glob = glob.trim_start_matches('/');
    let mut out = String::from(if anchored { "^" } else { "^(?:.*/)?" });
    let chars: Vec<char> = glob.chars().collect();
    let mut i = 0;
    let mut braces = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '*' if chars.get(i + 1) == Some(&'*') => {
                out.push_str(".*");
                i += 1;
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            '[' => {
                let end = chars[i..].iter().position(|&c| c == ']')?;
                let class: String = chars[i + 1..i + end].iter().collect();
                let class = class.strip_prefix('!').map_or(class.clone(), |c| format!("^{c}"));
                out.push('[');
                out.push_str(&class.replace('\\', "\\\\"));
                out.push(']');
                i += end;
            }
            '{' => {
                let end = chars[i..].iter().position(|&c| c == '}')? + i;
                let inner: String = chars[i + 1..end].iter().collect();
                if let Some((a, b)) = inner.split_once("..")
                    && let (Ok(a), Ok(b)) = (a.parse::<i64>(), b.parse::<i64>())
                {
                    let (lo, hi) = (a.min(b), a.max(b));
                    if hi.checked_sub(lo).is_none_or(|count| count > 1000) {
                        return None;
                    }
                    out.push_str(&format!(
                        "(?:{})",
                        (lo..=hi).map(|n| n.to_string()).collect::<Vec<_>>().join("|")
                    ));
                    i = end;
                } else if inner.contains(',') {
                    out.push_str("(?:");
                    braces += 1;
                } else {
                    out.push_str(&regex::escape(&format!("{{{inner}}}")));
                    i = end;
                }
            }
            ',' if braces > 0 => out.push('|'),
            '}' if braces > 0 => {
                out.push(')');
                braces -= 1;
            }
            '\\' if i + 1 < chars.len() => {
                out.push_str(&regex::escape(&chars[i + 1].to_string()));
                i += 1;
            }
            c => out.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    out.push('$');
    regex::Regex::new(&out).ok()
}

fn check_editorconfig(text: &str, tree: Option<&Tree>, check: &mut FileCheck) {
    let mut in_section = false;
    for (no, line) in numbered(text) {
        let trimmed = line.trim();
        let start = line.len() - line.trim_start().len();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if let Some(head) = trimmed.strip_prefix('[') {
            in_section = true;
            let Some(glob) = head.strip_suffix(']') else {
                check.error(
                    no,
                    line,
                    (start, line.trim_end().len()),
                    "The `[` is never closed.",
                    Some(replace("Add ]", no, &format!("{}]", line.trim_end()))),
                );
                continue;
            };
            if glob.matches('{').count() != glob.matches('}').count() {
                check.error(
                    no,
                    line,
                    (start, line.trim_end().len()),
                    "A `{` here is never closed, so no file matches.",
                    None,
                );
                continue;
            }
            let Some(regex) = editorconfig_regex(glob) else {
                check.error(
                    no,
                    line,
                    (start, line.trim_end().len()),
                    "Editors can’t read this section’s pattern.",
                    None,
                );
                continue;
            };
            if let Some(tree) = tree {
                let files = tree.count_files(format!("editorconfig:{glob}"), |p| regex.is_match(p));
                if files == 0 {
                    check.warn(
                        no,
                        line,
                        (start, line.trim_end().len()),
                        format!("No file matches `[{glob}]`."),
                        None,
                    );
                    check.note(no, "matches nothing", Some(Level::Warn));
                } else {
                    check.note(no, plural(files, "file", "files") + tree.more(), None);
                }
            }
            continue;
        }
        let (key, value) = match trimmed.split_once(['=', ':']) {
            Some((k, v)) => (k.trim().to_lowercase(), v.trim().to_lowercase()),
            None => {
                check.error(
                    no,
                    line,
                    (start, line.trim_end().len()),
                    "A property needs a value: `name = value`.",
                    None,
                );
                continue;
            }
        };
        let span = (start, line.trim_end().len());
        if key == "root" {
            if in_section {
                check.error(no, line, span, "`root` goes at the top, before any section.", None);
            } else if value != "true" && value != "false" {
                check.error(no, line, span, "`root` is `true` or `false`.", None);
            }
            continue;
        }
        if !in_section {
            check.error(no, line, span, "Properties go under a section, such as `[*]`.", None);
            continue;
        }
        let Some((_, values)) = EDITORCONFIG.iter().find(|(k, _)| *k == key) else {
            check.note(no, "not a standard property", None);
            continue;
        };
        if value == "unset" {
            continue;
        }
        let ok = match values {
            Some([]) => true,
            Some(values) => values.contains(&value.as_str()),
            None => {
                value.parse::<u32>().is_ok_and(|n| n > 0)
                    || (key == "indent_size" && value == "tab")
                    || (key == "max_line_length" && value == "off")
            }
        };
        if !ok {
            let takes = match values {
                Some(values) => values.iter().map(|v| format!("`{v}`")).collect::<Vec<_>>().join(", "),
                None if key == "indent_size" => "a number or `tab`".to_owned(),
                None if key == "max_line_length" => "a number or `off`".to_owned(),
                None => "a number".to_owned(),
            };
            check.error(no, line, span, format!("`{key}` takes {takes}."), None);
        }
    }
}

// --- New files ---------------------------------------------------------------------------------

impl Repo {
    /// What a new file starts with: what it is for, in comments, and what this repository
    /// plainly needs.
    fn starter(&self, kind: RepoFileKind) -> String {
        let has = |name: &str| self.workdir().join(name).exists();
        match kind {
            RepoFileKind::Gitignore => {
                let mut text = String::from("# Files and folders git leaves out. A rule doesn't touch files already committed.\n\n# System files\n.DS_Store\nThumbs.db\n");
                if has("Cargo.toml") {
                    text.push_str("\n# Rust\n/target/\n");
                }
                if has("package.json") {
                    text.push_str("\n# Node\nnode_modules/\n");
                }
                if has("pyproject.toml") || has("setup.py") || has("requirements.txt") {
                    text.push_str("\n# Python\n__pycache__/\n.venv/\n");
                }
                text.push_str("\n# Secrets\n.env\n");
                text
            }
            RepoFileKind::Gitattributes => "# How git treats files: line endings, binary files, Git LFS.\n\n# Line endings as each system expects, stored as LF.\n* text=auto\n".into(),
            RepoFileKind::Mailmap => self.mailmap_starter(),
            RepoFileKind::BlameIgnoreRevs => "# Commits Blame looks past, such as reformatting: one full commit hash per line.\n# GitHub reads this file too; on your own machine:\n#   git config blame.ignoreRevsFile .git-blame-ignore-revs\n".into(),
            RepoFileKind::Lfsconfig => {
                let url = self
                    .remotes_info()
                    .ok()
                    .and_then(|r| r.into_iter().next())
                    .map(|r| r.fetch_url)
                    .filter(|u| u.starts_with("https://"))
                    .map(|u| format!("{}/info/lfs", u.trim_end_matches('/')))
                    .unwrap_or_else(|| "https://example.com/team/project.git/info/lfs".into());
                format!("# Where Git LFS keeps the large files, for everyone who clones the repository.\n[lfs]\n\turl = {url}\n")
            }
            RepoFileKind::Gitmodules => String::new(),
            RepoFileKind::Codeowners => "# Who is asked to review changes to each part of the repository.\n# A later line wins over an earlier one.\n#   *            @org/team\n#   /docs/       @login\n".into(),
            RepoFileKind::Editorconfig => "# Indentation and line endings, the same in every editor (editorconfig.org).\nroot = true\n\n[*]\nindent_style = space\nindent_size = 4\nend_of_line = lf\ncharset = utf-8\ntrim_trailing_whitespace = true\ninsert_final_newline = true\n\n[*.md]\ntrim_trailing_whitespace = false\n".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailmap_lines() {
        let line = parse_mailmap("Alex Morgan <alex@oxbow.dev> <alex@old.local>").unwrap();
        assert_eq!(line.name.as_deref(), Some("Alex Morgan"));
        assert_eq!(line.old_email.as_deref(), Some("alex@old.local"));
        let line = parse_mailmap("Dana <dana@oxbow.dev> dana <DANA@oxbow.dev>").unwrap();
        assert_eq!(line.old_name.as_deref(), Some("dana"));
        let (message, _, fixed) = parse_mailmap("Sam Ruiz sam@oxbow.dev").unwrap_err();
        assert!(message.contains("angle brackets"));
        assert_eq!(fixed.as_deref(), Some("Sam Ruiz <sam@oxbow.dev>"));
        assert!(parse_mailmap("<alex@oxbow.dev>").is_err());
        assert!(parse_mailmap("Lee <lee@oxbow.dev").is_err());
    }

    #[test]
    fn days() {
        assert_eq!(day(0), "1 Jan 1970");
        assert_eq!(day(1_700_000_000), "14 Nov 2023");
    }

    #[test]
    fn editorconfig_globs() {
        let any = editorconfig_regex("*.md").unwrap();
        assert!(any.is_match("README.md") && any.is_match("docs/a.md") && !any.is_match("a.mdx"));
        let set = editorconfig_regex("*.{js,ts}").unwrap();
        assert!(set.is_match("app/x.ts") && !set.is_match("x.rs"));
        let anchored = editorconfig_regex("lib/**.py").unwrap();
        assert!(anchored.is_match("lib/a/b.py") && !anchored.is_match("src/lib/b.py"));
    }

    #[test]
    fn owners() {
        assert!(owner_ok("@octocat", false) && owner_ok("@acme/core", false) && owner_ok("dev@acme.io", false));
        assert!(!owner_ok("octocat", false) && !owner_ok("@a/b/c", false));
        assert!(owner_ok("@group/sub/team", true) && owner_ok("@@developer", true));
    }
    #[test]
    fn rule_memo_bounds_bytes_and_keeps_recent_rules() {
        let mut memo = Memo::<usize>::default();
        for n in 0..200 {
            memo.insert(n.to_string(), n, 0);
        }
        assert_eq!(memo.entries.len(), Memo::<usize>::COUNT);
        assert!(memo.get("0").is_none());
        assert_eq!(memo.get("72"), Some(72));
        memo.insert("next".into(), 201, 0);
        assert_eq!(memo.get("72"), Some(72));
        assert!(memo.get("73").is_none());
        memo.insert("large".into(), 300, Memo::<usize>::LIMIT / 2);
        memo.insert("large2".into(), 301, Memo::<usize>::LIMIT / 2);
        assert!(memo.bytes <= Memo::<usize>::LIMIT);
        assert!(memo.get("large").is_none());
        memo.insert("oversize".into(), 0, Memo::<usize>::LIMIT);
        assert!(memo.get("oversize").is_none());
        assert_eq!(memo.get("large2"), Some(301));
    }
}
