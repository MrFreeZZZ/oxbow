//! The working copy: what changed since the last commit, and its diffs.
//!
//! Status and diffs come from the `git` command line, so they honor every setting that affects
//! them (ignore rules, attributes, line endings, fsmonitor), and a hunk shown on screen is
//! exactly the hunk `git apply` gets when it is staged. Staging, committing and every other
//! change is a [`crate::Action`]: see action.rs, plan.rs and perform.rs.

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::cli::{GitCommand, literal};
use crate::commit::{
    DiffLine, DiffPage, FileChange, FileDiff, FileStatus, Hunk, LineKind, ModeChange, Source, SubmoduleChange,
    eol_change, over_limit, word_diff,
};
use crate::error::{Error, Result};
use crate::lfs::{self, LfsChange};
use crate::repo::Repo;

/// Files with more changed lines than this are not shown line by line.
const MAX_DIFF_LINES: usize = 20_000;

/// Untracked files bigger than this are not read to count their lines.
const MAX_COUNTED_BYTES: u64 = 8 * 1024 * 1024;
/// Bytes of new files a reading of the working tree reads at most to count their lines; the
/// files past them are counted by the readings after.
const MAX_FRESH_BYTES: u64 = 32 * 1024 * 1024;

/// What counting a new file's lines found, with the size and time of change it had then.
#[derive(Clone, Copy)]
struct Counted {
    len: u64,
    modified: std::time::SystemTime,
    binary: bool,
    additions: u32,
}

/// Lines counted in new files by path, kept while the files keep their size and time of change.
#[derive(Clone, Default)]
pub(crate) struct UntrackedCounts(std::sync::Arc<std::sync::Mutex<HashMap<String, Counted>>>);
/// Bytes of a file's patch read at most: a longer one is too large to show, as a longer blob is
/// in a commit's diff.
const MAX_PATCH_BYTES: usize = 16 * 1024 * 1024;
/// Files a page of the Summary's diffs takes at most, whatever its line budget: each is a path
/// on one `git diff` command line.
const PAGE_FILES: usize = 100;

/// Uncommitted changes, grouped the way they are staged.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkingTree {
    /// Changes in the index, ready to commit.
    pub staged: Vec<FileChange>,
    /// Changes in the working copy that are not staged, and untracked files.
    pub unstaged: Vec<FileChange>,
    /// Files with unresolved conflicts.
    pub conflicted: Vec<FileChange>,
    /// The changed lines of the files are counted, and which are binary or in LFS is known. A
    /// reading with the history lists the files first and counts them after: until then every
    /// file shows 0 lines, which is not what it has.
    pub counted: bool,
}

impl WorkingTree {
    pub fn is_empty(&self) -> bool {
        self.staged.is_empty() && self.unstaged.is_empty() && self.conflicted.is_empty()
    }

    /// Number of distinct files with any change.
    pub fn file_count(&self) -> usize {
        let mut paths: Vec<&str> = self
            .staged
            .iter()
            .chain(&self.unstaged)
            .chain(&self.conflicted)
            .map(|f| f.path.as_str())
            .collect();
        paths.sort_unstable();
        paths.dedup();
        paths.len()
    }
}

/// Which side of the working copy a diff or hunk belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    /// Index compared to `HEAD`.
    Staged,
    /// Working copy compared to the index.
    Unstaged,
}

impl Repo {
    /// Staged, unstaged, untracked and conflicted files, with their changed lines counted.
    pub fn working_tree(&self) -> Result<WorkingTree> {
        let mut tree = self.working_files()?;
        self.count_lines(&mut tree)?;
        Ok(tree)
    }

    /// The files of [`Repo::working_tree`] and their statuses alone, from `git status`: with
    /// thousands of edited files, counting their lines takes ten times as long as listing them.
    /// [`Repo::count_lines`] counts them after.
    pub fn working_files(&self) -> Result<WorkingTree> {
        let out = self.run(&GitCommand::new([
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
        ]))?;
        Ok(parse_status(&out.stdout))
    }

    /// Count the changed lines of every file of `tree`, a reading of [`Repo::working_files`],
    /// and tell binary, LFS and big files.
    pub fn count_lines(&self, tree: &mut WorkingTree) -> Result<()> {
        let staged = self.numstat(Side::Staged, &[])?;
        let unstaged = self.numstat(Side::Unstaged, &[])?;
        apply_stats(tree.staged.iter_mut(), &staged);
        apply_stats(tree.unstaged.iter_mut(), &unstaged);
        self.count_new_files(tree.unstaged.iter_mut(), true);
        self.mark_lfs_and_sizes(tree.staged.iter_mut().chain(tree.unstaged.iter_mut()));
        tree.counted = true;
        Ok(())
    }

    /// [`Repo::count_lines`] of a few `files` of one side, a page of diffs or a file shown, while
    /// the rest of the files are not counted yet: the paths go on the command line.
    pub(crate) fn count_some(&self, files: &mut [FileChange], side: Side) -> Result<()> {
        let paths: Vec<String> = files
            .iter()
            .filter(|f| !matches!(f.status, FileStatus::Untracked | FileStatus::Conflicted))
            .map(|f| literal(&f.path))
            .collect();
        if !paths.is_empty() {
            let stats = self.numstat(side, &paths)?;
            apply_stats(files.iter_mut(), &stats);
        }
        if side == Side::Unstaged {
            self.count_new_files(files.iter_mut(), false);
        }
        self.mark_lfs_and_sizes(files.iter_mut());
        Ok(())
    }

