use gix::ObjectId;
use gix::bstr::{BStr, ByteSlice};
use gix::diff::blob::{Algorithm, Diff, InternedInput};
use gix::object::tree::diff::ChangeDetached;
use serde::Serialize;

use crate::error::{Error, Result};
use crate::repo::Repo;

/// Lines of unchanged context around each change in a compact diff.
pub const CONTEXT_LINES: u32 = 3;
/// Blobs larger than this are not diffed line by line.
const MAX_BLOB_BYTES: usize = 4 * 1024 * 1024;

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

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum FileStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
    Copied,
    /// A new file git does not track yet (working copy only).
    Untracked,
    /// A file with unresolved merge conflicts (working copy only).
    Conflicted,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    /// Previous path for renames and copies.
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub additions: u32,
    pub deletions: u32,
    pub binary: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub file: FileChange,
    pub hunks: Vec<Hunk>,
    /// The file is too large to show line by line.
    pub too_large: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLine>,
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
    /// For a removed line paired with an added line: the parts of the text, marking the changed words.
    pub words: Option<Vec<WordPart>>,
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
    /// Changes with a few lines of context, split into hunks.
    Compact,
    /// The whole file in one hunk.
    WholeFile,
}

struct Change {
    file: FileChange,
    old: Option<ObjectId>,
    new: Option<ObjectId>,
}

impl Repo {
    /// Message, people, parents and changed files of a commit.
    pub fn commit_detail(&self, id: &str) -> Result<CommitDetail> {
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
        let files = changes(&repo, &commit)?
            .into_iter()
            .map(|change| {
                let mut file = change.file;
                match line_diff(&repo, change.old, change.new, DiffContext::Compact, false) {
                    Ok(Some((hunks, _))) => count_lines(&mut file, &hunks),
                    Ok(None) => file.binary = true,
                    // Counts are a nicety; a blob that can't be read still lists the file.
                    Err(_) => {}
                }
                file
            })
            .collect();
        Ok(CommitDetail {
            id: commit.id.to_string(),
            summary,
            body,
            author,
            committer,
            parents,
            files,
        })
    }

    /// Line diffs of a commit against its first parent, for all files or only `path`.
    pub fn commit_diff(&self, id: &str, path: Option<&str>, context: DiffContext) -> Result<Vec<FileDiff>> {
        let repo = self.local();
        let commit = find_commit(&repo, id)?;
        let mut out = Vec::new();
        for change in changes(&repo, &commit)? {
            if path.is_some_and(|p| p != change.file.path) {
                continue;
            }
            let mut file = change.file;
            let diff = match line_diff(&repo, change.old, change.new, context, true)? {
                Some((hunks, too_large)) => {
                    count_lines(&mut file, &hunks);
                    FileDiff { file, hunks, too_large }
                }
                None => {
                    file.binary = true;
                    FileDiff {
                        file,
                        hunks: Vec::new(),
                        too_large: false,
                    }
                }
            };
            out.push(diff);
        }
        Ok(out)
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
    let raw = repo
        .diff_tree_to_tree(old_tree.as_ref(), &new_tree, None)
        .map_err(Error::git)?;
    let path = |p: &BStr| p.to_str_lossy().into_owned();
    let mut out = Vec::new();
    for change in raw {
        if change.entry_mode().is_tree() {
            continue;
        }
        let file = |p: &BStr, old_path: Option<String>, status| FileChange {
            path: path(p),
            old_path,
            status,
            additions: 0,
            deletions: 0,
            binary: false,
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
                id,
                ..
            } => Change {
                file: file(location.as_ref(), None, FileStatus::Modified),
                old: Some(previous_id),
                new: Some(id),
            },
            ChangeDetached::Rewrite {
                source_location,
                source_id,
                location,
                id,
                copy,
                ..
            } => Change {
                file: file(
                    location.as_ref(),
                    Some(path(source_location.as_ref())),
                    if copy { FileStatus::Copied } else { FileStatus::Renamed },
                ),
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

fn is_binary(data: &[u8]) -> bool {
    data[..data.len().min(8000)].contains(&0)
}

/// Line diff between two blobs. `None` for binary content; `Some((hunks, too_large))` otherwise.
fn line_diff(
    repo: &gix::Repository,
    old: Option<ObjectId>,
    new: Option<ObjectId>,
    context: DiffContext,
    with_words: bool,
) -> Result<Option<(Vec<Hunk>, bool)>> {
    let before = blob(repo, old)?;
    let after = blob(repo, new)?;
    if is_binary(&before) || is_binary(&after) {
        return Ok(None);
    }
    if before.len() > MAX_BLOB_BYTES || after.len() > MAX_BLOB_BYTES {
        return Ok(Some((Vec::new(), true)));
    }
    let before = String::from_utf8_lossy(&before);
    let after = String::from_utf8_lossy(&after);
    Ok(Some((diff_text(&before, &after, context, with_words), false)))
}

/// Diff two texts into hunks.
pub fn diff_text(before: &str, after: &str, context: DiffContext, with_words: bool) -> Vec<Hunk> {
    let old: Vec<&str> = before.split_inclusive('\n').collect();
    let new: Vec<&str> = after.split_inclusive('\n').collect();
    let input = InternedInput::new(before, after);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    let changes: Vec<gix::diff::blob::Hunk> = diff.hunks().collect();
    if changes.is_empty() {
        return Vec::new();
    }

    let ctx = match context {
        DiffContext::Compact => CONTEXT_LINES,
        DiffContext::WholeFile => u32::MAX / 4,
    };
    // Group changes whose context would touch or overlap.
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for (i, change) in changes.iter().enumerate() {
        match groups.last_mut() {
            Some((_, last)) if change.before.start - changes[*last].before.end <= ctx.saturating_mul(2) => *last = i,
            _ => groups.push((i, i)),
        }
    }

    let text = |line: &str| line.trim_end_matches('\n').trim_end_matches('\r').to_owned();
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
                    words: None,
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
                    words: None,
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
                    words: None,
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
            old_start: old_start + 1,
            old_lines: o - old_start,
            new_start: new_start + 1,
            new_lines: n - new_start,
            lines,
        });
    }
    hunks
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
    fn new_file_is_all_additions() {
        let hunks = diff_text("", "a\nb\n", DiffContext::Compact, false);
        assert_eq!((hunks[0].old_start, hunks[0].old_lines, hunks[0].new_lines), (1, 0, 2));
        assert_eq!(kinds(&hunks[0]), "++");
    }
}
