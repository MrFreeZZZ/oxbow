use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use gix::ObjectId;
use gix::bstr::{BStr, ByteSlice};
use gix::diff::blob::{Algorithm, Diff, InternedInput};
use gix::object::tree::diff::ChangeDetached;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::repo::Repo;

/// Lines of unchanged context around each change in a compact diff.
pub const CONTEXT_LINES: u32 = 3;
/// Blobs larger than this are not diffed line by line.
const MAX_BLOB_BYTES: usize = 16 * 1024 * 1024;
/// Diff lines built for one file at most, even with no line limit in Settings (Show Diff Anyway):
/// 16 MiB of newlines would otherwise be 16 million lines, over a gigabyte.
const MAX_SHOWN_LINES: usize = 1_000_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitDetail {
    pub id: String,
    pub summary: String,
    /// Message without the summary line, trimmed.
    pub body: String,
    pub author: Person,
    pub committer: Person,
    pub parents: Vec<String>,
    /// Files changed compared to the first parent.
    pub files: Vec<FileChange>,
    /// A shallow clone stops here: the first parent isn't in it yet, so `files` is empty until a
    /// fetch brings it.
    pub shallow_end: bool,
    /// `files` have their line counts, sizes and what is unusual about them. Without, they are
    /// only the list of paths and how each changed, which needs no file read.
    pub counted: bool,
    /// The files before this one are not counted, though `counted` is set: the caller has
    /// their counts from their diffs.
    pub counted_from: usize,
}

/// Diffs of a commit's files, a few at a time: as many files from `from` on as fit a budget
/// of lines.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffPage {
    pub diffs: Vec<FileDiff>,
    /// Where the next page starts; `None` after the last file.
    pub next: Option<usize>,
    /// Changed files in all.
    pub files: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub name: String,
    pub email: String,
    /// Seconds since the Unix epoch.
    pub time: i64,
    /// Offset from UTC in seconds, as recorded in the commit.
    pub offset: i32,
}

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FileStatus {
    Added,
    Deleted,
    #[default]
    Modified,
    Renamed,
    Copied,
    /// A new file git does not track yet (working copy only).
    Untracked,
    /// A file with unresolved merge conflicts (working copy only).
    Conflicted,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    /// Previous path for renames and copies.
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub additions: u32,
    pub deletions: u32,
    pub binary: bool,
    /// Sizes in bytes of the file before and after, where known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_size: Option<u64>,
    /// The file mode changed, for example it became executable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<ModeChange>,
    /// For renames and copies: how much of the old file is left, in percent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity: Option<u32>,
    /// Only the line endings changed; the text is the same.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eol: Option<EolChange>,
    /// The file is kept in Git LFS, with the pointers before and after where they are known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lfs: Option<crate::lfs::LfsChange>,
    /// A submodule rather than a file: what changed is the commit recorded for it, or files in
    /// its folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submodule: Option<SubmoduleChange>,
    /// A new file whose lines were not counted this time (working copy only): many big new
    /// files are counted over a few reads, not all at once.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub uncounted: bool,
}

/// What changed about a submodule.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubmoduleChange {
    /// Another commit is recorded, or checked out in its folder.
    pub moved: bool,
    /// Its folder has uncommitted changes of its own (working copy only).
    pub changed: bool,
}

/// Old and new file mode, in git's octal form (`100644`, `100755`, `120000`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModeChange {
    pub old: String,
    pub new: String,
}

/// A file whose lines changed only in how they end.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EolChange {
    /// `CRLF`, `LF` or `mixed`.
    pub from: String,
    pub to: String,
    /// Lines whose ending changed.
    pub lines: u32,
}

/// Where the bytes of one side of a file diff can be read, to show an image or open a binary
/// file in another app.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Source {
    /// A blob in the object database.
    Blob { id: String },
    /// The file in the working tree.
    Worktree { path: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub file: FileChange,
    pub hunks: Vec<Hunk>,
    /// The file is too large to show line by line.
    pub too_large: bool,
    /// The diff has more changed lines than the limit in Settings, so it is left out until asked
    /// for; the counts in `file` are still right.
    pub limited: bool,
    /// The file before and after the change, where it exists.
    pub old: Option<Source>,
    pub new: Option<Source>,
    /// For a submodule: how its recorded commit moved, in place of line changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submodule: Option<crate::submodules::SubmoduleMove>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hunk {
    /// The `@@ -a,b +c,d @@` line, which also names the hunk when it is staged on its own.
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLine>,
    /// For uncommitted changes: a fingerprint of the hunk's exact bytes, line endings and "no
    /// newline at end of file" included. A hunk action sends it back, so a hunk that changed
    /// since it was shown is refused.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub check: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: LineKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    /// The line without its line ending.
    pub text: String,
    /// The line ends with a carriage return (CRLF), which `text` leaves out.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub cr: bool,
    /// For a removed line paired with an added line: the parts of the text, marking the changed words.
    pub words: Option<Vec<WordPart>>,
    /// An added line of the working copy that looks like it holds a secret: what it looks
    /// like, e.g. "GitHub token".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WordPart {
    pub text: String,
    pub changed: bool,
}

/// Which part of the full diff to compute.
#[derive(Debug, Clone, Copy)]
pub enum DiffContext {
    /// Changes with a few lines of context, split into hunks; as many as the repository's
    /// [`DiffOptions`](crate::DiffOptions) say where they apply.
    Compact,
    /// Changes with this many lines of context.
    Lines(u32),
    /// The whole file in one hunk.
    WholeFile,
}

#[derive(Clone)]
struct Change {
    file: FileChange,
    old: Option<ObjectId>,
    new: Option<ObjectId>,
}