    /// Count the lines of the new files among `files`, reading each file unless it kept the size
    /// and time of change it was counted with. `all`: these are every new file, so the counts of
    /// files no longer there are let go.
    fn count_new_files<'a>(&self, files: impl Iterator<Item = &'a mut FileChange>, all: bool) {
        let known = if all {
            std::mem::take(&mut *self.untracked.0.lock().expect("counts lock"))
        } else {
            self.untracked.0.lock().expect("counts lock").clone()
        };
        let mut kept = HashMap::new();
        let mut budget = MAX_FRESH_BYTES;
        for file in files.filter(|f| f.status == FileStatus::Untracked) {
            let path = self.workdir().join(&file.path);
            // A symlink counts as one, as for git: what it points at may never end (/dev/zero).
            let Some(meta) = std::fs::symlink_metadata(&path).ok().filter(|m| m.is_file()) else {
                continue;
            };
            let len = meta.len();
            let modified = meta.modified().ok();
            // A file with the size and time of change it was counted with is not read again.
            if let Some(seen) = known
                .get(&file.path)
                .filter(|c| c.len == len && Some(c.modified) == modified)
            {
                file.binary = seen.binary;
                file.additions = seen.additions;
                kept.insert(file.path.clone(), *seen);
                continue;
            }
            // Its start says whether it is binary; only a text file is read whole, unless it is
            // too big or past this reading's budget.
            let mut start = Vec::new();
            let _ = std::fs::File::open(&path).and_then(|f| f.take(8000).read_to_end(&mut start));
            file.binary = start.contains(&0);
            if !file.binary {
                if len > MAX_COUNTED_BYTES {
                    continue;
                }
                if len > budget {
                    file.uncounted = true;
                    continue;
                }
                budget -= len;
                let data = crate::lfs::read_regular(&path, MAX_COUNTED_BYTES).unwrap_or_default();
                file.additions = line_count(&data);
            }
            // A file changed within the last moments may change again with the same size and
            // time: it is counted again next time.
            if let Some(modified) = modified.filter(|m| m.elapsed().is_ok_and(|age| age.as_secs() >= 2)) {
                let counted = Counted {
                    len,
                    modified,
                    binary: file.binary,
                    additions: file.additions,
                };
                kept.insert(file.path.clone(), counted);
            }
        }
        let mut counts = self.untracked.0.lock().expect("counts lock");
        if all {
            *counts = kept;
        } else {
            counts.extend(kept);
        }
    }

    /// Mark files `.gitattributes` sends to LFS, and give binary, LFS and big files their size in
    /// the working copy, for the badge in the file list and the note on a file too big for git.
    fn mark_lfs_and_sizes<'a>(&self, files: impl Iterator<Item = &'a mut FileChange>) {
        let mut files: Vec<&mut FileChange> = files.filter(|f| f.status != FileStatus::Conflicted).collect();
        let paths: Vec<&str> = files
            .iter()
            .filter(|f| f.status != FileStatus::Deleted)
            .map(|f| f.path.as_str())
            .collect();
        let lfs = self.lfs_paths(&paths);
        for file in files.iter_mut() {
            if file.status == FileStatus::Deleted {
                continue;
            }
            let tracked = lfs.contains(&file.path);
            if tracked {
                file.lfs = Some(LfsChange::default());
            }
            let Some((size, _)) = self.lfs_file_size(&file.path) else {
                continue;
            };
            if tracked || file.binary || size >= lfs::BIG_FILE {
                file.new_size = Some(size);
            }
        }
    }

    /// The messages of your latest commits on `HEAD`, newest first and each once, for ↑ in the
    /// commit message box: those whose author has your email (`user.email`), or anyone's when
    /// none is set. Merges are left out: git wrote their messages.
    pub fn recent_messages(&self, count: usize) -> Result<Vec<String>> {
        if self.head()?.commit.is_none() {
            return Ok(Vec::new());
        }
        let email = self
            .run(&GitCommand::new(["config", "user.email"]))
            .ok()
            .map(|out| out.stdout.trim().to_owned())
            .filter(|email| !email.is_empty());
        // Twice as many, for the ones said twice.
        let mut args = vec![
            "log".to_owned(),
            "-z".to_owned(),
            "--no-merges".to_owned(),
            "--format=%B".to_owned(),
            format!("-n{}", count * 2),
        ];
        if let Some(email) = email {
            args.extend(["--fixed-strings".to_owned(), format!("--author=<{email}>")]);
        }
        args.extend(["HEAD".to_owned(), "--".to_owned()]);
        let out = self.run(&GitCommand::new(args))?;
        let mut messages: Vec<String> = Vec::new();
        for message in out.stdout.split('\0').map(str::trim).filter(|m| !m.is_empty()) {
            if !messages.iter().any(|m| m == message) {
                messages.push(message.to_owned());
            }
            if messages.len() == count {
                break;
            }
        }
        Ok(messages)
    }

    /// The diff of one file on one side of the working copy.
    pub fn working_diff(&self, path: &str, side: Side, whole_file: bool) -> Result<FileDiff> {
        let tree = self.working_files()?;
        let list = match side {
            Side::Staged => &tree.staged,
            Side::Unstaged => &tree.unstaged,
        };
        let file = list
            .iter()
            .chain(&tree.conflicted)
            .find(|f| f.path == path)
            .cloned()
            .ok_or_else(|| Error::Git(format!("{path} has no {} changes", side.word())))?;
        // Only this file's lines are counted.
        let mut file = [file];
        self.count_some(&mut file, side)?;
        let [file] = file;
        self.diff_of(file, side, whole_file, &AtomicBool::new(false))
    }

    /// The diffs of the files on one side of the working copy, in the order of `paths`, for
    /// the Summary of Uncommitted changes. Paths without changes on that side are left out.
    pub fn working_diffs(&self, paths: &[String], side: Side, whole_file: bool) -> Result<Vec<FileDiff>> {
        self.working_diffs_in(&self.working_tree()?, paths, side, whole_file)
    }

    /// [`Repo::working_diffs`] of the files of `tree`, a reading of the working tree made a moment
    /// ago, so that git is not asked for every file's status again. The diffs themselves are read
    /// now; staging a hunk or lines checks it against the files as they are then.
    pub fn working_diffs_in(
        &self,
        tree: &WorkingTree,
        paths: &[String],
        side: Side,
        whole_file: bool,
    ) -> Result<Vec<FileDiff>> {
        let never = AtomicBool::new(false);
        let mut diffs = Vec::new();
        let mut from = Some(0);
        while let Some(at) = from {
            let page = self.working_diff_page(tree, paths, side, whole_file, at, usize::MAX, &never)?;
            diffs.extend(page.diffs);
            from = page.next;
        }
        Ok(diffs)
    }

    /// A page of [`Repo::working_diffs_in`]: the diffs of the files of `paths` from the `from`th
    /// on, until they have about `budget` lines or [`PAGE_FILES`] files. The patches of tracked
    /// files come from one `git diff`, and new files are read here, so a page of hundreds of
    /// small changes runs a few git commands, not one per file. Setting `cancel` stops it between
    /// commands.
    #[allow(clippy::too_many_arguments)]
    pub fn working_diff_page(
        &self,
        tree: &WorkingTree,
        paths: &[String],
        side: Side,
        whole_file: bool,
        from: usize,
        budget: usize,
        cancel: &AtomicBool,
    ) -> Result<DiffPage> {
        let list = match side {
            Side::Staged => &tree.staged,
            Side::Unstaged => &tree.unstaged,
        };
        // A path in both lists is the side's own change.
        let mut by_path: HashMap<&str, &FileChange> = tree.conflicted.iter().map(|f| (f.path.as_str(), f)).collect();
        by_path.extend(list.iter().map(|f| (f.path.as_str(), f)));
        let mut files: Vec<Cow<FileChange>> = paths
            .iter()
            .filter_map(|p| by_path.get(p.as_str()).map(|f| Cow::Borrowed(*f)))
            .collect();
        // Lines not counted yet: the files this page can take are counted first, as their
        // counts say which patches are too long to read.
        if !tree.counted {
            let start = from.min(files.len());
            let end = (start + PAGE_FILES).min(files.len());
            let mut part: Vec<FileChange> = files[start..end].iter().map(|f| f.clone().into_owned()).collect();
            self.count_some(&mut part, side)?;
            for (at, file) in part.into_iter().enumerate() {
                files[start + at] = Cow::Owned(file);
            }
        }
        let max_lines = self.diff_options().max_lines;
        let context = self.diff_options().context_lines as usize;
        // The files of one batch are guessed from their counts beforehand; the page itself stops
        // at the lines actually read, and files of the batch past that wait for the next page.
        let guess = |file: &FileChange| -> usize {
            if self.needs_no_patch(file, max_lines) || file.binary || file.submodule.is_some() {
                return 8;
            }
            let changed = (file.additions + file.deletions) as usize;
            // A new file not counted yet shows all its lines too.
            if whole_file || file.uncounted {
                // Every line of the file: about one in 16 bytes, unless it was all added or removed.
                let size = std::fs::symlink_metadata(self.workdir().join(&file.path)).map_or(0, |m| m.len());
                changed.max((size / 16) as usize) + 6
            } else {
                changed + 6 + 2 * context
            }
        };
        let mut end = from.min(files.len());
        let mut guessed = 0usize;
        while end < files.len() && (end == from || (guessed < budget && end - from < PAGE_FILES)) {
            guessed = guessed.saturating_add(guess(&files[end]));
            end += 1;
        }
        let page = &files[from.min(end)..end];
        let stopped = || cancel.load(Ordering::Relaxed);
        if stopped() {
            return Err(Error::Cancelled);
        }

        let mut patches: HashMap<String, String> = HashMap::new();
        let tracked: Vec<&str> = page
            .iter()
            .filter(|f| {
                f.submodule.is_none()
                    && !matches!(f.status, FileStatus::Untracked | FileStatus::Conflicted)
                    && !self.needs_no_patch(f, max_lines)
            })
            .map(|f| f.path.as_str())
            .collect();
        if !tracked.is_empty() {
            patches.extend(self.batch_patches(&tracked, side, whole_file, cancel)?);
        }
        let fresh: Vec<&FileChange> = page
            .iter()
            .map(|f| &**f)
            .filter(|f| f.status == FileStatus::Untracked && !self.needs_no_patch(f, max_lines))
            .collect();
        if !fresh.is_empty() {
            if stopped() {
                return Err(Error::Cancelled);
            }
            patches.extend(self.untracked_patches(&fresh, cancel)?);
        }

        let mut diffs = Vec::with_capacity(page.len());
        let mut lines = 0usize;
        let mut next = (end < files.len()).then_some(end);
        for (at, file) in page.iter().enumerate() {
            if !diffs.is_empty() && lines >= budget {
                next = Some(from + at);
                break;
            }
            if stopped() {
                return Err(Error::Cancelled);
            }
            let file = file.clone().into_owned();
            let diff = match patches.remove(&file.path) {
                Some(patch) => self.diff_from_patch(file, side, &patch, false),
                // Files outside the batch, or whose part of it git did not print as expected,
                // take a command of their own.
                None => self.diff_of(file, side, whole_file, cancel)?,
            };
            // A card (binary, too large, a submodule) takes about as much room as a few lines.
            lines += diff.hunks.iter().map(|h| h.lines.len()).sum::<usize>().max(8);
            diffs.push(diff);
        }
        Ok(DiffPage {
            diffs,
            next,
            files: files.len(),
        })
    }

    /// The patches of tracked `paths` on `side` from one `git diff`, by path. A path whose part
    /// is missing or cut short is left out, for a command of its own. Like every diff of the
    /// working copy, of the bytes themselves, not the text a `textconv` makes of them: a hunk
    /// staged or thrown away has to apply to the file.
    fn batch_patches(
        &self,
        paths: &[&str],
        side: Side,
        whole_file: bool,
        cancel: &AtomicBool,
    ) -> Result<HashMap<String, String>> {
        let context = self.context_arg(whole_file);
        let mut args: Vec<String> = [
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--full-index",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            &context,
        ]
        .map(str::to_owned)
        .to_vec();
        if side == Side::Staged {
            args.push("--cached".into());
        }
        args.push("--".into());
        args.extend(paths.iter().map(|p| literal(p)));
        let out = self.spawn_cancellable_within(&GitCommand::new(args), MAX_PATCH_BYTES, cancel)?;
        // A failed batch leaves every file to its own command, which says what went wrong.
        if !out.success && !out.cut {
            return Ok(HashMap::new());
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut parts = split_patches(&text);
        // The last part of a cut patch stops short.
        if out.cut {
            parts.pop();
        }
        let wanted: std::collections::HashSet<&str> = paths.iter().copied().collect();
        Ok(parts
            .into_iter()
            .filter(|(path, _)| wanted.contains(path.as_str()))
            .map(|(path, patch)| (path, patch.to_owned()))
            .collect())
    }

    /// The patches of new files, as `git diff --no-index /dev/null <file>` prints them, made from
    /// the files themselves. Files git would show another way (a symlink, an attribute that
    /// turns off or changes their diff, one that can't be read) are left out, for git itself.
    fn untracked_patches(&self, files: &[&FileChange], cancel: &AtomicBool) -> Result<HashMap<String, String>> {
        let mut args = vec![
            "check-attr".to_owned(),
            "-z".to_owned(),
            "diff".to_owned(),
            "--".to_owned(),
        ];
        args.extend(files.iter().map(|f| f.path.clone()));
        let out = match self.run_cancellable(&GitCommand::new(args), cancel) {
            Ok(out) => out,
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(_) => return Ok(HashMap::new()),
        };
        // `<path>\0diff\0<value>\0` for each path.
        let fields: Vec<&str> = out.stdout.split('\0').collect();
        let plain: std::collections::HashSet<&str> = fields
            .chunks(3)
            .filter(|f| f.len() == 3 && matches!(f[2], "unspecified" | "set"))
            .map(|f| f[0])
            .collect();
        let mut patches = HashMap::new();
        for file in files.iter().filter(|f| plain.contains(f.path.as_str())) {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            let path = self.workdir().join(&file.path);
            let Some(meta) = std::fs::symlink_metadata(&path).ok().filter(|m| m.is_file()) else {
                continue;
            };
            if let Some(data) = crate::lfs::read_regular(&path, MAX_COUNTED_BYTES) {
                patches.insert(file.path.clone(), untracked_patch(&file.path, &data, executable(&meta)));
            }
        }
        Ok(patches)
    }

    /// The `-U` option for a compact or whole-file diff.
    fn context_arg(&self, whole_file: bool) -> String {
        if whole_file {
            "-U100000000".to_owned()
        } else {
            format!("-U{}", self.diff_options().context_lines)
        }
    }

    /// Whether `file`'s lines are not shown, so its patch is never read: past the line limit, or
    /// a new file too big to have its lines counted.
    fn needs_no_patch(&self, file: &FileChange, max_lines: u32) -> bool {
        let changed = (file.additions + file.deletions) as usize;
        let uncounted = file.status == FileStatus::Untracked
            && std::fs::symlink_metadata(self.workdir().join(&file.path))
                .is_ok_and(|m| m.is_file() && m.len() > MAX_COUNTED_BYTES);
        !file.binary && file.lfs.is_none() && (over_limit(file, max_lines) || changed > MAX_DIFF_LINES || uncounted)
    }

    /// The diff of `file`, one entry of the working tree, on `side`.
    fn diff_of(&self, file: FileChange, side: Side, whole_file: bool, cancel: &AtomicBool) -> Result<FileDiff> {
        if let Some(sub) = file.submodule {
            return Ok(self.submodule_diff(file, sub, side));
        }
        // The counts of `git diff --numstat` say beforehand when no lines will be shown: the
        // patch is then never read, only the ids of the two sides. An untracked file too big to
        // have its lines counted is all added lines.
        let max_lines = self.diff_options().max_lines;
        if self.needs_no_patch(&file, max_lines) {
            let ids = self.blob_ids(&file, side).unwrap_or_default();
            let blob = |id: &Option<String>| id.clone().filter(|id| id.bytes().any(|b| b != b'0'));
            let old = blob(&ids.0).map(|id| Source::Blob { id });
            let new = match side {
                Side::Staged => blob(&ids.1).map(|id| Source::Blob { id }),
                Side::Unstaged => (file.status != FileStatus::Deleted).then(|| Source::Worktree {
                    path: file.path.clone(),
                }),
            };
            let limited = over_limit(&file, max_lines);
            return Ok(FileDiff {
                file,
                hunks: Vec::new(),
                too_large: !limited,
                limited,
                old,
                new,
                submodule: None,
            });
        }
        let (patch, cut) = self.raw_diff_within(&file, side, whole_file, MAX_PATCH_BYTES, cancel)?;
        Ok(self.diff_from_patch(file, side, &patch, cut))
    }

    /// The diff of `file` on `side` from its `patch`, which was `cut` short when too long.
    fn diff_from_patch(&self, file: FileChange, side: Side, patch: &str, cut: bool) -> FileDiff {
        let max_lines = self.diff_options().max_lines;
        // Past this many lines nothing is shown, so the rest of the patch is not turned into lines.
        let mut parsed = parse_patch_within(patch, MAX_DIFF_LINES);
        parsed.cut |= cut;
        let mut hunks: Vec<Hunk> = parsed.hunks.into_iter().map(|h| h.hunk).collect();
        // Lines the secrets check would stop a commit on get a mark before they are staged.
        if crate::secrets::check_secrets() && !parsed.cut {
            for line in hunks.iter_mut().flat_map(|h| &mut h.lines) {
                if line.kind == LineKind::Added {
                    line.secret = crate::secrets::check_line(&line.text).map(|(kind, _)| kind);
                }
            }
        }
        let mut file = FileChange {
            binary: file.binary || parsed.binary,
            mode: parsed.mode,
            ..file
        };
        // A patch cut short keeps the counts of `git diff --numstat`.
        if !file.binary && !parsed.cut {
            file.additions = 0;
            file.deletions = 0;
            for line in hunks.iter().flat_map(|h| &h.lines) {
                match line.kind {
                    LineKind::Added => file.additions += 1,
                    LineKind::Removed => file.deletions += 1,
                    LineKind::Context => {}
                }
            }
            file.eol = eol_change(&hunks);
        }
        // The blob ids of `index <old>..<new>`; all zeros stands for a side that does not exist.
        let blob = |id: &Option<String>| id.clone().filter(|id| id.bytes().any(|b| b != b'0'));
        let old = blob(&parsed.ids.0).map(|id| Source::Blob { id });
        let new = match side {
            Side::Staged => blob(&parsed.ids.1).map(|id| Source::Blob { id }),
            Side::Unstaged => (file.status != FileStatus::Deleted).then(|| Source::Worktree {
                path: file.path.clone(),
            }),
        };
        if file.lfs.is_some() {
            // What git diffs for an LFS file is its pointer: three short lines.
            let side_text = |skip: LineKind| -> String {
                hunks
                    .iter()
                    .flat_map(|h| &h.lines)
                    .filter(|l| l.kind != skip)
                    .map(|l| format!("{}\n", l.text))
                    .collect()
            };
            let lfs = LfsChange {
                old: lfs::parse_pointer(side_text(LineKind::Added).as_bytes()),
                new: lfs::parse_pointer(side_text(LineKind::Removed).as_bytes()),
            };
            file.old_size = lfs.old.as_ref().map(|p| p.size);
            if let Some(new) = &lfs.new {
                file.new_size = Some(new.size);
            }
            file.lfs = Some(lfs);
        } else if file.binary {
            file.old_size = old.as_ref().and_then(|s| self.source_size(s));
            file.new_size = new.as_ref().and_then(|s| self.source_size(s));
        }
        let limited = !parsed.cut && over_limit(&file, max_lines);
        let too_large = parsed.cut || (!limited && hunks.iter().map(|h| h.lines.len()).sum::<usize>() > MAX_DIFF_LINES);
        FileDiff {
            file,
            hunks: if too_large || limited { Vec::new() } else { hunks },
            too_large,
            limited,
            old,
            new,
            submodule: None,
        }
    }

    /// A submodule's entry on one side of the working copy: the commit the index records against
    /// `HEAD`'s (staged), or the one checked out in its folder against the index's (unstaged).
    fn submodule_diff(&self, file: FileChange, sub: SubmoduleChange, side: Side) -> FileDiff {
        let id = |spec: String| {
            self.run(&GitCommand::new(["rev-parse", "--verify", "-q", &spec]))
                .ok()
                .map(|out| out.stdout.trim().to_owned())
                .filter(|id| !id.is_empty())
        };
        let path = &file.path;
        let (old, new, changed) = match side {
            Side::Staged => (id(format!("HEAD:{path}")), id(format!(":{path}")), Some(0)),
            Side::Unstaged => {
                let folder = self.workdir().join(path);
                let current =
                    crate::cli::run_in(&folder, &GitCommand::new(["rev-parse", "--verify", "-q", "HEAD"]), None)
                        .ok()
                        .map(|out| out.stdout.trim().to_owned())
                        .filter(|id| !id.is_empty());
                let changed = if sub.changed {
                    crate::worktrees::changed_files(&folder).ok()
                } else {
                    Some(0)
                };
                (id(format!(":{path}")), current, changed)
            }
        };
        let moved = self.submodule_move(path, old.as_deref(), new.as_deref(), changed);
        FileDiff {
            file,
            hunks: Vec::new(),
            too_large: false,
            limited: false,
            old: None,
            new: None,
            submodule: Some(moved),
        }
    }

    /// A patch with the file header and only the hunk that starts with `header` and still has
    /// the bytes `check` was made from; with `lines`, only those changed lines of it, for applying `forward`
    /// (staging) or in reverse.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn hunk_patch(
        &self,
        path: &str,
        side: Side,
        header: &str,
        check: &str,
        lines: Option<&[usize]>,
        forward: bool,
    ) -> Result<String> {
        let tree = self.working_files()?;
        let list = match side {
            Side::Staged => &tree.staged,
            Side::Unstaged => &tree.unstaged,
        };
        let file = list
            .iter()
            .find(|f| f.path == path)
            .ok_or_else(|| Error::Git(format!("{path} has no {} changes any more", side.word())))?;
        if file.status == FileStatus::Untracked {
            return Err(Error::Git(format!("{path} is not tracked yet: stage the whole file")));
        }
        let parsed = parse_patch(&self.raw_diff(file, side, false)?);
        let hunk = parsed
            .hunks
            .iter()
            // The header gives only line numbers: an edit inside the hunk keeps them.
            .find(|h| h.header == header && h.hunk.check == check)
            .ok_or_else(|| Error::Git(format!("{path} changed since it was shown; refresh and try again")))?;
        let text = match lines {
            None => hunk.text.clone(),
            Some(lines) => partial_hunk(&hunk.text, lines, forward)
                .ok_or_else(|| Error::Git("pick at least one changed line".into()))?,
        };
        Ok(format!("{}{}", parsed.file_header, text))
    }

    /// The unstaged patch of `file`, a tracked file: its file header, and each hunk's text with
    /// the hunk, for the part of it a changelist holds. No hunks for a binary file.
    pub(crate) fn unstaged_hunks(&self, file: &FileChange) -> Result<(String, Vec<(String, Hunk)>)> {
        let parsed = parse_patch(&self.raw_diff(file, Side::Unstaged, false)?);
        if parsed.binary {
            return Ok((parsed.file_header, Vec::new()));
        }
        Ok((
            parsed.file_header,
            parsed.hunks.into_iter().map(|h| (h.text, h.hunk)).collect(),
        ))
    }

    /// `git diff` output for one file.
    fn raw_diff(&self, file: &FileChange, side: Side, whole_file: bool) -> Result<String> {
        let (patch, _) = self.raw_diff_within(file, side, whole_file, usize::MAX, &AtomicBool::new(false))?;
        Ok(patch)
    }

    /// [`Repo::raw_diff`], reading at most `max` bytes of the patch: whether it was cut there
    /// comes with it. A whole-file patch of a huge file is never read further than shown.
    fn raw_diff_within(
        &self,
        file: &FileChange,
        side: Side,
        whole_file: bool,
        max: usize,
        cancel: &AtomicBool,
    ) -> Result<(String, bool)> {
        let context = self.context_arg(whole_file);
        let mut args = vec![
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--full-index",
            &context,
        ];
        let path = literal(&file.path);
        let untracked = file.status == FileStatus::Untracked;
        if untracked {
            args.extend(["--no-index", "--", "/dev/null", &file.path]);
        } else {
            if side == Side::Staged {
                args.push("--cached");
            }
            args.extend(["--", &path]);
        }
        let command = GitCommand::new(args);
        let out = self.spawn_cancellable_within(&command, max, cancel)?;
        let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
        // `--no-index` exits with 1 when the files differ, which they always do here.
        if !out.success && !out.cut && !untracked {
            let (stdout, stderr) = (text(&out.stdout), text(&out.stderr));
            return Err(Error::Command {
                command: command.display(),
                code: out.code,
                output: if stderr.trim().is_empty() { stdout } else { stderr },
            });
        }
        Ok((text(&out.stdout), out.cut))
    }

    /// The blob ids of both sides of `file`'s change on `side`, as the `index` line of its patch
    /// gives them (all zeros for a side that has none), without reading the patch.
    fn blob_ids(&self, file: &FileChange, side: Side) -> Result<(Option<String>, Option<String>)> {
        if file.status == FileStatus::Untracked {
            return Ok((None, None));
        }
        let mut args = vec!["diff", "--raw", "-z", "--no-renames", "--no-abbrev"];
        if side == Side::Staged {
            args.push("--cached");
        }
        let path = literal(&file.path);
        args.extend(["--", &path]);
        let out = self.run(&GitCommand::new(args))?;
        // `:<old mode> <new mode> <old id> <new id> <status>\0<path>\0`
        let mut fields = out.stdout.split('\0').next().unwrap_or_default().split(' ').skip(2);
        Ok((fields.next().map(str::to_owned), fields.next().map(str::to_owned)))
    }

    /// Added and deleted lines per path, and whether the file is binary: of every file on
    /// `side`, or of `paths` (pathspecs) alone.
    fn numstat(&self, side: Side, paths: &[String]) -> Result<Stats> {
        let mut args: Vec<String> = ["diff", "--numstat", "-z", "--no-renames", "--no-textconv"]
            .map(str::to_owned)
            .to_vec();
        if side == Side::Staged {
            args.push("--cached".into());
            if self.head()?.commit.is_none() {
                // Nothing committed yet: compare the index with the empty tree.
                args.push("4b825dc642cb6eb9a060e54bf8d69288fbee4904".into());
            }
        }
        if !paths.is_empty() {
            args.push("--".into());
            args.extend(paths.iter().cloned());
        }
        let out = self.run(&GitCommand::new(args))?;
        let mut stats = HashMap::new();
        for record in out.stdout.split('\0').filter(|r| !r.is_empty()) {
            let mut parts = record.splitn(3, '\t');
            let (Some(add), Some(del), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
                continue;
            };
            let binary = add == "-";
            stats.insert(
                path.to_owned(),
                (add.parse().unwrap_or(0), del.parse().unwrap_or(0), binary),
            );
        }
        Ok(stats)
    }
}

