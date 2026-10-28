//! Splitting a commit: files or single lines picked from it become a commit of their own right
//! after it, and the rest keeps its place and message. Edit Stack carries it out like any other
//! plan, with its dry run and its force push.
//!
//! The two commits are made before the rebase, with `git commit-tree` from trees built in an
//! index of their own, so the working copy and the real index are never touched: the first has
//! the commit's other changes on its parent, the second the commit's own tree on top of that.
//! Nothing above the split changes, as the second commit has the tree the commit had. Made from
//! the same commit and picks, they come out the same, so the dry run and the rebase agree; the
//! rebase then commits each again, with today's committer and signature.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};

use gix::diff::blob::{Algorithm, Diff, InternedInput};
use serde::{Deserialize, Serialize};

use crate::cli::GitCommand;
use crate::commit::{DiffContext, FileChange, FileDiff, FileStatus, ModeChange, Source};
use crate::error::{Error, Result};
use crate::repo::Repo;

/// The id git gives a missing side of a change.
const NONE: &str = "0000000000000000000000000000000000000000";

/// Files with more changed lines than this are split whole, not line by line.
const MAX_LINES: usize = 20_000;

/// Some of a commit's changes taken out into a new commit right after it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackSplit {
    /// The commit split. It keeps its place and message with the changes not picked.
    pub id: String,
    /// The changes that go into the new commit.
    pub files: Vec<SplitFile>,
    /// The new commit's message.
    pub message: String,
}

/// A file of the split commit whose changes, or some of them, go into the new commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitFile {
    pub path: String,
    /// Only these changed lines; the whole file without them.
    #[serde(default)]
    pub lines: Option<SplitLines>,
}

/// Changed lines of a file, by their numbers in the diff of [`Repo::split_files`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitLines {
    /// Removed lines, by their number in the file before the commit.
    #[serde(default)]
    pub removed: Vec<u32>,
    /// Added lines, by their number in the file the commit made.
    #[serde(default)]
    pub added: Vec<u32>,
}

/// The id the new commit of a split has in a plan's steps, before it exists.
pub fn split_part(id: &str) -> String {
    format!("split:{id}")
}

/// The commit a step id of [`split_part`] belongs to.
pub fn split_of(step: &str) -> Option<&str> {
    step.strip_prefix("split:")
}

/// One file of a commit as `git diff --raw` lists it.
#[derive(Debug, Clone)]
struct Raw {
    path: String,
    old_mode: String,
    new_mode: String,
    old_id: String,
    new_id: String,
}

/// The commit being split, as `git log` gives it.
struct Original {
    parent: String,
    tree: String,
    message: String,
    /// GIT_AUTHOR_* and GIT_COMMITTER_* for its parts.
    env: Vec<(&'static str, String)>,
}

/// The two commits a split makes: its rest on the commit's parent, then the picked changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SplitCommits {
    pub rest: String,
    pub rest_tree: String,
    pub picked: String,
}

impl Repo {
    /// The changes of `commit` against its parent, file by file, as a split picks from them:
    /// no renames, every line exact. Binary files, links, submodules and very long diffs come
    /// without lines, to be picked whole.
    pub fn split_files(&self, commit: &str) -> Result<Vec<FileDiff>> {
        let original = self.original(commit)?;
        let raw = self.raw_changes(&original.parent, commit)?;
        raw.into_iter()
            .map(|r| {
                let status = if r.old_mode == "000000" {
                    FileStatus::Added
                } else if r.new_mode == "000000" {
                    FileStatus::Deleted
                } else {
                    FileStatus::Modified
                };
                let blob = |id: &str| (id != NONE).then(|| Source::Blob { id: id.to_owned() });
                let mut file = FileChange {
                    path: r.path.clone(),
                    status,
                    mode: (status == FileStatus::Modified && r.old_mode != r.new_mode).then(|| ModeChange {
                        old: r.old_mode.clone(),
                        new: r.new_mode.clone(),
                    }),
                    ..FileChange::default()
                };
                let mut diff = FileDiff {
                    hunks: Vec::new(),
                    too_large: false,
                    limited: false,
                    old: blob(&r.old_id),
                    new: blob(&r.new_id),
                    submodule: None,
                    file: FileChange::default(),
                };
                match self.texts(&r)? {
                    None => file.binary = !is_special(&r),
                    Some((before, after)) => {
                        match crate::commit::diff_text_within(
                            &before,
                            &after,
                            DiffContext::Compact,
                            true,
                            false,
                            MAX_LINES,
                        ) {
                            Ok(hunks) => {
                                for line in hunks.iter().flat_map(|h| &h.lines) {
                                    match line.kind {
                                        crate::commit::LineKind::Added => file.additions += 1,
                                        crate::commit::LineKind::Removed => file.deletions += 1,
                                        crate::commit::LineKind::Context => {}
                                    }
                                }
                                diff.hunks = hunks;
                            }
                            Err(over) => {
                                file.additions = over.additions;
                                file.deletions = over.deletions;
                                diff.too_large = true;
                            }
                        }
                    }
                }
                diff.file = file;
                Ok(diff)
            })
            .collect()
    }