/// The changed files of the commit asked for last: its file list, its counts and each page of
/// its diffs all start from them, and a commit's changes never change.
#[derive(Clone, Default)]
pub(crate) struct ChangesCache(Arc<Mutex<Option<ChangesOf>>>);

/// The changed files between the two trees compared last: Compare's file list, its counts and
/// each page of its diffs all start from them.
#[derive(Clone, Default)]
pub(crate) struct PairChangesCache(Arc<Mutex<Option<PairChanges>>>);

/// The trees of a comparison, old and new, and the files changed between them.
type PairChanges = ((ObjectId, ObjectId), Arc<Vec<Change>>);

/// A commit and its changed files, once read.
type ChangesOf = (ObjectId, Arc<ChangesSlot>);
type ChangesSlot = Mutex<Option<Arc<Vec<Change>>>>;

/// What reading both sides of a change found.
struct Inspected {
    /// `None` for binary content.
    hunks: Option<Vec<Hunk>>,
    too_large: bool,
    /// More changed lines than the limit: counted, but no lines were built.
    limited: bool,
}

impl Repo {
    /// Message, people, parents and changed files of a commit, with their line counts.
    pub fn commit_detail(&self, id: &str) -> Result<CommitDetail> {
        self.commit_detail_with(id, Some(0), &AtomicBool::new(false))
    }