/// What `git diff --numstat` says of each path: added and deleted lines, and whether it is binary.
type Stats = HashMap<String, (u32, u32, bool)>;

/// Give `files` their counts from `stats`; a file it leaves out keeps what it has.
fn apply_stats<'a>(files: impl Iterator<Item = &'a mut FileChange>, stats: &Stats) {
    for file in files {
        if let Some(&(add, del, binary)) = stats.get(&file.path) {
            file.additions = add;
            file.deletions = del;
            file.binary = binary;
        }
    }
}

impl Side {
    fn word(self) -> &'static str {
        match self {
            Side::Staged => "staged",
            Side::Unstaged => "unstaged",
        }
    }
}

/// A fingerprint of a hunk's text (FNV-1a), enough to notice that it changed.
pub(crate) fn fingerprint(text: &str) -> String {
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}{:x}", text.len())
}

fn change(path: &str, old_path: Option<&str>, status: FileStatus) -> FileChange {
    FileChange {
        path: path.to_owned(),
        old_path: old_path.map(str::to_owned),
        status,
        ..FileChange::default()
    }
}

fn status_of(code: char) -> Option<FileStatus> {
    Some(match code {
        'A' => FileStatus::Added,
        'D' => FileStatus::Deleted,
        'R' => FileStatus::Renamed,
        'C' => FileStatus::Copied,
        'M' | 'T' => FileStatus::Modified,
        _ => return None,
    })
}

