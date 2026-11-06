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

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gix::bstr::ByteSlice;
use serde::{Deserialize, Serialize};

use crate::cli::{GitCommand, literal};
use crate::error::{Error, Result};
use crate::repo::Repo;

mod attributes;
mod codeowners;
mod editorconfig;
mod gitmodules;
mod ignore;
mod ignorerevs;
mod lfsconfig;
mod mailmap;

use editorconfig::check_editorconfig;
use gitmodules::check_gitmodules;
use mailmap::{People, check_mailmap};

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

fn replace(label: &str, line: usize, text: &str) -> Fix {
    Fix::Replace {
        label: label.to_owned(),
        line,
        text: Some(text.to_owned()),
    }
}

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