    /// [`Repo::commit_detail`], with the changed files counted from the `count`th on, or not at
    /// all: the list alone comes at once even for a commit of thousands of files, as it reads
    /// none of them, and the files whose diffs are shown already have their counts. Setting
    /// `cancel` stops the counting: a newer question took its place.
    pub fn commit_detail_with(&self, id: &str, count: Option<usize>, cancel: &AtomicBool) -> Result<CommitDetail> {
        let repo = self.local();
        let commit = find_commit(&repo, id)?;
        let message = commit.message_raw_sloppy().to_str_lossy();
        let (summary, body) = match message.split_once('\n') {
            Some((summary, body)) => (summary.trim().to_owned(), body.trim().to_owned()),
            None => (message.trim().to_owned(), String::new()),
        };
        let person = |sig: gix::actor::SignatureRef<'_>| {
            let time = sig.time().unwrap_or_default();
            Person {
                name: sig.name.to_str_lossy().trim().to_owned(),
                email: sig.email.to_str_lossy().trim().to_owned(),
                time: time.seconds,
                offset: time.offset,
            }
        };
        let author = person(commit.author().map_err(Error::git)?);
        let committer = person(commit.committer().map_err(Error::git)?);
        let parents = commit.parent_ids().map(|p| p.to_string()).collect();
        let shallow_end = parent_missing(&repo, &commit);
        let options = self.diff_options();
        // A diff that ignores whitespace counts fewer lines than the list does.
        let counted_from = match count {
            Some(from) if !options.ignore_whitespace => from,
            _ => 0,
        };
        let files = if shallow_end {
            Vec::new()
        } else if count.is_some() {
            let changes = self.commit_changes(&repo, &commit)?;
            counted(&repo, &changes, counted_from, options.max_lines, cancel)?
        } else {
            self.commit_changes(&repo, &commit)?
                .iter()
                .map(|c| c.file.clone())
                .collect()
        };
        Ok(CommitDetail {
            id: commit.id.to_string(),
            summary,
            body,
            author,
            committer,
            parents,
            files,
            shallow_end,
            counted: count.is_some(),
            counted_from,
        })
    }

    /// Line diffs of a commit against its first parent, for all files or only `path`.
    pub fn commit_diff(&self, id: &str, path: Option<&str>, context: DiffContext) -> Result<Vec<FileDiff>> {
        self.commit_diff_with(id, path, context, &AtomicBool::new(false))
    }

    /// [`Repo::commit_diff`]; setting `cancel` stops it: a newer question took its place.
    pub fn commit_diff_with(
        &self,
        id: &str,
        path: Option<&str>,
        context: DiffContext,
        cancel: &AtomicBool,
    ) -> Result<Vec<FileDiff>> {
        let repo = self.local();
        let commit = find_commit(&repo, id)?;
        if parent_missing(&repo, &commit) {
            return Ok(Vec::new());
        }
        let changes = self.commit_changes(&repo, &commit)?;
        self.diffs(&repo, &changes, path, context, cancel)
    }

    /// The line diffs of a commit's files from the `from`th on, as many as fit `budget` lines
    /// (one file at least), for its Summary: a commit of thousands of files shows its first
    /// ones without diffing the others. Setting `cancel` stops it, between files.
    pub fn commit_diff_page(
        &self,
        id: &str,
        from: usize,
        budget: usize,
        context: DiffContext,
        cancel: &AtomicBool,
    ) -> Result<DiffPage> {
        let repo = self.local();
        let commit = find_commit(&repo, id)?;
        if parent_missing(&repo, &commit) {
            return Ok(DiffPage {
                diffs: Vec::new(),
                next: None,
                files: 0,
            });
        }
        let changes = self.commit_changes(&repo, &commit)?;
        self.page_of(&repo, &changes, from, budget, context, cancel)
    }

    /// The diffs of `changes` from the `from`th on, until they have `budget` lines.
    fn page_of(
        &self,
        repo: &gix::Repository,
        changes: &[Change],
        from: usize,
        budget: usize,
        context: DiffContext,
        cancel: &AtomicBool,
    ) -> Result<DiffPage> {
        let files = changes.len();
        let mut diffs = Vec::new();
        let mut lines = 0;
        let mut next = None;
        for (at, change) in changes.iter().enumerate().skip(from) {
            if !diffs.is_empty() && lines >= budget {
                next = Some(at);
                break;
            }
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            let diff = self.diff_one(repo, change.clone(), context, cancel)?;
            // A card (binary, too large, a picture) takes about as much room as a few lines.
            lines += diff.hunks.iter().map(|h| h.lines.len()).sum::<usize>().max(8);
            diffs.push(diff);
        }
        Ok(DiffPage { diffs, next, files })
    }

    /// Changed files of a commit against its first parent. A stash keeps its untracked files in a
    /// third commit; they are part of what it changes, so they come after its tracked files.
    fn commit_changes(&self, repo: &gix::Repository, commit: &gix::Commit<'_>) -> Result<Arc<Vec<Change>>> {
        let slot = {
            let mut cache = self.changes.0.lock().expect("changes lock");
            match &*cache {
                Some((id, slot)) if *id == commit.id => slot.clone(),
                _ => {
                    let slot = Arc::new(Mutex::new(None));
                    *cache = Some((commit.id, slot.clone()));
                    slot
                }
            }
        };
        // Questions about the same commit at once wait for the one reading its changes.
        let mut found = slot.lock().expect("changes lock");
        if let Some(changes) = &*found {
            return Ok(changes.clone());
        }
        let changes = Arc::new(self.read_commit_changes(repo, commit)?);
        *found = Some(changes.clone());
        Ok(changes)
    }

    fn read_commit_changes(&self, repo: &gix::Repository, commit: &gix::Commit<'_>) -> Result<Vec<Change>> {
        let mut out = changes(repo, commit)?;
        let id = commit.id.to_string();
        let untracked = self
            .stashes()?
            .into_iter()
            .find(|s| s.id == id)
            .and_then(|s| s.untracked);
        if let Some(untracked) = untracked {
            out.extend(tree_changes(repo, None, &from_tree(repo, &untracked)?)?);
        }
        Ok(out)
    }

    /// Changed files between two commits' trees (or two trees), with line counts.
    pub fn tree_files(&self, from: &str, to: &str) -> Result<Vec<FileChange>> {
        self.tree_files_with(from, to, Some(0), &AtomicBool::new(false))
    }

    /// [`Repo::tree_files`], counting lines from the `count`th file on (the ones before are
    /// counted by the diffs shown of them), or not at all without `count`. Setting `cancel`
    /// stops it.
    pub fn tree_files_with(
        &self,
        from: &str,
        to: &str,
        count: Option<usize>,
        cancel: &AtomicBool,
    ) -> Result<Vec<FileChange>> {
        let repo = self.local();
        let changes = self.pair_changes(&repo, from, to)?;
        match count {
            Some(at) => counted(&repo, &changes, at, self.diff_options().max_lines, cancel),
            None => Ok(changes.iter().map(|c| c.file.clone()).collect()),
        }
    }

    /// A page of [`Repo::tree_diff`] of every file: the diffs from the `from`th file on, until
    /// they have `budget` lines. Setting `cancel` stops it.
    pub fn tree_diff_page(
        &self,
        from_tree_id: &str,
        to: &str,
        from: usize,
        budget: usize,
        context: DiffContext,
        cancel: &AtomicBool,
    ) -> Result<DiffPage> {
        let repo = self.local();
        let changes = self.pair_changes(&repo, from_tree_id, to)?;
        self.page_of(&repo, &changes, from, budget, context, cancel)
    }

    /// Line diffs between two commits' trees (or two trees), for all files or only `path`.
    pub fn tree_diff(&self, from: &str, to: &str, path: Option<&str>, context: DiffContext) -> Result<Vec<FileDiff>> {
        self.tree_diff_with(from, to, path, context, &AtomicBool::new(false))
    }

    /// [`Repo::tree_diff`] that setting `cancel` stops.
    pub fn tree_diff_with(
        &self,
        from: &str,
        to: &str,
        path: Option<&str>,
        context: DiffContext,
        cancel: &AtomicBool,
    ) -> Result<Vec<FileDiff>> {
        let repo = self.local();
        let changes = self.pair_changes(&repo, from, to)?;
        self.diffs(&repo, &changes, path, context, cancel)
    }

    /// The changes between the trees of `from` and `to`, kept for the pair compared last.
    fn pair_changes(&self, repo: &gix::Repository, from: &str, to: &str) -> Result<Arc<Vec<Change>>> {
        let (old, new) = (from_tree(repo, from)?, from_tree(repo, to)?);
        let key = (old.id, new.id);
        if let Some((pair, changes)) = &*self.pair_changes.0.lock().expect("pair changes lock")
            && *pair == key
        {
            return Ok(changes.clone());
        }
        let changes = Arc::new(tree_changes(repo, Some(&old), &new)?);
        *self.pair_changes.0.lock().expect("pair changes lock") = Some((key, changes.clone()));
        Ok(changes)
    }

    fn diffs(
        &self,
        repo: &gix::Repository,
        changes: &[Change],
        path: Option<&str>,
        context: DiffContext,
        cancel: &AtomicBool,
    ) -> Result<Vec<FileDiff>> {
        changes
            .iter()
            .filter(|change| path.is_none_or(|p| p == change.file.path))
            .map(|change| self.diff_one(repo, change.clone(), context, cancel))
            .collect()
    }

    /// The line diff of one changed file. Setting `cancel` stops it.
    fn diff_one(
        &self,
        repo: &gix::Repository,
        change: Change,
        context: DiffContext,
        cancel: &AtomicBool,
    ) -> Result<FileDiff> {
        let options = self.diff_options();
        let context = match context {
            DiffContext::Compact => DiffContext::Lines(options.context_lines),
            other => other,
        };
        let mut file = change.file;
        if file.submodule.is_some() {
            let id = |id: Option<ObjectId>| id.map(|id| id.to_string());
            let moved = self.submodule_move(&file.path, id(change.old).as_deref(), id(change.new).as_deref(), 0);
            return Ok(FileDiff {
                file,
                hunks: Vec::new(),
                too_large: false,
                limited: false,
                old: None,
                new: None,
                submodule: Some(moved),
            });
        }
        let found = inspect(
            repo,
            &mut file,
            change.old,
            change.new,
            context,
            true,
            options.ignore_whitespace,
            options.max_lines,
            cancel,
        )?;
        let (hunks, too_large, limited) = (found.hunks.unwrap_or_default(), found.too_large, found.limited);
        let blob = |id: Option<ObjectId>| id.map(|id| Source::Blob { id: id.to_string() });
        Ok(FileDiff {
            file,
            hunks,
            too_large,
            limited,
            old: blob(change.old),
            new: blob(change.new),
            submodule: None,
        })
    }
}