/// Parse `git status --porcelain=v2 -z`.
fn parse_status(out: &str) -> WorkingTree {
    let mut tree = WorkingTree::default();
    let mut records = out.split('\0').filter(|r| !r.is_empty());
    while let Some(record) = records.next() {
        let mut fields = record.split(' ');
        match fields.next() {
            Some("1") | Some("2") => {
                let renamed = record.starts_with('2');
                let xy: Vec<char> = fields.next().unwrap_or("..").chars().collect();
                // 1 XY sub mH mI mW hH hI path / 2 XY sub mH mI mW hH hI score path\0orig
                let skip = if renamed { 7 } else { 6 };
                let path = record.splitn(skip + 3, ' ').nth(skip + 2).unwrap_or_default();
                let old_path = if renamed { records.next() } else { None };
                // `S<c><m><u>` for a submodule: its commit changed, tracked or untracked files in it.
                let sub: Vec<char> = fields.next().unwrap_or("N...").chars().collect();
                let gitlink = sub.first() == Some(&'S');
                if let Some(status) = status_of(xy[0]) {
                    let mut file = change(path, old_path, status);
                    if gitlink {
                        file.submodule = Some(SubmoduleChange {
                            moved: true,
                            changed: false,
                        });
                    }
                    tree.staged.push(file);
                }
                if let Some(status) = status_of(xy[1]) {
                    let mut file = change(path, None, status);
                    if gitlink {
                        file.submodule = Some(SubmoduleChange {
                            moved: sub.get(1) == Some(&'C'),
                            changed: sub.get(2) == Some(&'M') || sub.get(3) == Some(&'U'),
                        });
                    }
                    tree.unstaged.push(file);
                }
            }
            Some("u") => {
                let path = record.splitn(11, ' ').nth(10).unwrap_or_default();
                tree.conflicted.push(change(path, None, FileStatus::Conflicted));
            }
            Some("?") => tree.unstaged.push(change(&record[2..], None, FileStatus::Untracked)),
            _ => {}
        }
    }
    tree
}