    /// Make the two commits of `split`. Refused when nothing is picked, when everything is, or
    /// when a picked file or line is not one of the commit's changes.
    pub(crate) fn split_commits(&self, split: &StackSplit) -> Result<SplitCommits> {
        let original = self.original(&split.id)?;
        let raw = self.raw_changes(&original.parent, &split.id)?;
        let by_path: HashMap<&str, &Raw> = raw.iter().map(|r| (r.path.as_str(), r)).collect();
        if split.files.is_empty() {
            return Err(Error::Git("pick the changes that go into the new commit".into()));
        }
        if split.message.trim().is_empty() {
            return Err(Error::Git("type a message for the new commit".into()));
        }
        let mut seen = HashSet::new();
        let mut entries = String::new();
        for file in &split.files {
            let Some(r) = by_path.get(file.path.as_str()) else {
                return Err(Error::Git(format!("{} is not a file this commit changes", file.path)));
            };
            if !seen.insert(file.path.as_str()) {
                return Err(Error::Git(format!("{} is picked twice", file.path)));
            }
            let entry = match &file.lines {
                None => self.before_entry(r),
                Some(lines) => match self.texts(r)? {
                    None => {
                        return Err(Error::Git(format!(
                            "{} can only be split whole: it is not a text file",
                            file.path
                        )));
                    }
                    Some((before, after)) => match rest_text(&before, &after, lines)? {
                        // Every change of the file is picked: it goes whole.
                        None => self.before_entry(r),
                        Some(text) => {
                            let id = self
                                .run_with_input(
                                    &GitCommand::new(["hash-object", "-w", "--stdin", "--no-filters"]),
                                    Some(text.as_bytes()),
                                )?
                                .stdout
                                .trim()
                                .to_owned();
                            let mode = if r.new_mode == "000000" {
                                &r.old_mode
                            } else {
                                &r.new_mode
                            };
                            format!("{mode} {id}\t{}", r.path)
                        }
                    },
                },
            };
            entries.push_str(&entry);
            entries.push('\0');
        }

        let rest_tree = self.tree_with(&split.id, &entries)?;
        let parent_tree = self
            .run(&GitCommand::new([
                "rev-parse".to_owned(),
                "--verify".to_owned(),
                format!("{}^{{tree}}", original.parent),
            ]))?
            .stdout
            .trim()
            .to_owned();
        if rest_tree == parent_tree {
            return Err(Error::Git(
                "leave some of the changes in the commit: picking all of them moves nothing".into(),
            ));
        }
        if rest_tree == original.tree {
            return Err(Error::Git("pick at least one change for the new commit".into()));
        }
        let rest = self.commit_tree(&rest_tree, &original.parent, &original.message, &original.env)?;
        let picked = self.commit_tree(&original.tree, &rest, &split.message, &original.env)?;
        Ok(SplitCommits {
            rest,
            rest_tree,
            picked,
        })
    }

    /// `commit`'s parent, tree, message and people, and that it has a single parent.
    fn original(&self, commit: &str) -> Result<Original> {
        let out = self.run(&GitCommand::new([
            "log",
            "-1",
            "--date=raw",
            "--format=%P%x00%T%x00%an%x00%ae%x00%ad%x00%cn%x00%ce%x00%cd%x00%B",
            "--end-of-options",
            commit,
            "--",
        ]))?;
        let fields: Vec<&str> = out.stdout.splitn(9, '\0').collect();
        let [parents, tree, an, ae, ad, cn, ce, cd, message] = fields[..] else {
            return Err(Error::Git(format!("could not read the commit {commit}")));
        };
        let parents: Vec<&str> = parents.split_whitespace().collect();
        let [parent] = parents[..] else {
            return Err(Error::Git(if parents.is_empty() {
                "the first commit of the repository can't be split here".into()
            } else {
                "a merge commit can't be split".into()
            }));
        };
        Ok(Original {
            parent: parent.to_owned(),
            tree: tree.trim().to_owned(),
            message: message.trim_end().to_owned() + "\n",
            env: vec![
                ("GIT_AUTHOR_NAME", an.to_owned()),
                ("GIT_AUTHOR_EMAIL", ae.to_owned()),
                ("GIT_AUTHOR_DATE", ad.to_owned()),
                ("GIT_COMMITTER_NAME", cn.to_owned()),
                ("GIT_COMMITTER_EMAIL", ce.to_owned()),
                ("GIT_COMMITTER_DATE", cd.to_owned()),
            ],
        })
    }