/// More changed lines than `max` (0: no limit).
pub(crate) fn over_limit(file: &FileChange, max: u32) -> bool {
    max > 0 && file.additions + file.deletions > max
}

impl Repo {
    /// [`Repo::read_source`] of a side no bigger than `max` bytes, as a preview takes: a bigger
    /// one is not read, and the error says how big it is.
    pub fn read_source_within(&self, source: &Source, max: u64) -> Result<Vec<u8>> {
        if let Some(size) = self.source_size(source).filter(|&size| size > max) {
            return Err(Error::Git(format!(
                "it is {} MB, too big to preview here; Open shows it in its own app",
                size.div_ceil(1024 * 1024)
            )));
        }
        // A file that grew since it was measured is read no further than shows that.
        let data = self.read_source_upto(source, max.saturating_add(1))?;
        if data.len() as u64 > max {
            return Err(Error::Git(
                "it is too big to preview here; Open shows it in its own app".into(),
            ));
        }
        Ok(data)
    }

    /// The bytes of one side of a file diff.
    pub fn read_source(&self, source: &Source) -> Result<Vec<u8>> {
        self.read_source_upto(source, u64::MAX)
    }

    /// [`Repo::read_source`], reading at most `limit` bytes of a file in the working tree. An
    /// object never changes, so its size, checked beforehand, holds.
    fn read_source_upto(&self, source: &Source, limit: u64) -> Result<Vec<u8>> {
        match source {
            Source::Blob { id } => {
                let oid =
                    ObjectId::from_hex(id.as_bytes()).map_err(|_| Error::Git(format!("{id} is not an object id")))?;
                let repo = self.local();
                let object = repo.find_object(oid).map_err(Error::git)?;
                if object.kind != gix::object::Kind::Blob {
                    return Err(Error::Git(format!("{id} is not a file")));
                }
                Ok(object.detach().data)
            }
            Source::Worktree { path } => {
                let relative = std::path::Path::new(path);
                // Only a path inside the working tree.
                if !relative
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_)))
                {
                    return Err(Error::Git(format!("{path} is not a path in the repository")));
                }
                let full = self.workdir().join(relative);
                // A symlink's side is where it points, as git stores it, never what is there.
                if std::fs::symlink_metadata(&full).is_ok_and(|m| m.file_type().is_symlink()) {
                    return std::fs::read_link(&full)
                        .map(|target| target.to_string_lossy().into_owned().into_bytes())
                        .map_err(|err| Error::Git(format!("could not read {path}: {err}")));
                }
                crate::lfs::read_regular(&full, limit)
                    .ok_or_else(|| Error::Git(format!("could not read {path}: not a plain file")))
            }
        }
    }
}

impl Repo {
    /// The size of one side of a file diff, without reading it: the object's header, or the
    /// file's size on disk (where a symlink points, for a symlink). `None` when it is not there.
    pub fn source_size(&self, source: &Source) -> Option<u64> {
        match source {
            Source::Blob { id } => {
                let oid = ObjectId::from_hex(id.as_bytes()).ok()?;
                let header = self.local().find_header(oid).ok()?;
                (header.kind() == gix::object::Kind::Blob).then(|| header.size())
            }
            Source::Worktree { path } => {
                let relative = std::path::Path::new(path);
                if !relative
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_)))
                {
                    return None;
                }
                let full = self.workdir().join(relative);
                let meta = std::fs::symlink_metadata(&full).ok()?;
                if meta.file_type().is_symlink() {
                    return std::fs::read_link(&full).ok().map(|t| t.as_os_str().len() as u64);
                }
                meta.is_file().then_some(meta.len())
            }
        }
    }
}

fn find_commit<'r>(repo: &'r gix::Repository, id: &str) -> Result<gix::Commit<'r>> {
    let oid = ObjectId::from_hex(id.as_bytes()).map_err(|_| Error::UnknownCommit(id.to_owned()))?;
    repo.find_commit(oid).map_err(|_| Error::UnknownCommit(id.to_owned()))
}

fn count_lines(file: &mut FileChange, hunks: &[Hunk]) {
    file.additions = 0;
    file.deletions = 0;
    for line in hunks.iter().flat_map(|h| &h.lines) {
        match line.kind {
            LineKind::Added => file.additions += 1,
            LineKind::Removed => file.deletions += 1,
            LineKind::Context => {}
        }
    }
}

/// Files with their added and removed line counts, from the `from`th on: the ones before are
/// listed uncounted. Setting `cancel` stops it.
fn counted(
    repo: &gix::Repository,
    changes: &[Change],
    from: usize,
    max_lines: u32,
    cancel: &AtomicBool,
) -> Result<Vec<FileChange>> {
    changes
        .iter()
        .enumerate()
        .map(|(at, change)| {
            let mut file = change.file.clone();
            if at < from || file.submodule.is_some() {
                // A submodule records a commit of another repository: there are no lines to count.
                return Ok(file);
            }
            // Counts are a nicety; a blob that can't be read still lists the file.
            match inspect(
                repo,
                &mut file,
                change.old,
                change.new,
                DiffContext::Compact,
                false,
                false,
                max_lines,
                cancel,
            ) {
                Err(Error::Cancelled) => Err(Error::Cancelled),
                _ => Ok(file),
            }
        })
        .collect()
}