/// Split the output of `git diff` (with `a/` and `b/` prefixes and no renames) into each file's
/// part, by path. A part whose path can't be read from its `diff --git` line is left out.
fn split_patches(text: &str) -> Vec<(String, &str)> {
    let mut starts: Vec<usize> = Vec::new();
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        if line.starts_with("diff --git ") {
            starts.push(at);
        }
        at += line.len();
    }
    starts.push(text.len());
    starts
        .windows(2)
        .filter_map(|w| {
            let part = &text[w[0]..w[1]];
            let first = part.lines().next()?;
            Some((patch_path(first.strip_prefix("diff --git ")?)?, part))
        })
        .collect()
}

/// The path of a `diff --git a/<path> b/<path>` line without renames: both halves are the same
/// path, quoted by git when it has unusual characters.
fn patch_path(rest: &str) -> Option<String> {
    if rest.starts_with('"') {
        let (old, tail) = unquote(rest)?;
        let new = tail.strip_prefix(' ')?;
        let new = if new.starts_with('"') {
            unquote(new)?.0
        } else {
            new.to_owned()
        };
        let path = old.strip_prefix("a/")?;
        return (new.strip_prefix("b/")? == path).then(|| path.to_owned());
    }
    // `a/` + path + ` b/` + path.
    let len = rest.len().checked_sub(5)? / 2;
    let path = rest.get(2..2 + len)?;
    (rest.get(..2)? == "a/" && rest.get(2 + len..5 + len)? == " b/" && rest.get(5 + len..)? == path)
        .then(|| path.to_owned())
}

