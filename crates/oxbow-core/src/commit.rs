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
    /// Changes with a few lines of context, split into hunks; as many as the repository's
    /// [`DiffOptions`](crate::DiffOptions) say where they apply.
    Compact,
    /// Changes with this many lines of context.
    Lines(u32),
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
        let files = counted(&repo, self.commit_changes(&repo, &commit)?);
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
        let changes = self.commit_changes(&repo, &commit)?;
        self.diffs(&repo, changes, path, context)
    }

    /// Changed files of a commit against its first parent. A stash keeps its untracked files in a
    /// third commit; they are part of what it changes, so they come after its tracked files.
    fn commit_changes(&self, repo: &gix::Repository, commit: &gix::Commit<'_>) -> Result<Vec<Change>> {
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

    /// Changed files between two commits' trees, with line counts.
    pub fn tree_files(&self, from: &str, to: &str) -> Result<Vec<FileChange>> {
        let repo = self.local();
        let changes = tree_changes(&repo, Some(&from_tree(&repo, from)?), &from_tree(&repo, to)?)?;
        Ok(counted(&repo, changes))
    }

    /// Line diffs between two commits' trees, for all files or only `path`.
    pub fn tree_diff(&self, from: &str, to: &str, path: Option<&str>, context: DiffContext) -> Result<Vec<FileDiff>> {
        let repo = self.local();
        let changes = tree_changes(&repo, Some(&from_tree(&repo, from)?), &from_tree(&repo, to)?)?;
        self.diffs(&repo, changes, path, context)
    }

    fn diffs(
        &self,
        repo: &gix::Repository,
        changes: Vec<Change>,
        path: Option<&str>,
        context: DiffContext,
    ) -> Result<Vec<FileDiff>> {
        let options = self.diff_options();
        let context = match context {
            DiffContext::Compact => DiffContext::Lines(options.context_lines),
            other => other,
        };
        let mut out = Vec::new();
        for change in changes {
            if path.is_some_and(|p| p != change.file.path) {
                continue;
            }
            let mut file = change.file;
            let diff = match line_diff(repo, change.old, change.new, context, true, options.ignore_whitespace)? {
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

/// Files with their added and removed line counts.
fn counted(repo: &gix::Repository, changes: Vec<Change>) -> Vec<FileChange> {
    changes
        .into_iter()
        .map(|change| {
            let mut file = change.file;
            match line_diff(repo, change.old, change.new, DiffContext::Compact, false, false) {
                Ok(Some((hunks, _))) => count_lines(&mut file, &hunks),
                Ok(None) => file.binary = true,
                // Counts are a nicety; a blob that can't be read still lists the file.
                Err(_) => {}
            }
            file
        })
        .collect()
}

fn from_tree<'r>(repo: &'r gix::Repository, id: &str) -> Result<gix::Tree<'r>> {
    find_commit(repo, id)?.tree().map_err(Error::git)
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
    ignore_whitespace: bool,
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
    Ok(Some((
        diff_text_with(&before, &after, context, with_words, ignore_whitespace),
        false,
    )))
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
    let old: Vec<&str> = before.split_inclusive('\n').collect();
    let new: Vec<&str> = after.split_inclusive('\n').collect();
    // The squeezed texts have exactly one line per original line, so the diff's line numbers
    // point into `old` and `new` as they are.
    let squeezed = ignore_whitespace.then(|| (squeeze_whitespace(before), squeeze_whitespace(after)));
    let input = match &squeezed {
        Some((b, a)) => InternedInput::new(b.as_str(), a.as_str()),
        None => InternedInput::new(before, after),
    };
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    let changes: Vec<gix::diff::blob::Hunk> = diff.hunks().collect();
    if changes.is_empty() {
        return Vec::new();
    }

    let ctx = match context {
        DiffContext::Compact => CONTEXT_LINES,
        DiffContext::Lines(lines) => lines,
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
            header: hunk_header(old_start + 1, o - old_start, new_start + 1, n - new_start),
            old_start: old_start + 1,
            old_lines: o - old_start,
            new_start: new_start + 1,
            new_lines: n - new_start,
            lines,
            check: String::new(),
        });
    }
    hunks
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
}