/// The tree of commit `id`, or `id` itself when it is a tree, as an Operation Log snapshot is.
fn from_tree<'r>(repo: &'r gix::Repository, id: &str) -> Result<gix::Tree<'r>> {
    let oid = ObjectId::from_hex(id.as_bytes()).map_err(|_| Error::UnknownCommit(id.to_owned()))?;
    repo.find_object(oid)
        .map_err(|_| Error::UnknownCommit(id.to_owned()))?
        .peel_to_tree()
        .map_err(Error::git)
}

/// Whether the commit's first parent is missing, as where a shallow clone stops.
fn parent_missing(repo: &gix::Repository, commit: &gix::Commit<'_>) -> bool {
    commit
        .parent_ids()
        .next()
        .is_some_and(|parent| !repo.has_object(parent))
}

/// Changed files of `commit` compared to its first parent (or to nothing for a root commit).
fn changes(repo: &gix::Repository, commit: &gix::Commit<'_>) -> Result<Vec<Change>> {
    let new_tree = commit.tree().map_err(Error::git)?;
    let old_tree = match commit.parent_ids().next() {
        Some(parent) => Some(
            parent
                .object()
                .map_err(Error::git)?
                .into_commit()
                .tree()
                .map_err(Error::git)?,
        ),
        None => None,
    };
    tree_changes(repo, old_tree.as_ref(), &new_tree)
}

/// Changed files between two trees; everything is added when there is no old one.
fn tree_changes(
    repo: &gix::Repository,
    old_tree: Option<&gix::Tree<'_>>,
    new_tree: &gix::Tree<'_>,
) -> Result<Vec<Change>> {
    let raw = repo.diff_tree_to_tree(old_tree, new_tree, None).map_err(Error::git)?;
    let path = |p: &BStr| p.to_str_lossy().into_owned();
    let mut out = Vec::new();
    for change in raw {
        if change.entry_mode().is_tree() {
            continue;
        }
        let gitlink = change.entry_mode().is_commit();
        let file = |p: &BStr, old_path: Option<String>, status| FileChange {
            path: path(p),
            old_path,
            status,
            submodule: gitlink.then_some(SubmoduleChange {
                moved: true,
                changed: false,
            }),
            ..FileChange::default()
        };
        let mode = |old: gix::object::tree::EntryMode, new: gix::object::tree::EntryMode| {
            (old != new).then(|| ModeChange {
                old: mode_text(old),
                new: mode_text(new),
            })
        };
        out.push(match change {
            ChangeDetached::Addition { location, id, .. } => Change {
                file: file(location.as_ref(), None, FileStatus::Added),
                old: None,
                new: Some(id),
            },
            ChangeDetached::Deletion { location, id, .. } => Change {
                file: file(location.as_ref(), None, FileStatus::Deleted),
                old: Some(id),
                new: None,
            },
            ChangeDetached::Modification {
                location,
                previous_id,
                previous_entry_mode,
                id,
                entry_mode,
                ..
            } => Change {
                file: FileChange {
                    mode: mode(previous_entry_mode, entry_mode),
                    ..file(location.as_ref(), None, FileStatus::Modified)
                },
                old: Some(previous_id),
                new: Some(id),
            },
            ChangeDetached::Rewrite {
                source_location,
                source_id,
                source_entry_mode,
                location,
                id,
                entry_mode,
                copy,
                ..
            } => Change {
                file: FileChange {
                    mode: mode(source_entry_mode, entry_mode),
                    ..file(
                        location.as_ref(),
                        Some(path(source_location.as_ref())),
                        if copy { FileStatus::Copied } else { FileStatus::Renamed },
                    )
                },
                old: Some(source_id),
                new: Some(id),
            },
        });
    }
    out.sort_by(|a, b| a.file.path.cmp(&b.file.path));
    Ok(out)
}

fn blob(repo: &gix::Repository, id: Option<ObjectId>) -> Result<Vec<u8>> {
    match id {
        // Submodule entries point at commits of another repository; show them as empty.
        Some(id) => match repo.find_object(id) {
            Ok(object) if object.kind == gix::object::Kind::Blob => Ok(object.detach().data),
            Ok(_) => Ok(Vec::new()),
            Err(err) => Err(Error::git(err)),
        },
        None => Ok(Vec::new()),
    }
}

/// The size of a blob from its header, 0 when there is none (or it is a submodule's commit).
fn blob_size(repo: &gix::Repository, id: Option<ObjectId>) -> u64 {
    id.and_then(|id| repo.find_header(id).ok())
        .filter(|h| h.kind() == gix::object::Kind::Blob)
        .map_or(0, |h| h.size())
}