/// A path as git prints it: as it is, or quoted C-style when it holds a tab, a line break, a
/// quote or a backslash, which `core.quotePath=false` still quotes.
pub(crate) fn path_field(field: &str) -> String {
    match unquote(field) {
        Some((path, "")) if field.starts_with('"') => path,
        _ => field.to_owned(),
    }
}

/// The path of a `---`/`+++` line of a patch: quoted C-style when it has a tab, a line break,
/// a quote or a byte outside ASCII, and followed by a tab when it has a space.
pub(crate) fn header_path(field: &str) -> Option<String> {
    if field.starts_with('"') {
        return unquote(field).map(|(path, _)| path);
    }
    Some(field.strip_suffix('\t').unwrap_or(field).to_owned())
}

/// A path git quoted C-style (`"dir/\303\244.txt"`), and what follows it.
fn unquote(text: &str) -> Option<(String, &str)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 1;
    loop {
        match *bytes.get(i)? {
            b'"' => return Some((String::from_utf8_lossy(&out).into_owned(), &text[i + 1..])),
            b'\\' => {
                let c = *bytes.get(i + 1)?;
                if c.is_ascii_digit() {
                    let digits = text.get(i + 1..i + 4)?;
                    out.push(u8::from_str_radix(digits, 8).ok()?);
                    i += 4;
                    continue;
                }
                out.push(match c {
                    b'a' => 7,
                    b'b' => 8,
                    b't' => b'\t',
                    b'n' => b'\n',
                    b'v' => 11,
                    b'f' => 12,
                    b'r' => b'\r',
                    other => other,
                });
                i += 2;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
}

/// Lines in a file as git counts them: empty ones too, and a last one without a line break.
fn line_count(data: &[u8]) -> u32 {
    let breaks = data.iter().filter(|&&b| b == b'\n').count();
    let open = data.last().is_some_and(|&b| b != b'\n');
    (breaks + usize::from(open)) as u32
}

/// Whether a file has its executable bit, which git records as mode 100755.
#[cfg(unix)]
fn executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable(_meta: &std::fs::Metadata) -> bool {
    false
}

/// The patch `git diff --no-index /dev/null <path>` prints for a new file holding `data`: every
/// line added, or a note that the file is binary.
fn untracked_patch(path: &str, data: &[u8], executable: bool) -> String {
    let mode = if executable { "100755" } else { "100644" };
    let mut patch = format!("diff --git a/{path} b/{path}\nnew file mode {mode}\n");
    if data.iter().take(8000).any(|&b| b == 0) {
        patch.push_str(&format!("Binary files /dev/null and b/{path} differ\n"));
        return patch;
    }
    if data.is_empty() {
        return patch;
    }
    let text = String::from_utf8_lossy(data);
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let range = if lines.len() == 1 {
        "+1".to_owned()
    } else {
        format!("+1,{}", lines.len())
    };
    patch.push_str(&format!("--- /dev/null\n+++ b/{path}\n@@ -0,0 {range} @@\n"));
    for line in &lines {
        patch.push('+');
        patch.push_str(line);
    }
    if !text.ends_with('\n') {
        patch.push_str("\n\\ No newline at end of file\n");
    }
    patch
}

struct ParsedHunk {
    /// The `@@ -a,b +c,d @@` part of the hunk's first line.
    header: String,
    /// The hunk as it appears in the patch, header line included.
    text: String,
    hunk: Hunk,
}

struct ParsedPatch {
    /// Everything before the first hunk: `diff --git`, mode and index lines, `---` and `+++`.
    file_header: String,
    hunks: Vec<ParsedHunk>,
    binary: bool,
    /// `old mode` and `new mode` from the header.
    mode: Option<ModeChange>,
    /// The blob ids of the `index` line.
    ids: (Option<String>, Option<String>),
    /// The hunks stop short: the patch had more lines than were asked for.
    cut: bool,
}

/// Parse the output of `git diff` for a single file.
fn parse_patch(patch: &str) -> ParsedPatch {
    parse_patch_within(patch, usize::MAX)
}

/// [`parse_patch`], stopping once the hunks have more than `max` lines: the patch is then `cut`
/// and its hunks are only good for telling that it was too long.
fn parse_patch_within(patch: &str, max: usize) -> ParsedPatch {
    let mut file_header = String::new();
    let mut lines = 0usize;
    let mut cut = false;
    let mut hunks: Vec<ParsedHunk> = Vec::new();
    let mut binary = false;
    let (mut old_mode, mut new_mode, mut ids) = (None, None, (None, None));
    let (mut old_no, mut new_no) = (0u32, 0u32);
    for line in patch.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        if let Some(rest) = body.strip_prefix("@@ ") {
            let Some(end) = rest.find(" @@") else { continue };
            let ranges = &rest[..end];
            let mut parts = ranges.split(' ');
            let (old_start, old_lines) = parse_range(parts.next().unwrap_or("-0").trim_start_matches('-'));
            let (new_start, new_lines) = parse_range(parts.next().unwrap_or("+0").trim_start_matches('+'));
            old_no = old_start;
            new_no = new_start;
            hunks.push(ParsedHunk {
                header: format!("@@ {ranges} @@"),
                text: line.to_owned(),
                hunk: Hunk {
                    header: format!("@@ {ranges} @@"),
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                    lines: Vec::new(),
                    check: String::new(),
                    runs: Vec::new(),
                },
            });
            continue;
        }
        let Some(current) = hunks.last_mut() else {
            if body.starts_with("Binary files ") || body == "GIT binary patch" {
                binary = true;
            } else if let Some(m) = body.strip_prefix("old mode ") {
                old_mode = Some(m.trim().to_owned());
            } else if let Some(m) = body.strip_prefix("new mode ") {
                new_mode = Some(m.trim().to_owned());
            } else if let Some((a, b)) = body
                .strip_prefix("index ")
                .and_then(|r| r.split(' ').next()?.split_once(".."))
            {
                ids = (Some(a.to_owned()), Some(b.to_owned()));
            }
            file_header.push_str(line);
            continue;
        };
        if lines >= max {
            cut = true;
            break;
        }
        current.text.push_str(line);
        let text = body.get(1..).unwrap_or_default().trim_end_matches('\r').to_owned();
        let cr = body.ends_with('\r');
        let (kind, old_line, new_line) = match body.chars().next() {
            Some('+') => {
                new_no += 1;
                (LineKind::Added, None, Some(new_no - 1))
            }
            Some('-') => {
                old_no += 1;
                (LineKind::Removed, Some(old_no - 1), None)
            }
            Some(' ') => {
                old_no += 1;
                new_no += 1;
                (LineKind::Context, Some(old_no - 1), Some(new_no - 1))
            }
            // "\ No newline at end of file" belongs to the patch but is not a line of the file.
            _ => continue,
        };
        lines += 1;
        current.hunk.lines.push(DiffLine {
            kind,
            old_line,
            new_line,
            text,
            cr,
            words: None,
            secret: None,
        });
    }
    // Changed words and the fingerprint that staging checks are for hunks that are shown.
    if !cut {
        for hunk in &mut hunks {
            mark_words(&mut hunk.hunk.lines);
            hunk.hunk.check = fingerprint(&hunk.text);
        }
        let mut seen = HashMap::new();
        for hunk in &mut hunks {
            hunk.hunk.runs = crate::changelists::runs_of(&hunk.hunk.lines, &mut seen)
                .into_iter()
                .map(|run| run.key)
                .collect();
        }
    }
    let mode = match (old_mode, new_mode) {
        (Some(old), Some(new)) => Some(ModeChange { old, new }),
        _ => None,
    };
    ParsedPatch {
        file_header,
        hunks,
        binary,
        mode,
        ids,
        cut,
    }
}

/// The hunk `text` (its `@@` line first) with only the changed lines whose indexes, counted
/// as in [`Hunk::lines`], are in `selected`, for `git apply` going `forward` or with `--reverse`.
/// git has no command for single lines; this is what `git add -p` and `e` do by hand. A changed
/// line left out is dropped when it only exists on the side the patch makes, and becomes
/// context when the side the patch applies to already has it. `None` when nothing is selected.
pub(crate) fn partial_hunk(text: &str, selected: &[usize], forward: bool) -> Option<String> {
    let mut rows = text.split_inclusive('\n');
    let head = rows.next()?;
    let rest = head.strip_prefix("@@ ")?;
    let end = rest.find(" @@")?;
    let mut ranges = rest[..end].split(' ');
    let (old_start, old_lines) = parse_range(ranges.next()?.trim_start_matches('-'));
    let (new_start, new_lines) = parse_range(ranges.next()?.trim_start_matches('+'));
    // The function name git shows after the second @@, and the line end.
    let tail = &rest[end + 3..];

    // Each line with its index, and the "\ No newline at end of file" that follows it.
    let mut parsed: Vec<(&str, Option<&str>, usize)> = Vec::new();
    for row in rows {
        match row.as_bytes().first() {
            Some(b'+' | b'-' | b' ') => parsed.push((row, None, parsed.len())),
            Some(b'\\') => {
                if let Some(last) = parsed.last_mut() {
                    last.1 = Some(row);
                }
            }
            _ => {}
        }
    }
    let mut body = String::new();
    let (mut old, mut new, mut picked) = (0u32, 0u32, 0usize);
    let mut emit = |(row, no_newline, index): (&str, Option<&str>, usize)| {
        let added = row.starts_with('+');
        if row.starts_with(' ') {
            body.push_str(row);
            old += 1;
            new += 1;
        } else if selected.contains(&index) {
            picked += 1;
            body.push_str(row);
            if added {
                new += 1;
            } else {
                old += 1;
            }
        } else if added != forward {
            body.push(' ');
            body.push_str(&row[1..]);
            old += 1;
            new += 1;
        } else {
            return;
        }
        if let Some(marker) = no_newline {
            body.push_str(marker);
        }
    };
    // A run of changed lines goes pair by pair, the n-th removed line beside the n-th added one,
    // so the lines left out keep their places: discarding `b`/`B` alone of `-a -b +A +B`
    // leaves `A`, `b`, not `b`, `A`.
    let mut at = 0;
    while at < parsed.len() {
        if parsed[at].0.starts_with(' ') {
            emit(parsed[at]);
            at += 1;
            continue;
        }
        let end = (at..parsed.len())
            .find(|&i| parsed[i].0.starts_with(' '))
            .unwrap_or(parsed.len());
        let run = &parsed[at..end];
        let removed: Vec<_> = run.iter().filter(|r| r.0.starts_with('-')).copied().collect();
        let added: Vec<_> = run.iter().filter(|r| r.0.starts_with('+')).copied().collect();
        for n in 0..removed.len().max(added.len()) {
            if let Some(&row) = removed.get(n) {
                emit(row);
            }
            if let Some(&row) = added.get(n) {
                emit(row);
            }
        }
        at = end;
    }
    if picked == 0 {
        return None;
    }
    // A side with no lines names the line before it, so the start moves by one when that changes.
    let start = |start: u32, was: u32, now: u32| match (was, now) {
        (0, n) if n > 0 => start + 1,
        (w, 0) if w > 0 => start.saturating_sub(1),
        _ => start,
    };
    Some(format!(
        "@@ -{},{} +{},{} @@{tail}{body}",
        start(old_start, old_lines, old),
        old,
        start(new_start, new_lines, new),
        new
    ))
}

fn parse_range(range: &str) -> (u32, u32) {
    match range.split_once(',') {
        Some((start, count)) => (start.parse().unwrap_or(0), count.parse().unwrap_or(0)),
        None => (range.parse().unwrap_or(0), 1),
    }
}

/// Changed words for runs of removed lines followed by the same number of added lines.
fn mark_words(lines: &mut [DiffLine]) {
    let mut i = 0;
    while i < lines.len() {
        if lines[i].kind != LineKind::Removed {
            i += 1;
            continue;
        }
        let removed_end = (i..lines.len())
            .find(|&j| lines[j].kind != LineKind::Removed)
            .unwrap_or(lines.len());
        let added_end = (removed_end..lines.len())
            .find(|&j| lines[j].kind != LineKind::Added)
            .unwrap_or(lines.len());
        let count = removed_end - i;
        if count == added_end - removed_end {
            for k in 0..count {
                let (r, a) = word_diff(&lines[i + k].text, &lines[removed_end + k].text);
                lines[i + k].words = Some(r);
                lines[removed_end + k].words = Some(a);
            }
        }
        i = added_end.max(i + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_files_count_lines_as_git_does() {
        assert_eq!(line_count(b""), 0);
        assert_eq!(line_count(b"\n"), 1);
        assert_eq!(line_count(b"one\n\n\nend\n"), 4);
        assert_eq!(line_count(b"one\r\ntwo\r\n"), 2);
        assert_eq!(line_count(b"one\ntwo"), 2);
    }

    #[test]
    fn a_patch_past_the_budget_is_cut() {
        let patch = "diff --git a/f b/f\nindex 1111111..2222222 100644\n--- a/f\n+++ b/f\n\
                     @@ -1,2 +1,3 @@\n a\n-b\n+c\n+d\n";
        let whole = parse_patch_within(patch, 10);
        assert!(!whole.cut);
        assert_eq!(whole.hunks[0].hunk.lines.len(), 4);
        assert!(!whole.hunks[0].hunk.check.is_empty());
        let cut = parse_patch_within(patch, 2);
        assert!(cut.cut);
        assert_eq!(cut.hunks[0].hunk.lines.len(), 2);
    }

    #[test]
    fn status_records_are_sorted_into_groups() {
        let out = "1 MM N... 100644 100644 100644 aaa bbb src/a b.rs\0\
                   1 A. N... 000000 100644 100644 000 ccc new.rs\0\
                   2 R. N... 100644 100644 100644 ddd ddd R100 renamed.rs\0old.rs\0\
                   u UU N... 100644 100644 100644 100644 e1 e2 e3 both.rs\0\
                   ? notes.txt\0";
        let tree = parse_status(out);
        let staged: Vec<_> = tree.staged.iter().map(|f| (f.path.as_str(), f.status)).collect();
        assert_eq!(
            staged,
            [
                ("src/a b.rs", FileStatus::Modified),
                ("new.rs", FileStatus::Added),
                ("renamed.rs", FileStatus::Renamed)
            ]
        );
        assert_eq!(tree.staged[2].old_path.as_deref(), Some("old.rs"));
        let unstaged: Vec<_> = tree.unstaged.iter().map(|f| (f.path.as_str(), f.status)).collect();
        assert_eq!(
            unstaged,
            [
                ("src/a b.rs", FileStatus::Modified),
                ("notes.txt", FileStatus::Untracked)
            ]
        );
        assert_eq!(tree.conflicted[0].path, "both.rs");
        assert_eq!(tree.file_count(), 5);
    }

    #[test]
    fn patch_hunks_keep_their_text_and_line_numbers() {
        let patch = "diff --git a/f b/f\nindex 1..2 100644\n--- a/f\n+++ b/f\n\
                     @@ -1,3 +1,3 @@ fn main\n a\n-b c\n+b d\n e\n\
                     @@ -10 +10,2 @@\n x\n+y\n\\ No newline at end of file\n";
        let parsed = parse_patch(patch);
        assert_eq!(
            parsed.file_header,
            "diff --git a/f b/f\nindex 1..2 100644\n--- a/f\n+++ b/f\n"
        );
        assert_eq!(parsed.hunks.len(), 2);
        assert_eq!(parsed.hunks[0].header, "@@ -1,3 +1,3 @@");
        let first = &parsed.hunks[0].hunk;
        let lines: Vec<_> = first
            .lines
            .iter()
            .map(|l| (l.kind, l.old_line, l.new_line, l.text.as_str()))
            .collect();
        assert_eq!(
            lines,
            [
                (LineKind::Context, Some(1), Some(1), "a"),
                (LineKind::Removed, Some(2), None, "b c"),
                (LineKind::Added, None, Some(2), "b d"),
                (LineKind::Context, Some(3), Some(3), "e"),
            ]
        );
        assert!(first.lines[1].words.is_some());
        let second = &parsed.hunks[1];
        assert_eq!((second.hunk.old_start, second.hunk.old_lines), (10, 1));
        assert!(second.text.ends_with("\\ No newline at end of file\n"));
        assert_eq!(second.hunk.lines.len(), 2);
    }

    #[test]
    fn picked_lines_of_a_run_keep_the_others_in_their_places() {
        let hunk = "@@ -1,3 +1,3 @@\n x\n-a\n-b\n+A\n+B\n";
        // Indexes as in Hunk::lines: x 0, -a 1, -b 2, +A 3, +B 4.
        // Staging a/A alone: b stays, after A.
        assert_eq!(
            partial_hunk(hunk, &[1, 3], true).unwrap(),
            "@@ -1,3 +1,3 @@\n x\n-a\n+A\n b\n"
        );
        // Discarding b/B alone: A stays, before b.
        assert_eq!(
            partial_hunk(hunk, &[2, 4], false).unwrap(),
            "@@ -1,3 +1,3 @@\n x\n A\n-b\n+B\n"
        );
        // More added than removed: the extra ones come after the pairs.
        let more = "@@ -1,1 +1,3 @@\n-a\n+A\n+A2\n+A3\n";
        assert_eq!(partial_hunk(more, &[3], true).unwrap(), "@@ -1,1 +1,2 @@\n a\n+A3\n");
        assert_eq!(partial_hunk(hunk, &[0], true), None, "a context line is no pick");
    }
}