    /// The files `commit` changes against `parent`, without renames.
    fn raw_changes(&self, parent: &str, commit: &str) -> Result<Vec<Raw>> {
        let out = self.run(&GitCommand::new([
            "diff",
            "--raw",
            "-z",
            "--no-renames",
            "--no-abbrev",
            "--no-ext-diff",
            "--no-textconv",
            parent,
            commit,
            "--",
        ]))?;
        // `:<old mode> <new mode> <old id> <new id> <status>\0<path>\0`
        let mut fields = out.stdout.split('\0');
        let mut raw = Vec::new();
        while let (Some(head), Some(path)) = (fields.next(), fields.next()) {
            let mut parts = head.trim_start_matches(['\n', ':']).split(' ');
            let (Some(old_mode), Some(new_mode), Some(old_id), Some(new_id)) =
                (parts.next(), parts.next(), parts.next(), parts.next())
            else {
                break;
            };
            raw.push(Raw {
                path: path.to_owned(),
                old_mode: old_mode.to_owned(),
                new_mode: new_mode.to_owned(),
                old_id: old_id.to_owned(),
                new_id: new_id.to_owned(),
            });
        }
        Ok(raw)
    }

    /// Both sides of a file as text, or `None` for a binary file, a link or a submodule.
    fn texts(&self, r: &Raw) -> Result<Option<(String, String)>> {
        if is_special(r) {
            return Ok(None);
        }
        let read = |id: &str| -> Result<Option<String>> {
            if id == NONE {
                return Ok(Some(String::new()));
            }
            let bytes = self.read_source(&Source::Blob { id: id.to_owned() })?;
            if bytes[..bytes.len().min(8000)].contains(&0) {
                return Ok(None);
            }
            Ok(String::from_utf8(bytes).ok())
        };
        Ok(match (read(&r.old_id)?, read(&r.new_id)?) {
            (Some(before), Some(after)) => Some((before, after)),
            _ => None,
        })
    }

    /// The index entry that puts the file back as it was before the commit: `update-index
    /// --index-info` removes it with mode 0.
    fn before_entry(&self, r: &Raw) -> String {
        if r.old_mode == "000000" {
            format!("0 {NONE}\t{}", r.path)
        } else {
            format!("{} {}\t{}", r.old_mode, r.old_id, r.path)
        }
    }

    /// The tree of `commit` with the index `entries` (NUL-ended) written over it, built in an
    /// index file of its own.
    fn tree_with(&self, commit: &str, entries: &str) -> Result<String> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let name = format!(
            "oxbow/split-index-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let index = self.git_path(&name)?;
        if let Some(dir) = index.parent() {
            std::fs::create_dir_all(dir).map_err(|err| Error::Git(format!("could not split the commit: {err}")))?;
        }
        let index_env = index.to_string_lossy().into_owned();
        let with_index =
            |args: &[&str]| GitCommand::new(args.iter().copied()).env("GIT_INDEX_FILE", index_env.as_str());
        let result = (|| {
            self.run(&with_index(&["read-tree", commit]))?;
            self.run_with_input(
                &with_index(&["update-index", "-z", "--index-info"]),
                Some(entries.as_bytes()),
            )?;
            Ok(self.run(&with_index(&["write-tree"]))?.stdout.trim().to_owned())
        })();
        let _ = std::fs::remove_file(&index);
        result
    }