/// Whether a blob looks binary from its first bytes, the way git decides, without reading the
/// rest: `git cat-file` streams it and is stopped once they are in.
fn starts_binary(repo: &gix::Repository, id: ObjectId) -> bool {
    use std::io::Read;
    use std::process::Stdio;
    let Ok(mut child) = crate::config::git()
        .arg("--git-dir")
        .arg(repo.git_dir())
        .args(["cat-file", "blob", &id.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let mut head = Vec::with_capacity(8000);
    if let Some(out) = child.stdout.take() {
        let _ = out.take(8000).read_to_end(&mut head);
    }
    let _ = child.kill();
    let _ = child.wait();
    is_binary(&head)
}

fn is_binary(data: &[u8]) -> bool {
    data[..data.len().min(8000)].contains(&0)
}

/// Read both sides of a change and diff them line by line, filling in what `file` can tell
/// about them: sizes, line counts, binary content, rename similarity and line-ending changes.
/// With more than `max_lines` changed lines (0: no limit) only the counts are worked out.
/// Setting `cancel` stops it before each costly step: reading the blobs, diffing them.
#[allow(clippy::too_many_arguments)]
fn inspect(
    repo: &gix::Repository,
    file: &mut FileChange,
    old: Option<ObjectId>,
    new: Option<ObjectId>,
    context: DiffContext,
    with_words: bool,
    ignore_whitespace: bool,
    max_lines: u32,
    cancel: &AtomicBool,
) -> Result<Inspected> {
    let stop = || {
        if cancel.load(Ordering::Relaxed) {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    };
    stop()?;
    // A blob too big to diff is not read at all, binary or not: its header gives the size.
    let sizes = (blob_size(repo, old), blob_size(repo, new));
    if sizes.0.max(sizes.1) > MAX_BLOB_BYTES as u64 {
        file.old_size = old.map(|_| sizes.0);
        file.new_size = new.map(|_| sizes.1);
        if matches!(file.status, FileStatus::Renamed | FileStatus::Copied) {
            file.similarity = Some(if old == new { 100 } else { 0 });
        }
        // Its first bytes still tell a picture or other binary file from big text.
        if [old, new].into_iter().flatten().any(|id| starts_binary(repo, id)) {
            file.binary = true;
            return Ok(Inspected {
                hunks: None,
                too_large: false,
                limited: false,
            });
        }
        return Ok(Inspected {
            hunks: Some(Vec::new()),
            too_large: true,
            limited: false,
        });
    }
    let before = blob(repo, old)?;
    let after = blob(repo, new)?;
    file.old_size = old.map(|_| before.len() as u64);
    file.new_size = new.map(|_| after.len() as u64);
    let pointers = (crate::lfs::parse_pointer(&before), crate::lfs::parse_pointer(&after));
    if pointers.0.is_some() || pointers.1.is_some() {
        // The sizes of the files LFS keeps, not of their pointers.
        file.old_size = pointers.0.as_ref().map(|p| p.size).or(file.old_size);
        file.new_size = pointers.1.as_ref().map(|p| p.size).or(file.new_size);
        file.lfs = Some(crate::lfs::LfsChange {
            old: pointers.0,
            new: pointers.1,
        });
    }
    if is_binary(&before) || is_binary(&after) {
        file.binary = true;
        if matches!(file.status, FileStatus::Renamed | FileStatus::Copied) {
            file.similarity = Some(if before == after { 100 } else { 0 });
        }
        return Ok(Inspected {
            hunks: None,
            too_large: false,
            limited: false,
        });
    }
    if before.len() > MAX_BLOB_BYTES || after.len() > MAX_BLOB_BYTES {
        return Ok(Inspected {
            hunks: Some(Vec::new()),
            too_large: true,
            limited: false,
        });
    }
    stop()?;
    let before = String::from_utf8_lossy(&before);
    let after = String::from_utf8_lossy(&after);
    let budget = if max_lines == 0 {
        MAX_SHOWN_LINES
    } else {
        MAX_SHOWN_LINES.min(max_lines as usize)
    };
    let (hunks, too_large, limited) =
        match diff_text_within(&before, &after, context, with_words, ignore_whitespace, budget) {
            Ok(hunks) => {
                count_lines(file, &hunks);
                file.eol = eol_change(&hunks);
                (hunks, false, false)
            }
            Err(Over { additions, deletions }) => {
                file.additions = additions;
                file.deletions = deletions;
                (Vec::new(), !over_limit(file, max_lines), over_limit(file, max_lines))
            }
        };
    if matches!(file.status, FileStatus::Renamed | FileStatus::Copied) {
        file.similarity = Some(similarity(&before, &after, file.deletions));
    }
    Ok(Inspected {
        hunks: Some(hunks),
        too_large,
        limited,
    })
}

/// Share of the old file's lines that are still there, in percent, measured against the longer side.
fn similarity(before: &str, after: &str, deletions: u32) -> u32 {
    let old = before.split_inclusive('\n').count() as u64;
    let new = after.split_inclusive('\n').count() as u64;
    let longest = old.max(new);
    if longest == 0 {
        return 100;
    }
    ((old.saturating_sub(u64::from(deletions)) * 100) / longest) as u32
}

/// When every change in `hunks` only swaps line endings, which way and on how many lines.
pub(crate) fn eol_change(hunks: &[Hunk]) -> Option<EolChange> {
    let (mut from_cr, mut from_lf, mut to_cr, mut to_lf, mut lines) = (0u32, 0u32, 0u32, 0u32, 0u32);
    for hunk in hunks {
        let mut i = 0;
        let l = &hunk.lines;
        while i < l.len() {
            if l[i].kind == LineKind::Context {
                i += 1;
                continue;
            }
            let removed = l[i..].iter().take_while(|x| x.kind == LineKind::Removed).count();
            let added = l[i + removed..]
                .iter()
                .take_while(|x| x.kind == LineKind::Added)
                .count();
            if removed == 0 || removed != added {
                return None;
            }
            for k in 0..removed {
                let (r, a) = (&l[i + k], &l[i + removed + k]);
                if r.text != a.text || r.cr == a.cr {
                    return None;
                }
                if r.cr {
                    from_cr += 1
                } else {
                    from_lf += 1
                }
                if a.cr {
                    to_cr += 1
                } else {
                    to_lf += 1
                }
                lines += 1;
            }
            i += removed + added;
        }
    }
    let name = |cr: u32, lf: u32| match (cr, lf) {
        (_, 0) => "CRLF",
        (0, _) => "LF",
        _ => "mixed",
    };
    (lines > 0).then(|| EolChange {
        from: name(from_cr, from_lf).to_owned(),
        to: name(to_cr, to_lf).to_owned(),
        lines,
    })
}

/// A tree entry mode as git prints it.
fn mode_text(mode: gix::object::tree::EntryMode) -> String {
    format!("{:06o}", mode.value())
}

/// Diff two texts into hunks.
pub fn diff_text(before: &str, after: &str, context: DiffContext, with_words: bool) -> Vec<Hunk> {
    diff_text_with(before, after, context, with_words, false)
}

/// The same line with every run of whitespace as one space and none at the ends, so lines that
/// differ only in whitespace compare equal.
fn squeeze_whitespace(text: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" ") + "\n")
        .collect()
}

/// Diff two texts into hunks, optionally treating lines that differ only in whitespace as equal.
pub fn diff_text_with(
    before: &str,
    after: &str,
    context: DiffContext,
    with_words: bool,
    ignore_whitespace: bool,
) -> Vec<Hunk> {
    diff_text_within(before, after, context, with_words, ignore_whitespace, usize::MAX).unwrap_or_default()
}

/// The changed lines of a diff that was not built: more than its budget.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Over {
    pub additions: u32,
    pub deletions: u32,
}

/// [`diff_text_with`], giving up before a single line is built when more than `budget` lines
/// would be: changed lines count against it, and so does their context.
pub(crate) fn diff_text_within(
    before: &str,
    after: &str,
    context: DiffContext,
    with_words: bool,
    ignore_whitespace: bool,
    budget: usize,
) -> std::result::Result<Vec<Hunk>, Over> {
    // The squeezed texts have exactly one line per original line, so the diff's line numbers
    // point into the original lines as they are.
    let squeezed = ignore_whitespace.then(|| (squeeze_whitespace(before), squeeze_whitespace(after)));
    let input = match &squeezed {
        Some((b, a)) => InternedInput::new(b.as_str(), a.as_str()),
        None => InternedInput::new(before, after),
    };
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    let changes: Vec<gix::diff::blob::Hunk> = diff.hunks().collect();
    if changes.is_empty() {
        return Ok(Vec::new());
    }

    let ctx = match context {
        DiffContext::Compact => CONTEXT_LINES,
        DiffContext::Lines(lines) => lines,
        DiffContext::WholeFile => u32::MAX / 4,
    };
    let deletions: usize = changes.iter().map(|c| c.before.len()).sum();
    let additions: usize = changes.iter().map(|c| c.after.len()).sum();
    // Context: at most `ctx` lines on each side of a change, and never more than the file has.
    let context_lines = (ctx as usize)
        .saturating_mul(2)
        .saturating_mul(changes.len())
        .min(input.before.len());
    if additions + deletions > budget || additions + deletions + context_lines > MAX_SHOWN_LINES {
        return Err(Over {
            additions: additions as u32,
            deletions: deletions as u32,
        });
    }
    drop(input);
    drop(squeezed);
    let old: Vec<&str> = before.split_inclusive('\n').collect();
    let new: Vec<&str> = after.split_inclusive('\n').collect();
    // Group changes whose context would touch or overlap.
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for (i, change) in changes.iter().enumerate() {
        match groups.last_mut() {
            Some((_, last)) if change.before.start - changes[*last].before.end <= ctx.saturating_mul(2) => *last = i,
            _ => groups.push((i, i)),
        }
    }

    let text = |line: &str| line.trim_end_matches('\n').trim_end_matches('\r').to_owned();
    let cr = |line: &str| line.trim_end_matches('\n').ends_with('\r');
    let mut hunks = Vec::with_capacity(groups.len());
    for (first, last) in groups {
        let start = &changes[first];
        let lead = ctx.min(start.before.start);
        let end = &changes[last];
        let tail = ctx.min(old.len() as u32 - end.before.end);
        let (mut o, mut n) = (start.before.start - lead, start.after.start - lead);
        let (old_start, new_start) = (o, n);
        let mut lines = Vec::new();
        let push_context = |lines: &mut Vec<DiffLine>, o: &mut u32, n: &mut u32, count: u32| {
            for _ in 0..count {
                lines.push(DiffLine {
                    kind: LineKind::Context,
                    old_line: Some(*o + 1),
                    new_line: Some(*n + 1),
                    text: text(old[*o as usize]),
                    cr: cr(new[*n as usize]),
                    words: None,
                    secret: None,
                });
                *o += 1;
                *n += 1;
            }
        };
        for change in &changes[first..=last] {
            let gap = change.before.start - o;
            push_context(&mut lines, &mut o, &mut n, gap);
            let mut removed: Vec<DiffLine> = change
                .before
                .clone()
                .map(|i| DiffLine {
                    kind: LineKind::Removed,
                    old_line: Some(i + 1),
                    new_line: None,
                    text: text(old[i as usize]),
                    cr: cr(old[i as usize]),
                    words: None,
                    secret: None,
                })
                .collect();
            let mut added: Vec<DiffLine> = change
                .after
                .clone()
                .map(|i| DiffLine {
                    kind: LineKind::Added,
                    old_line: None,
                    new_line: Some(i + 1),
                    text: text(new[i as usize]),
                    cr: cr(new[i as usize]),
                    words: None,
                    secret: None,
                })
                .collect();
            if with_words && removed.len() == added.len() {
                for (r, a) in removed.iter_mut().zip(added.iter_mut()) {
                    let (rw, aw) = word_diff(&r.text, &a.text);
                    r.words = Some(rw);
                    a.words = Some(aw);
                }
            }
            lines.append(&mut removed);
            lines.append(&mut added);
            o = change.before.end;
            n = change.after.end;
        }
        push_context(&mut lines, &mut o, &mut n, tail);
        hunks.push(Hunk {
            header: hunk_header(old_start + 1, o - old_start, new_start + 1, n - new_start),
            old_start: old_start + 1,
            old_lines: o - old_start,
            new_start: new_start + 1,
            new_lines: n - new_start,
            lines,
            check: String::new(),
        });
    }
    Ok(hunks)
}