    /// A commit of `tree` on `parent` with `message`, by the people in `env`, unsigned: the
    /// rebase commits it again with today's committer and signature.
    fn commit_tree(&self, tree: &str, parent: &str, message: &str, env: &[(&'static str, String)]) -> Result<String> {
        let mut command = GitCommand::new(["commit-tree", "--no-gpg-sign", tree, "-p", parent, "-F", "-"]);
        for (key, value) in env {
            command = command.env(*key, value.as_str());
        }
        let message = message.trim_end().to_owned() + "\n";
        Ok(self
            .run_with_input(&command, Some(message.as_bytes()))?
            .stdout
            .trim()
            .to_owned())
    }
}

/// A link (120000) or a submodule (160000) on either side: taken whole.
fn is_special(r: &Raw) -> bool {
    [&r.old_mode, &r.new_mode]
        .iter()
        .any(|m| m.starts_with("12") || m.starts_with("16"))
}

/// The file with the changes of `after` that are not in `picked` made to `before`: what the
/// commit keeps of it. `None` when every change is picked. Removed and added lines go pair by
/// pair, as in the diff, so the lines left keep their places.
fn rest_text(before: &str, after: &str, picked: &SplitLines) -> Result<Option<String>> {
    let input = InternedInput::new(before, after);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    let changes: Vec<gix::diff::blob::Hunk> = diff.hunks().collect();
    drop(input);
    let old: Vec<&str> = before.split_inclusive('\n').collect();
    let new: Vec<&str> = after.split_inclusive('\n').collect();
    let removed: HashSet<u32> = picked.removed.iter().copied().collect();
    let added: HashSet<u32> = picked.added.iter().copied().collect();
    let (mut found, mut all) = (0usize, true);
    let mut out = String::with_capacity(before.len().max(after.len()));
    // A line that ends the file without a line break gets one when another line follows it.
    let push = |out: &mut String, line: &str| {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(line);
    };
    let mut at = 0usize;
    for change in &changes {
        for line in &old[at..change.before.start as usize] {
            push(&mut out, line);
        }
        let gone: Vec<u32> = change.before.clone().collect();
        let came: Vec<u32> = change.after.clone().collect();
        for n in 0..gone.len().max(came.len()) {
            if let Some(&i) = gone.get(n) {
                if removed.contains(&(i + 1)) {
                    found += 1;
                    // The removal goes into the new commit: here the line stays.
                    push(&mut out, old[i as usize]);
                } else {
                    all = false;
                }
            }
            if let Some(&i) = came.get(n) {
                if added.contains(&(i + 1)) {
                    found += 1;
                } else {
                    all = false;
                    push(&mut out, new[i as usize]);
                }
            }
        }
        at = change.before.end as usize;
    }
    for line in &old[at..] {
        push(&mut out, line);
    }
    if found != removed.len() + added.len() {
        return Err(Error::Git(
            "a picked line is not a change of this commit any more: reload and pick again".into(),
        ));
    }
    if found == 0 {
        return Err(Error::Git("pick at least one changed line".into()));
    }
    Ok((!all).then_some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(removed: &[u32], added: &[u32]) -> SplitLines {
        SplitLines {
            removed: removed.to_vec(),
            added: added.to_vec(),
        }
    }

    #[test]
    fn rest_keeps_the_changes_not_picked() {
        let before = "a\nb\nc\nd\n";
        let after = "a\nB\nc\nD\ne\n";
        // b → B is picked; d → D and the new e stay.
        assert_eq!(
            rest_text(before, after, &lines(&[2], &[2])).unwrap().as_deref(),
            Some("a\nb\nc\nD\ne\n")
        );
        // Only the new line e is picked.
        assert_eq!(
            rest_text(before, after, &lines(&[], &[5])).unwrap().as_deref(),
            Some("a\nB\nc\nD\n")
        );
    }

    #[test]
    fn rest_pairs_removed_and_added_lines() {
        // -a -b +A +B with b/B picked keeps A then b, in their places.
        assert_eq!(
            rest_text("a\nb\n", "A\nB\n", &lines(&[2], &[2])).unwrap().as_deref(),
            Some("A\nb\n")
        );
    }

    #[test]
    fn every_change_picked_is_the_whole_file() {
        assert_eq!(rest_text("a\n", "b\n", &lines(&[1], &[1])).unwrap(), None);
    }

    #[test]
    fn a_line_that_is_no_change_is_refused() {
        assert!(rest_text("a\nb\n", "a\nc\n", &lines(&[1], &[])).is_err());
        assert!(rest_text("a\n", "b\n", &lines(&[], &[])).is_err());
    }

    #[test]
    fn a_last_line_without_a_break_gets_one_before_another() {
        // "a" without a line break becomes "a\n" and "b\n": keeping the old "a" and the new "b"
        // must not run them together.
        let rest = rest_text("a", "a\nb\n", &lines(&[1], &[1])).unwrap();
        assert_eq!(rest.as_deref(), Some("a\nb\n"));
    }

    #[test]
    fn split_ids_round_trip() {
        assert_eq!(split_of(&split_part("abc")), Some("abc"));
        assert_eq!(split_of("abc"), None);
    }
}