/// The header git writes for a hunk: a count of 1 is left out, and an empty side starts one line earlier.
fn hunk_header(old_start: u32, old_lines: u32, new_start: u32, new_lines: u32) -> String {
    let range = |start: u32, lines: u32| match lines {
        0 => format!("{},0", start - 1),
        1 => start.to_string(),
        _ => format!("{start},{lines}"),
    };
    format!(
        "@@ -{} +{} @@",
        range(old_start, old_lines),
        range(new_start, new_lines)
    )
}

/// Split a removed/added line pair into unchanged and changed word runs.
pub(crate) fn word_diff(before: &str, after: &str) -> (Vec<WordPart>, Vec<WordPart>) {
    let input = InternedInput::new(
        gix::diff::blob::sources::words(before),
        gix::diff::blob::sources::words(after),
    );
    let diff = Diff::compute(Algorithm::Histogram, &input);
    let side = |tokens: &[gix::diff::blob::Token], removed: bool| {
        let mut parts: Vec<WordPart> = Vec::new();
        for (i, token) in tokens.iter().enumerate() {
            let changed = if removed {
                diff.is_removed(i as u32)
            } else {
                diff.is_added(i as u32)
            };
            let word = input.interner[*token];
            match parts.last_mut() {
                Some(last) if last.changed == changed => last.text.push_str(word),
                _ => parts.push(WordPart {
                    text: word.to_owned(),
                    changed,
                }),
            }
        }
        parts
    };
    (side(&input.before, true), side(&input.after, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hunk_headers_are_written_like_git_writes_them() {
        assert_eq!(hunk_header(3, 7, 3, 9), "@@ -3,7 +3,9 @@");
        assert_eq!(hunk_header(5, 1, 5, 1), "@@ -5 +5 @@");
        assert_eq!(hunk_header(1, 0, 1, 4), "@@ -0,0 +1,4 @@");
    }

    fn kinds(hunk: &Hunk) -> String {
        hunk.lines
            .iter()
            .map(|l| match l.kind {
                LineKind::Context => ' ',
                LineKind::Added => '+',
                LineKind::Removed => '-',
            })
            .collect()
    }

    #[test]
    fn compact_diff_keeps_three_lines_of_context() {
        let before: String = (1..=20).map(|i| format!("line {i}\n")).collect();
        let after = before.replace("line 10\n", "line ten\n");
        let hunks = diff_text(&before, &after, DiffContext::Compact, false);
        assert_eq!(hunks.len(), 1);
        let h = &hunks[0];
        assert_eq!((h.old_start, h.old_lines, h.new_start, h.new_lines), (7, 7, 7, 7));
        assert_eq!(kinds(h), "   -+   ");
        assert_eq!(h.lines[3].text, "line 10");
        assert_eq!(h.lines[4].new_line, Some(10));
    }

    #[test]
    fn far_apart_changes_make_separate_hunks() {
        let before: String = (1..=30).map(|i| format!("line {i}\n")).collect();
        let after = before
            .replace("line 2\n", "line two\n")
            .replace("line 28\n", "line 28!\n");
        let hunks = diff_text(&before, &after, DiffContext::Compact, false);
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].old_start, 1);
        assert_eq!(kinds(&hunks[0]), " -+   ");
        assert_eq!(kinds(&hunks[1]), "   -+  ");
    }

    #[test]
    fn whole_file_is_one_hunk() {
        let before: String = (1..=30).map(|i| format!("line {i}\n")).collect();
        let after = before
            .replace("line 2\n", "line two\n")
            .replace("line 28\n", "line 28!\n");
        let hunks = diff_text(&before, &after, DiffContext::WholeFile, false);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].lines.len(), 32);
    }

    #[test]
    fn changed_words_are_marked() {
        let hunks = diff_text("let timeout = 10;\n", "let timeout = 30;\n", DiffContext::Compact, true);
        let added = &hunks[0].lines[1];
        let parts = added.words.as_ref().unwrap();
        let changed: Vec<_> = parts.iter().filter(|p| p.changed).map(|p| p.text.as_str()).collect();
        assert_eq!(changed, ["30"]);
        let joined: String = parts.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(joined, "let timeout = 30;");
    }

    #[test]
    fn context_lines_can_be_chosen() {
        let before: String = (1..=20).map(|i| format!("line {i}\n")).collect();
        let after = before.replace("line 10\n", "line ten\n");
        let hunks = diff_text(&before, &after, DiffContext::Lines(1), false);
        assert_eq!(kinds(&hunks[0]), " -+ ");
        assert_eq!(hunks[0].old_start, 9);
    }

    #[test]
    fn whitespace_changes_can_be_ignored() {
        let before = "fn main() {\n    run();\n    stop();\n}\n";
        let after = "fn main() {\n\trun();  \n    halt();\n}\n";
        let all = diff_text_with(before, after, DiffContext::Compact, false, false);
        assert_eq!(kinds(&all[0]), " --++ ");
        let changed = diff_text_with(before, after, DiffContext::Compact, false, true);
        assert_eq!(kinds(&changed[0]), "  -+ ");
        assert_eq!(changed[0].lines[2].text, "    stop();");
    }

    #[test]
    fn new_file_is_all_additions() {
        let hunks = diff_text("", "a\nb\n", DiffContext::Compact, false);
        assert_eq!((hunks[0].old_start, hunks[0].old_lines, hunks[0].new_lines), (1, 0, 2));
        assert_eq!(kinds(&hunks[0]), "++");
    }

    #[test]
    fn a_diff_over_its_budget_is_only_counted() {
        let after = "\n".repeat(50);
        let over = diff_text_within("x\n", &after, DiffContext::Compact, false, false, 10);
        assert_eq!(
            over.err(),
            Some(Over {
                additions: 50,
                deletions: 1
            })
        );
        let within = diff_text_within("x\n", &after, DiffContext::Compact, false, false, 51).unwrap();
        assert_eq!(within[0].lines.len(), 51);
    }
}
