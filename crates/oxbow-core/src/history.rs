use std::collections::{BinaryHeap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gix::ObjectId;
use gix::bstr::ByteSlice;
use gix::revision::walk::Sorting;
use gix::traverse::commit::simple::CommitTimeOrder;
use serde::Serialize;

use crate::bisect::Bisect;
use crate::error::{Error, Result};
use crate::fetched::RemoteFetch;
use crate::graph::{self, GraphCommit, LeadIn, RowLayout, Run, Segment};
use crate::operation::{Operation, OperationKind};
use crate::remote::Tracking;
use crate::repo::{HeadInfo, RefInfo, RefKind, Repo, StashInfo};
use crate::submodules::Submodule;
use crate::worktree::WorkingTree;
use crate::worktrees::Worktree;

/// Commits of the first load. As the list scrolls on the app asks for more, and gets only the
/// rows it does not have yet: see [`History::pieces`].
pub const PAGE: usize = 2_000;
/// Unpushed commits walked at most while the history loads: past them, the loaded rows not met
/// yet wait for [`Repo::settle_unpushed`].
const UNPUSHED_WALK: usize = 5 * PAGE;

/// Which loaded commits are unpushed, as [`Repo::settle_unpushed`] found past the walk the
/// history takes, for the branches and remote-tracking branches where they were then.
#[derive(Clone, Default)]
pub(crate) struct UnpushedAnswers(std::sync::Arc<std::sync::Mutex<Answers>>);

#[derive(Default)]
struct Answers {
    /// The tips the answers are for: [`tips_key`].
    tips: u64,
    unpushed: HashMap<ObjectId, bool>,
    /// Counts the answers added, so that rows built before them are built again.
    added: u64,
}

/// Options for loading the history.
#[derive(Debug, Clone)]
pub struct HistoryOptions {
    /// Maximum number of commits to load, newest first.
    pub limit: usize,
    /// A commit that must be loaded even when it is older than `limit` commits: the history goes
    /// on to the end of the page it is on. Ignored when no branch, tag or `HEAD` reaches it.
    pub until: Option<String>,
    /// Also lay out the graph of the checked-out branch and its base alone: [`History::focus`].
    pub focus: bool,
    /// How many changes the window's watch had noticed when this read began
    /// ([`crate::Watch::seen`]); `None` without a watch of the files.
    pub seen: Option<u64>,
    /// Nothing was done here since the last read, so only changes the watch notices can have
    /// happened: e.g. the window came back to the front. What git said of the uncommitted
    /// changes is then taken again while `seen`, `HEAD` and what is staged stay the same.
    pub quiet: bool,
}

impl Default for HistoryOptions {
    fn default() -> Self {
        HistoryOptions {
            limit: PAGE,
            until: None,
            focus: false,
            seen: None,
            quiet: false,
        }
    }
}

/// Everything the History screen needs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct History {
    pub head: HeadInfo,
    /// Name of the branch drawn as the trunk in column 0.
    pub trunk: Option<String>,
    /// Row of the trunk's newest commit. Rows above it are newer commits on other branches.
    pub trunk_tip_row: Option<usize>,
    /// Gray dashed lines that fill empty columns above lines starting below the top.
    pub lead_ins: Vec<LeadIn>,
    /// While `HEAD` is detached, the branch checked out before, to go back to.
    pub previous_branch: Option<String>,
    /// A merge, rebase, cherry-pick or revert that stopped, waiting to be finished or aborted.
    pub operation: Option<Operation>,
    /// A bisect in progress, or one that found its commit and waits to be finished.
    pub bisect: Option<Bisect>,
    /// Every working folder of the repository, the main one first. Empty when git can't list
    /// them: `problems` says so. The changes in the others are pending: [`Repo::folder_states`].
    pub worktrees: Vec<Worktree>,
    /// The repository's submodules, in the order of `.gitmodules`. Empty when they can't be
    /// read: `problems` says so. What their folders have is pending: [`Repo::folder_states`].
    pub submodules: Vec<Submodule>,
    /// The parts of this read that failed. What they show is not known, which is not the same
    /// as none: no operation, no upstream, no changes.
    pub problems: Vec<ReadProblem>,
    pub refs: Vec<RefInfo>,
    /// Tags the default remote does not have yet, as far as Oxbow knows.
    pub local_tags: Vec<String>,
    pub remotes: Vec<String>,
    /// When each remote was last fetched, in the order of `remotes`.
    pub fetched: Vec<RemoteFetch>,
    /// Upstream of the checked-out branch, with ahead/behind counts.
    pub tracking: Option<Tracking>,
    /// Remote a branch without an upstream is published to.
    pub default_remote: Option<String>,
    /// Stash entries, newest first. Each one is also a row of the graph.
    pub stashes: Vec<StashInfo>,
    pub rows: Vec<HistoryRow>,
    /// Straight lines of the graph that run over two rows or more, ordered by their first row.
    /// They are left out of the rows' own segments: see [`History::segments_at`].
    pub runs: Vec<Run>,
    /// More commits exist beyond `rows`.
    pub truncated: bool,
    /// Commits loaded, stashes aside: what to ask for to load the same history again, which may
    /// be more than was asked for when `until` was older.
    pub loaded: usize,
    /// The repository is a shallow clone: its history stops at commits whose parents were not
    /// fetched.
    pub shallow: bool,
    /// The uncommitted changes the row of uncommitted changes counts; `None` when git could not
    /// list them, and `problems` says why: the row is still there, with no counts. The Changes
    /// panel shows these, so a reload asks git for them once.
    pub changes: Option<WorkingTree>,
    /// Stands for the files on disk of `changes`, by size and modification time: an edit that
    /// keeps the line counts still changes it. For Reload to tell whether anything changed.
    pub changes_stamp: String,
    /// Which reading of `changes` this is, for diffs of the same files: set by the app, 0 here.
    pub snapshot: u64,
    /// Stands for what the commit rows were built from: the same key, the same rows.
    pub key: String,
    /// The caller has the rows of `key` already, so they were left out: `rows` holds only the
    /// row of uncommitted changes, if there is one, and `runs` and `lead_ins` are empty.
    pub same: bool,
    /// When not empty, how to put the rows together from the ones the caller has (those of the
    /// key it asked with) and `rows`, which then holds only the others: one piece after the
    /// other, each a run of the caller's rows or of the next rows of `rows`. More commits loaded
    /// as the list scrolls, or a new commit on top, send only what is new.
    pub pieces: Vec<Piece>,
    /// The git commands this read ran, as typed in a shell; commits and refs are read from
    /// `.git` directly. What Reload shows in Activity.
    pub reads: Vec<String>,
    /// Read on a quiet refresh ([`HistoryOptions::quiet`]) with nothing changed since the read
    /// before: no git command ran, and everything is as that read found it.
    pub unchanged: bool,
    /// Some loaded commits not marked unpushed may be: more were unpushed than the history
    /// walks while it loads. [`Repo::settle_unpushed`] tells, and the next load marks them.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub unpushed_pending: bool,
    /// With [`HistoryOptions::focus`], the graph of the checked-out branch and its base alone.
    pub focus: Option<Focus>,
}

/// The history as "Current Branch Only" shows it: the commits of the checked-out branch, of its
/// upstream and of the trunk's first-parent line, laid out on their own. A line to a commit left
/// out stops short ([`RowLayout::cuts`]) rather than running on to another commit.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Focus {
    /// The rows shown, as indexes into [`History::rows`], in order.
    pub rows: Vec<usize>,
    /// The graph of each row shown, without the straight lines in `runs`.
    pub graph: Vec<RowLayout>,
    pub runs: Vec<Run>,
    pub lead_ins: Vec<LeadIn>,
}

/// A run of history rows: `len` of the caller's rows from `known` on, or with `known` unset,
/// the next `len` rows of [`History::rows`].
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Piece {
    pub known: Option<usize>,
    pub len: usize,
}

impl History {
    /// Every line of a row: the runs over it, then its own segments.
    pub fn segments_at(&self, row: usize) -> Vec<Segment> {
        let mut out: Vec<Segment> = self
            .runs
            .iter()
            .take_while(|run| run.first <= row)
            .filter(|run| run.last >= row)
            .map(Run::segment)
            .collect();
        out.extend(self.rows[row].graph.segments.iter().cloned());
        out
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRow {
    pub id: String,
    pub summary: String,
    pub author_name: String,
    pub author_email: String,
    /// Author time, seconds since the Unix epoch.
    pub time: i64,
    pub parents: Vec<String>,
    /// Branches and tags pointing at this commit.
    pub labels: Vec<Label>,
    /// The commit is not on any remote-tracking branch yet.
    pub unpushed: bool,
    /// Only a detached `HEAD` has the commit: no branch or tag keeps it.
    pub no_branch: bool,
    pub graph: RowLayout,
    /// Set on the row of uncommitted changes, whose `id` is [`WORKTREE_ID`].
    pub worktree: Option<WorktreeSummary>,
}

/// Id of the history row that stands for the uncommitted changes.
pub const WORKTREE_ID: &str = "worktree";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeSummary {
    /// Files with any change.
    pub files: usize,
    pub staged: usize,
    pub unstaged: usize,
    /// Of the unstaged files, the new ones git does not track yet.
    pub untracked: usize,
    pub conflicted: usize,
    /// `git status` failed: the counts are 0 because they are not known.
    pub unknown: bool,
}

/// A part of the History read that failed, and git's words for why.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReadProblem {
    pub part: ReadPart,
    pub error: String,
}

/// The parts of the History read that can fail on their own, leaving the commits shown.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum ReadPart {
    /// The uncommitted changes of this folder (`git status`).
    Changes,
    /// The upstreams of the branches and their ahead/behind counts (`git for-each-ref`).
    Tracking,
    /// A merge, rebase, cherry-pick or revert under way.
    Operation,
    Bisect,
    /// The list of worktrees (`git worktree list`).
    Worktrees,
    /// The list of submodules (`.gitmodules` and the index).
    Submodules,
}

impl ReadPart {
    /// How the Console names it.
    pub fn name(self) -> &'static str {
        match self {
            ReadPart::Changes => "status",
            ReadPart::Tracking => "upstreams",
            ReadPart::Operation => "operation",
            ReadPart::Bisect => "bisect",
            ReadPart::Worktrees => "worktrees",
            ReadPart::Submodules => "submodules",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    pub name: String,
    pub kind: RefKind,
    /// Color of the line the label sits on (tags are always drawn yellow).
    pub color: u8,
    /// The checked-out branch.
    pub head: bool,
}

struct Node {
    id: ObjectId,
    parents: Vec<ObjectId>,
    time: i64,
}

/// The commit rows of the last history built, to build the next one without walking the commits
/// and laying out the graph again while nothing they depend on changed: a refresh after the
/// window comes back to the front, a staged file, a fetch that brought nothing.
#[derive(Debug, Default, Clone)]
pub struct HistoryCache {
    last: Option<(u64, Arc<Rows>)>,
    asked: Asked,
}

/// What the last read asked git, each with a key for what the answer depends on: the next read
/// takes it again while the key stays the same. A refresh with nothing changed runs no git.
#[derive(Debug, Default, Clone)]
struct Asked {
    /// The upstreams of the local branches, by the refs and the settings.
    tracking: Option<(u64, HashMap<String, Tracking>)>,
    /// git's list of the worktrees, by the refs and their records in `.git/worktrees`.
    worktrees: Option<(u64, Vec<Worktree>)>,
    /// The uncommitted changes, by what the watch noticed, `HEAD` and what is staged.
    changes: Option<(u64, WorkingTree)>,
    /// The upstream of the checked-out branch once old versions of commits it rewrote were
    /// looked for, by its upstream and what the watch noticed: the reflogs it reads move with
    /// the refs.
    rewrite: Option<(u64, Option<Tracking>)>,
}

impl HistoryCache {
    /// Nothing was read yet.
    pub fn is_empty(&self) -> bool {
        self.last.is_none()
    }
}

/// The part of [`History`] that comes from walking the commits.
#[derive(Debug)]
struct Rows {
    /// With the row of uncommitted changes on top when there are any, its counts and summary
    /// filled in afresh each time by [`fill_worktree_row`].
    rows: Vec<HistoryRow>,
    runs: Vec<Run>,
    lead_ins: Vec<LeadIn>,
    trunk: Option<String>,
    trunk_tip_row: Option<usize>,
    truncated: bool,
    loaded: usize,
    unpushed_pending: bool,
    focus: Option<Focus>,
}

fn keyed(value: &impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// What the commit rows are built from. The commits themselves never change under their ids,
/// so the refs that reach them stand for them.
#[allow(clippy::too_many_arguments)]
fn rows_key(
    workdir: &std::path::Path,
    options: &HistoryOptions,
    head: &HeadInfo,
    refs: &[RefInfo],
    stashes: &[StashInfo],
    remotes: &[String],
    rebasing: bool,
    changes: bool,
    production: Option<&str>,
    shallow: u64,
    settled: u64,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    workdir.hash(&mut hasher);
    options.limit.hash(&mut hasher);
    options.until.hash(&mut hasher);
    options.focus.hash(&mut hasher);
    head.branch.hash(&mut hasher);
    head.commit.hash(&mut hasher);
    for r in refs {
        // The focused graph follows the upstream of the branch, which changes without a ref moving.
        let upstream = r.tracking.as_ref().map(|t| (&t.remote, &t.branch));
        (&r.name, r.kind as u8, &r.target, &r.remote, upstream).hash(&mut hasher);
    }
    for stash in stashes {
        (&stash.id, stash.index).hash(&mut hasher);
    }
    remotes.hash(&mut hasher);
    (rebasing, changes, production, shallow, settled, graph::branch_colors()).hash(&mut hasher);
    hasher.finish()
}

/// What the row of uncommitted changes says: the operation under way and the file counts.
fn fill_worktree_row(
    row: &mut HistoryRow,
    tree: Option<&WorkingTree>,
    operation: Option<&Operation>,
    branch: Option<&str>,
) {
    row.summary = operation
        .map_or("Uncommitted changes", |op| match op.kind {
            OperationKind::Merge => "Merge in progress",
            OperationKind::Squash => "Squash merge in progress",
            OperationKind::Rebase => "Rebase in progress",
            OperationKind::CherryPick => "Cherry-pick in progress",
            OperationKind::Revert => "Revert in progress",
            OperationKind::Am => "Applying patches",
            OperationKind::StashApply => "Stash apply in progress",
        })
        .to_owned();
    row.time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    row.graph.branch = branch.map(str::to_owned);
    row.worktree = Some(match tree {
        Some(tree) => WorktreeSummary {
            files: tree.file_count(),
            staged: tree.staged.len(),
            unstaged: tree.unstaged.len(),
            untracked: tree
                .unstaged
                .iter()
                .filter(|f| matches!(f.status, crate::commit::FileStatus::Untracked))
                .count(),
            conflicted: tree.conflicted.len(),
            unknown: false,
        },
        None => WorktreeSummary {
            files: 0,
            staged: 0,
            unstaged: 0,
            untracked: 0,
            conflicted: 0,
            unknown: true,
        },
    });
}

impl Repo {
    /// A hash of the size and modification time of each changed file on disk.
    fn files_stamp(&self, tree: &WorkingTree) -> String {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for file in tree.staged.iter().chain(&tree.unstaged).chain(&tree.conflicted) {
            let meta = std::fs::symlink_metadata(self.workdir().join(&file.path)).ok();
            (&file.path, meta.as_ref().map(|m| (m.len(), m.modified().ok()))).hash(&mut hasher);
        }
        format!("{:016x}", hasher.finish())
    }

    /// Stands for the commits where a shallow clone's history stops, as `.git/shallow` lists
    /// them; 0 for a full clone. It changes when a fetch deepens the history, which moves no ref.
    pub fn shallow_stamp(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let Ok(list) = std::fs::read(self.local().shallow_file()) else {
            return 0;
        };
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        list.hash(&mut hasher);
        hasher.finish().max(1)
    }

    /// Stands for what the upstreams and their ahead/behind counts are worked out from: `HEAD`,
    /// the refs, where the history stops, and the repository's settings as written.
    fn state_print(&self, head: &HeadInfo, refs: &[RefInfo], boundary: u64) -> u64 {
        let local = self.local();
        let refs: Vec<_> = refs.iter().map(|r| (&r.name, r.kind as u8, &r.target)).collect();
        let settings = [
            local.common_dir().join("config"),
            local.git_dir().join("config.worktree"),
        ]
        .map(|file| std::fs::read(file).ok());
        keyed(&(&head.branch, &head.commit, refs, boundary, settings))
    }

    /// Load the commit history of all branches and tags with its graph layout.
    pub fn history(&self, options: &HistoryOptions) -> Result<History> {
        self.history_with(options, &mut HistoryCache::default(), None)
    }

    /// [`Repo::history`], reusing the commit rows of `cache` when nothing they depend on changed
    /// and keeping the new ones there. With `known`, the key of rows the caller has already, the
    /// same rows are left out of the answer: see [`History::same`].
    pub fn history_with(
        &self,
        options: &HistoryOptions,
        cache: &mut HistoryCache,
        known: Option<&str>,
    ) -> Result<History> {
        // The records of a worktree are in the main repository, so its history still reads when
        // its own folder is gone: only git's commands in that folder would fail, one by one.
        if std::fs::metadata(self.workdir()).is_err() {
            return Err(Error::FolderGone {
                path: self.workdir().display().to_string(),
            });
        }
        let (history, reads) = crate::cli::recording(|| self.read_history(options, cache, known));
        if let Ok(history) = &history {
            let status = match &history.changes {
                Some(tree) => {
                    let changes = tree.file_count();
                    format!(
                        "status {} {}",
                        crate::console::grouped(changes),
                        if changes == 1 { "change" } else { "changes" }
                    )
                }
                None => "status failed".to_owned(),
            };
            let mut note = format!(
                "refs {} · commits {} · {status}",
                crate::console::grouped(history.refs.len()),
                crate::console::grouped(history.loaded),
            );
            for problem in history.problems.iter().filter(|p| p.part != ReadPart::Changes) {
                note.push_str(&format!(" · {} failed", problem.part.name()));
            }
            crate::console::read_note(note);
        }
        history.map(|history| History { reads, ..history })
    }

    fn read_history(&self, options: &HistoryOptions, cache: &mut HistoryCache, known: Option<&str>) -> Result<History> {
        let repo = self.local();
        let head = self.head()?;
        let mut refs = self.refs()?;
        // A deeper fetch moves where the history stops without moving any ref.
        let boundary = self.shallow_stamp();
        let state = self.state_print(&head, &refs, boundary);
        // Only what the watch notices can have changed, and it counts what it noticed.
        let quiet = options.quiet && options.seen.is_some();
        let mut asked = Asked::default();
        // Answers taken again, of the four in `Asked`.
        let mut taken = 0;
        // Each of these can fail on its own: the history still shows, and `problems` says what
        // isn't known, so that it doesn't look like none.
        let mut problems = Vec::new();
        let mut note = |part: ReadPart, err: Error| {
            problems.push(ReadProblem {
                part,
                error: err.to_string(),
            })
        };
        let tracking_key = state;
        let tracked = match &cache.asked.tracking {
            Some((key, upstreams)) if *key == tracking_key => {
                taken += 1;
                Ok(upstreams.clone())
            }
            _ => self.branch_tracking(),
        };
        if let Ok(upstreams) = &tracked {
            asked.tracking = Some((tracking_key, upstreams.clone()));
        }
        let mut upstreams = tracked.unwrap_or_else(|err| {
            note(ReadPart::Tracking, err);
            HashMap::new()
        });
        for r in refs.iter_mut().filter(|r| r.kind == RefKind::Local) {
            r.tracking = upstreams.remove(&r.name);
        }
        let remotes = self.remotes();
        let stashes = self.stashes()?;
        let operation = self.operation().unwrap_or_else(|err| {
            note(ReadPart::Operation, err);
            None
        });
        let rebasing = operation.as_ref().is_some_and(|op| op.kind == OperationKind::Rebase);
        let bisect = self.bisect().unwrap_or_else(|err| {
            note(ReadPart::Bisect, err);
            None
        });
        // Only the lists: what is in each folder comes after the history, see `folders.rs`.
        let worktrees_key = keyed(&(state, self.worktrees_print()));
        let listed = match &cache.asked.worktrees {
            Some((key, list)) if *key == worktrees_key => {
                taken += 1;
                Ok(list.clone())
            }
            _ => self.listed_worktrees(),
        };
        if let Ok(list) = &listed {
            asked.worktrees = Some((worktrees_key, list.clone()));
        }
        let worktrees = listed.map(|list| self.place_worktrees(list)).unwrap_or_else(|err| {
            note(ReadPart::Worktrees, err);
            Vec::new()
        });
        let submodules = self.submodule_list().unwrap_or_else(|err| {
            note(ReadPart::Submodules, err);
            Vec::new()
        });
        // Uncommitted changes get a row of their own on top. When `git status` fails the row
        // stays, saying the changes aren't known, rather than looking clean.
        let staged = crate::watch::staged(&repo.index_path(), repo.object_hash());
        let changes_key = options.seen.map(|seen| keyed(&(seen, &head.commit, staged)));
        let changes = match &cache.asked.changes {
            Some((key, tree)) if quiet && Some(*key) == changes_key => {
                taken += 1;
                Ok(tree.clone())
            }
            _ => self.working_tree(),
        };
        if let (Ok(tree), Some(key)) = (&changes, changes_key) {
            asked.changes = Some((key, tree.clone()));
        }
        let changes = changes.map_err(|err| note(ReadPart::Changes, err)).ok();
        let dirty = changes.as_ref().is_none_or(|tree| !tree.is_empty());
        let production = self.production_setting();
        let shallow = repo.is_shallow();

        let key = rows_key(
            self.workdir(),
            options,
            &head,
            &refs,
            &stashes,
            &remotes,
            rebasing,
            dirty,
            production.as_deref(),
            boundary,
            self.unpushed.0.lock().expect("unpushed lock").added,
        );
        let rows_before = cache.last.as_ref().map(|(last, _)| *last);
        // The rows the caller has, when they are the ones built last.
        let mut before = None;
        let built = match &cache.last {
            Some((last, built)) if *last == key => built.clone(),
            last => {
                let previous = last.as_ref().map(|(_, rows)| rows.clone());
                let built = Arc::new(self.build_rows(
                    &repo,
                    options,
                    &head,
                    &refs,
                    &stashes,
                    &remotes,
                    rebasing,
                    dirty,
                    production.as_deref(),
                    previous.as_deref(),
                )?);
                if let Some((last, rows)) = last
                    && known == Some(format!("{last:016x}").as_str())
                {
                    before = Some(rows.clone());
                }
                cache.last = Some((key, built.clone()));
                built
            }
        };
        let key_text = format!("{key:016x}");
        let same = known == Some(key_text.as_str());
        let mut pieces = Vec::new();
        let mut rows: Vec<HistoryRow> = if same {
            built
                .rows
                .iter()
                .take_while(|r| r.worktree.is_some())
                .cloned()
                .collect()
        } else if let Some(before) = before {
            let (fresh, layout) = pieced(&before.rows, &built.rows);
            pieces = layout;
            fresh
        } else {
            built.rows.clone()
        };
        if let Some(row) = rows.first_mut().filter(|r| r.worktree.is_some()) {
            fill_worktree_row(row, changes.as_ref(), operation.as_ref(), head.branch.as_deref());
        }

        let previous_branch = head.branch.is_none().then(|| self.previous_branch(&refs)).flatten();
        // The checked-out branch's upstream was read with the others. A failing `git for-each-ref`
        // leaves it out, and `problems` says so.
        let upstream = head.branch.as_ref().and_then(|branch| {
            refs.iter()
                .find(|r| r.kind == RefKind::Local && &r.name == branch)
                .and_then(|r| r.tracking.clone())
        });
        let rewrite_key = options.seen.map(|seen| keyed(&(seen, tracking_key, &head.branch)));
        let tracking = match (&cache.asked.rewrite, &head.branch, upstream) {
            (Some((key, tracking)), _, _) if quiet && Some(*key) == rewrite_key => {
                taken += 1;
                tracking.clone()
            }
            (_, Some(branch), Some(mut t)) => {
                if t.behind > 0 {
                    self.find_rewrite(branch, &mut t);
                }
                Some(t)
            }
            _ => None,
        };
        if let Some(key) = rewrite_key
            && problems.iter().all(|p| p.part != ReadPart::Tracking)
        {
            asked.rewrite = Some((key, tracking.clone()));
        }
        let unchanged = quiet && taken == 4 && rows_before == Some(key);
        cache.asked = asked;
        Ok(History {
            head,
            trunk: built.trunk.clone(),
            trunk_tip_row: built.trunk_tip_row,
            lead_ins: if same { Vec::new() } else { built.lead_ins.clone() },
            previous_branch,
            operation,
            bisect,
            worktrees,
            submodules,
            problems,
            local_tags: self.local_tags(&refs),
            fetched: self.fetch_times_of(&remotes, &refs),
            refs,
            tracking,
            default_remote: self.default_remote(),
            remotes,
            stashes,
            rows,
            runs: if same { Vec::new() } else { built.runs.clone() },
            truncated: built.truncated,
            loaded: built.loaded,
            shallow,
            changes_stamp: changes.as_ref().map_or_else(String::new, |tree| self.files_stamp(tree)),
            changes,
            snapshot: 0,
            key: key_text,
            same,
            pieces,
            reads: Vec::new(),
            unchanged,
            unpushed_pending: built.unpushed_pending,
            focus: built.focus.clone(),
        })
    }

    /// Walk the commits and lay out the graph.
    #[allow(clippy::too_many_arguments)]
    fn build_rows(
        &self,
        repo: &gix::Repository,
        options: &HistoryOptions,
        head: &HeadInfo,
        refs: &[RefInfo],
        stashes: &[StashInfo],
        remotes: &[String],
        rebasing: bool,
        dirty: bool,
        production: Option<&str>,
        previous: Option<&Rows>,
    ) -> Result<Rows> {
        let mut tips: Vec<ObjectId> = refs
            .iter()
            .filter_map(|r| ObjectId::from_hex(r.target.as_bytes()).ok())
            .collect();
        if let Some(id) = head
            .commit
            .as_deref()
            .and_then(|h| ObjectId::from_hex(h.as_bytes()).ok())
        {
            tips.push(id);
        }
        tips.sort();
        tips.dedup();

        let mut nodes = Vec::new();
        let mut truncated = false;
        if !tips.is_empty() {
            let walk = repo
                .rev_walk(tips.iter().copied())
                .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst))
                .all()
                .map_err(Error::git)?;
            let mut until = options
                .until
                .as_deref()
                .and_then(|id| ObjectId::from_hex(id.as_bytes()).ok());
            let mut limit = options.limit.max(1);
            for info in walk {
                let info = info.map_err(Error::git)?;
                if nodes.len() % 1024 == 0 {
                    crate::cli::check_stop()?;
                }
                if nodes.len() == limit {
                    if until.is_none() {
                        truncated = true;
                        break;
                    }
                    // Still looking for `until`: one more page.
                    limit += PAGE;
                }
                if until == Some(info.id) {
                    until = None;
                }
                nodes.push(Node {
                    id: info.id,
                    parents: info.parent_ids.iter().copied().collect(),
                    time: info.commit_time.unwrap_or_default(),
                });
            }
            // No branch or tag reaches `until`: back to the history asked for.
            if until.is_some() && nodes.len() > options.limit {
                nodes.truncate(options.limit.max(1));
                truncated = true;
            }
        }
        let loaded = nodes.len();
        // Stashes hang off the commit they were made on. Only that first parent is drawn: the other
        // parents (the index and the untracked files) are internal to the stash.
        let mut stash_ids = HashSet::new();
        for stash in stashes {
            let Ok(id) = ObjectId::from_hex(stash.id.as_bytes()) else {
                continue;
            };
            let Ok(commit) = repo.find_commit(id) else {
                continue;
            };
            if !stash_ids.insert(id) {
                continue;
            }
            nodes.push(Node {
                id,
                parents: commit.parent_ids().take(1).map(|p| p.detach()).collect(),
                time: commit.time().map(|t| t.seconds).unwrap_or_default(),
            });
        }
        let nodes = topo_order(nodes);
        let row_of: HashMap<ObjectId, usize> = nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();

        // The trunk: first-parent chain of the main branch.
        let trunk_ref = pick_trunk(refs, production);
        let mut on_trunk = vec![false; nodes.len()];
        let mut trunk_tip_row = None;
        if let Some(tip) = trunk_ref.and_then(|r| ObjectId::from_hex(r.target.as_bytes()).ok()) {
            let mut row = row_of.get(&tip).copied();
            trunk_tip_row = row;
            while let Some(r) = row {
                on_trunk[r] = true;
                row = nodes[r].parents.first().and_then(|p| row_of.get(p)).copied();
            }
        }

        let (unpushed, unpushed_pending) = self.unpushed_commits(repo, refs, head, &nodes)?;

        // Commits a branch or tag keeps. While HEAD is detached, the others were made there and
        // belong to no branch. Children come before parents, so one pass reaches every ancestor.
        // A rebase detaches HEAD while it works; its commits will be the branch's.
        let mut kept = vec![head.branch.is_some() || rebasing; nodes.len()];
        for r in refs {
            if let Some(&row) = ObjectId::from_hex(r.target.as_bytes())
                .ok()
                .and_then(|id| row_of.get(&id))
            {
                kept[row] = true;
            }
        }
        for row in 0..nodes.len() {
            if kept[row] {
                for parent in &nodes[row].parents {
                    if let Some(&p) = row_of.get(parent) {
                        kept[p] = true;
                    }
                }
            }
        }
        let no_branch: Vec<bool> = nodes
            .iter()
            .enumerate()
            .map(|(row, node)| !kept[row] && !stash_ids.contains(&node.id))
            .collect();

        // Labels per commit.
        let mut labels: HashMap<ObjectId, Vec<Label>> = HashMap::new();
        for r in refs {
            let Ok(id) = ObjectId::from_hex(r.target.as_bytes()) else {
                continue;
            };
            let is_head = r.kind == RefKind::Local && head.branch.as_deref() == Some(r.name.as_str());
            labels.entry(id).or_default().push(Label {
                name: r.name.clone(),
                kind: r.kind,
                color: 0,
                head: is_head,
            });
        }
        // A detached HEAD gets a label of its own, as no branch label marks it. A rebase detaches
        // HEAD too, but there the banner says what is going on.
        if let Some(id) = head
            .commit
            .as_deref()
            .filter(|_| head.branch.is_none() && !rebasing)
            .and_then(|h| ObjectId::from_hex(h.as_bytes()).ok())
        {
            labels.entry(id).or_default().push(Label {
                name: "HEAD".to_owned(),
                kind: RefKind::Head,
                color: 0,
                head: true,
            });
        }
        for stash in stashes {
            let Ok(id) = ObjectId::from_hex(stash.id.as_bytes()) else {
                continue;
            };
            labels.entry(id).or_default().push(Label {
                name: format!("stash@{{{}}}", stash.index),
                kind: RefKind::Stash,
                color: 0,
                head: false,
            });
        }
        for list in labels.values_mut() {
            list.sort_by_key(|l| (!l.head, l.kind as u8, l.name.clone()));
        }

        // What the last history read of each commit: a commit never changes under its id.
        let known: HashMap<&str, &HistoryRow> = previous
            .map(|p| {
                p.rows
                    .iter()
                    .filter(|r| r.worktree.is_none())
                    .map(|r| (r.id.as_str(), r))
                    .collect()
            })
            .unwrap_or_default();
        let mut graph_input = Vec::with_capacity(nodes.len());
        let mut details = Vec::with_capacity(nodes.len());
        for (row, node) in nodes.iter().enumerate() {
            let id = node.id.to_string();
            let (summary, author_name, author_email, author_time) = match known.get(id.as_str()) {
                Some(r) => (r.summary.clone(), r.author_name.clone(), r.author_email.clone(), r.time),
                None => {
                    let commit = repo.find_commit(node.id).map_err(Error::git)?;
                    let message = commit.message_raw_sloppy();
                    let summary = message
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .to_str_lossy()
                        .trim()
                        .to_owned();
                    let author = commit.author().map_err(Error::git)?;
                    (
                        summary,
                        author.name.to_str_lossy().into_owned(),
                        author.email.to_str_lossy().into_owned(),
                        author.time().map(|t| t.seconds).unwrap_or(node.time),
                    )
                }
            };
            let row_labels = labels.get(&node.id);
            graph_input.push(GraphCommit {
                parents: node.parents.iter().map(|p| row_of.get(p).copied()).collect(),
                trunk: on_trunk[row],
                tip_name: row_labels.and_then(|l| tip_name(l, remotes)),
                merged_name: (node.parents.len() > 1)
                    .then(|| merged_branch(&summary, remotes))
                    .flatten(),
                unpushed: unpushed.contains(&node.id) || stash_ids.contains(&node.id),
                side: stash_ids.contains(&node.id),
                id,
                color: if stash_ids.contains(&node.id) {
                    Some(graph::STASH_COLOR)
                } else {
                    no_branch[row].then_some(graph::NO_BRANCH_COLOR)
                },
                worktree: false,
                cut: Vec::new(),
            });
            details.push((summary, author_name, author_email, author_time));
        }

        // Uncommitted changes go on a side line down to HEAD, in the color of HEAD's line.
        let head_id = head
            .commit
            .as_deref()
            .and_then(|h| ObjectId::from_hex(h.as_bytes()).ok());
        let head_row = head_id.and_then(|id| row_of.get(&id).copied());
        let lay = |input: Vec<GraphCommit>, head_row: Option<usize>| {
            if !dirty {
                return graph::layout(&input, head_row);
            }
            // One layout with the row of uncommitted changes on top: it takes the color of
            // HEAD's line as the layout finds it, or of the branch about to be born.
            let color = match head_row {
                Some(_) => None,
                None => Some(head.branch.as_deref().map_or(graph::TRUNK_COLOR, graph::color_for_name)),
            };
            let mut shifted = Vec::with_capacity(input.len() + 1);
            shifted.push(GraphCommit {
                parents: vec![head_row.map(|r| r + 1)],
                side: true,
                color,
                unpushed: true,
                worktree: true,
                ..Default::default()
            });
            shifted.extend(input.into_iter().map(|mut c| {
                c.parents = c.parents.into_iter().map(|p| p.map(|r| r + 1)).collect();
                c
            }));
            graph::layout(&shifted, head_row.map(|r| r + 1))
        };
        let focus = options.focus.then(|| {
            let upstream = head
                .branch
                .as_deref()
                .and_then(|branch| refs.iter().find(|r| r.kind == RefKind::Local && r.name == branch))
                .and_then(|r| r.tracking.as_ref())
                .map(|t| format!("{}/{}", t.remote, t.branch))
                .and_then(|name| refs.iter().find(|r| r.kind == RefKind::Remote && r.name == name))
                .and_then(|r| ObjectId::from_hex(r.target.as_bytes()).ok())
                .and_then(|id| row_of.get(&id).copied());
            let shown = focus_rows(&nodes, &row_of, &on_trunk, &[head_row, upstream]);
            let (input, head_at) = focus_input(&graph_input, &shown, head_row);
            let laid = lay(input, head_at);
            let mut graph = laid.rows;
            let offset = usize::from(dirty);
            let rows: Vec<usize> = (0..offset).chain(shown.iter().map(|&r| r + offset)).collect();
            let runs = graph::take_runs(graph.iter_mut());
            Focus {
                rows,
                graph,
                runs,
                lead_ins: laid.lead_ins,
            }
        });
        let mut worktree_row = None;
        let mut graph = lay(graph_input, head_row);
        if dirty {
            worktree_row = Some(HistoryRow {
                id: WORKTREE_ID.to_owned(),
                summary: String::new(),
                author_name: String::new(),
                author_email: String::new(),
                time: 0,
                parents: head_id.iter().map(ToString::to_string).collect(),
                labels: Vec::new(),
                unpushed: true,
                no_branch: false,
                graph: graph.rows.remove(0),
                worktree: Some(WorktreeSummary {
                    files: 0,
                    staged: 0,
                    unstaged: 0,
                    untracked: 0,
                    conflicted: 0,
                    unknown: false,
                }),
            });
        }
        // Trunk commits belong to the trunk branch, named without its remote (`origin/main` -> `main`).
        let trunk_name = trunk_ref.map(|r| match r.kind {
            RefKind::Remote => strip_remote(&r.name, remotes).to_owned(),
            _ => r.name.clone(),
        });
        let lead_ins = graph.lead_ins;
        let rows = nodes.iter().zip(details).zip(graph.rows).enumerate().map(
            |(row, ((node, (summary, author_name, author_email, time)), graph))| {
                let mut labels = labels.remove(&node.id).unwrap_or_default();
                // Labels take the color of the line they sit on.
                for label in &mut labels {
                    label.color = graph.color;
                }
                let mut graph = graph;
                if graph.branch.is_none() && on_trunk[row] {
                    graph.branch = trunk_name.clone();
                }
                HistoryRow {
                    id: node.id.to_string(),
                    summary,
                    author_name,
                    author_email,
                    time,
                    parents: node.parents.iter().map(ToString::to_string).collect(),
                    labels,
                    unpushed: unpushed.contains(&node.id) || stash_ids.contains(&node.id),
                    no_branch: no_branch[row],
                    graph,
                    worktree: None,
                }
            },
        );
        let mut rows: Vec<HistoryRow> = worktree_row.into_iter().chain(rows).collect();
        let runs = graph::take_runs(rows.iter_mut().map(|r| &mut r.graph));
        let trunk_tip_row = trunk_tip_row.map(|r| r + usize::from(dirty));

        Ok(Rows {
            rows,
            runs,
            lead_ins,
            trunk: trunk_ref.map(|r| r.name.clone()),
            trunk_tip_row,
            truncated,
            loaded,
            unpushed_pending,
            focus,
        })
    }
}

/// Rows of `nodes` that "Current Branch Only" shows, in order: the trunk's first-parent line, and
/// the first-parent line down from each of `tips` until it meets a row already shown.
fn focus_rows(
    nodes: &[Node],
    row_of: &HashMap<ObjectId, usize>,
    on_trunk: &[bool],
    tips: &[Option<usize>],
) -> Vec<usize> {
    let mut shown = on_trunk.to_vec();
    for &tip in tips {
        let mut row = tip;
        while let Some(r) = row.filter(|&r| !shown[r]) {
            shown[r] = true;
            row = nodes[r].parents.first().and_then(|p| row_of.get(p)).copied();
        }
    }
    (0..nodes.len()).filter(|&r| shown[r]).collect()
}

/// The commits of `shown` alone, for the layout, and the row of `head_row` among them. Parents
/// left out are cut, not taken for commits that weren't loaded.
fn focus_input(all: &[GraphCommit], shown: &[usize], head_row: Option<usize>) -> (Vec<GraphCommit>, Option<usize>) {
    let mut index = vec![None; all.len()];
    for (i, &row) in shown.iter().enumerate() {
        index[row] = Some(i);
    }
    let input = shown
        .iter()
        .map(|&row| {
            let mut commit = all[row].clone();
            commit.cut = commit
                .parents
                .iter()
                .map(|p| p.is_some_and(|p| index[p].is_none()))
                .collect();
            commit.parents = commit.parents.iter().map(|p| p.and_then(|p| index[p])).collect();
            commit
        })
        .collect();
    (input, head_row.and_then(|r| index[r]))
}

/// `rows` as pieces of `before`, the rows the caller has, and of the rows it lacks, which come
/// back in order. A row is the caller's when its commit has a row there that is the same in
/// every way; the row of uncommitted changes always comes anew.
fn pieced(before: &[HistoryRow], rows: &[HistoryRow]) -> (Vec<HistoryRow>, Vec<Piece>) {
    let at: HashMap<&str, usize> = before
        .iter()
        .enumerate()
        .filter(|(_, r)| r.worktree.is_none())
        .map(|(i, r)| (r.id.as_str(), i))
        .collect();
    let mut fresh = Vec::new();
    let mut pieces: Vec<Piece> = Vec::new();
    for row in rows {
        let known = at
            .get(row.id.as_str())
            .copied()
            .filter(|&i| row.worktree.is_none() && before[i] == *row);
        match (pieces.last_mut(), known) {
            (Some(Piece { known: Some(from), len }), Some(i)) if *from + *len == i => *len += 1,
            (Some(Piece { known: None, len }), None) => *len += 1,
            _ => pieces.push(Piece { known, len: 1 }),
        }
        if known.is_none() {
            fresh.push(row.clone());
        }
    }
    (fresh, pieces)
}

impl Repo {
    /// The branch checked out before `HEAD` was detached, read from `HEAD`'s reflog.
    fn previous_branch(&self, refs: &[RefInfo]) -> Option<String> {
        let repo = self.local();
        let head = repo.find_reference("HEAD").ok()?;
        let mut log = head.log_iter();
        for line in log.rev().ok()?? {
            let line = line.ok()?;
            let message = line.message.to_str_lossy();
            let Some((from, _)) = message
                .strip_prefix("checkout: moving from ")
                .and_then(|rest| rest.split_once(" to "))
            else {
                continue;
            };
            if refs.iter().any(|r| r.kind == RefKind::Local && r.name == from) {
                return Some(from.to_owned());
            }
        }
        None
    }
}

/// Order commits children before parents; among commits that are ready, newest first.
fn topo_order(nodes: Vec<Node>) -> Vec<Node> {
    let index: HashMap<ObjectId, usize> = nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
    let mut children = vec![0usize; nodes.len()];
    for node in &nodes {
        for parent in &node.parents {
            if let Some(&p) = index.get(parent) {
                children[p] += 1;
            }
        }
    }
    // Max-heap on (time, reverse walk position) keeps the walk's order for equal times.
    let mut ready: BinaryHeap<(i64, std::cmp::Reverse<usize>)> = nodes
        .iter()
        .enumerate()
        .filter(|(i, _)| children[*i] == 0)
        .map(|(i, n)| (n.time, std::cmp::Reverse(i)))
        .collect();
    let mut order = Vec::with_capacity(nodes.len());
    while let Some((_, std::cmp::Reverse(i))) = ready.pop() {
        order.push(i);
        for parent in &nodes[i].parents {
            if let Some(&p) = index.get(parent) {
                children[p] -= 1;
                if children[p] == 0 {
                    ready.push((nodes[p].time, std::cmp::Reverse(p)));
                }
            }
        }
    }
    let mut slots: Vec<Option<Node>> = nodes.into_iter().map(Some).collect();
    order.into_iter().filter_map(|i| slots[i].take()).collect()
}

/// Where Settings › This Repository › Production branch is kept, as a full ref name.
pub const PRODUCTION_KEY: &str = "oxbow.productionBranch";

/// Full ref name of a branch: `refs/heads/main`, `refs/remotes/origin/main`.
pub fn full_ref(r: &RefInfo) -> Option<String> {
    match r.kind {
        RefKind::Local => Some(format!("refs/heads/{}", r.name)),
        RefKind::Remote => Some(format!("refs/remotes/{}", r.name)),
        _ => None,
    }
}

/// The production branch, drawn as the trunk: the branch chosen in Settings (a full ref), or with
/// none chosen `main` or `master`, local first, then on a remote (`origin` before the others).
/// A chosen branch that no longer exists gives none, as does a repository without `main` or
/// `master`: the checked-out branch never stands in for it.
pub fn pick_trunk<'a>(refs: &'a [RefInfo], chosen: Option<&str>) -> Option<&'a RefInfo> {
    if let Some(chosen) = chosen {
        return refs.iter().find(|r| full_ref(r).as_deref() == Some(chosen));
    }
    let find = |kind: RefKind, name: &str| refs.iter().find(|r| r.kind == kind && r.name == name);
    let mut remotes: Vec<&str> = refs.iter().filter_map(|r| r.remote.as_deref()).collect();
    remotes.sort_by_key(|&r| (r != "origin", r));
    remotes.dedup();
    find(RefKind::Local, "main")
        .or_else(|| find(RefKind::Local, "master"))
        .or_else(|| {
            remotes.iter().find_map(|remote| {
                find(RefKind::Remote, &format!("{remote}/main"))
                    .or_else(|| find(RefKind::Remote, &format!("{remote}/master")))
            })
        })
}

impl Repo {
    /// The production branch chosen in Settings, as a full ref name; `None` is Auto.
    pub fn production_setting(&self) -> Option<String> {
        let repo = self.local();
        let config = repo.config_snapshot();
        config
            .string(PRODUCTION_KEY)
            .map(|value| value.to_str_lossy().trim().to_owned())
            .filter(|value| !value.is_empty())
    }
}

/// The tips of local branches and `HEAD`, and of remote-tracking branches.
fn unpushed_tips(refs: &[RefInfo], head: &HeadInfo) -> (Vec<ObjectId>, Vec<ObjectId>) {
    let ids = |kind: RefKind| -> Vec<ObjectId> {
        let mut ids: Vec<ObjectId> = refs
            .iter()
            .filter(|r| r.kind == kind)
            .filter_map(|r| ObjectId::from_hex(r.target.as_bytes()).ok())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    };
    let (mut local, remote) = (ids(RefKind::Local), ids(RefKind::Remote));
    if let Some(id) = head
        .commit
        .as_deref()
        .and_then(|h| ObjectId::from_hex(h.as_bytes()).ok())
        && !local.contains(&id)
    {
        local.push(id);
    }
    (local, remote)
}

/// Commits reachable from `local` but from none of `hidden`, the way `git rev-list <local>
/// --not <hidden>` finds them: newest first from both sides at once, stopping once all that is
/// left to walk is hidden. `None` past `limit` of them: a hidden commit older than what it
/// holds may still be to come. gix's own walk with hidden tips first walks all they reach,
/// which in a clone is the whole history: 0.7 s for 100,000 commits to find the one not pushed.
fn only_from(repo: &gix::Repository, local: &[ObjectId], hidden: &[ObjectId], limit: usize) -> Option<Vec<ObjectId>> {
    const LOCAL: u8 = 1;
    const HIDDEN: u8 = 2;
    const WALKED: u8 = 4;
    // As git: once only hidden commits are left, a few more are walked in case a commit's
    // time is older than its parent's.
    const SLOP: usize = 5;
    struct Seen {
        flags: u8,
        parents: Vec<ObjectId>,
    }
    let mut seen: HashMap<ObjectId, Seen> = HashMap::new();
    let mut queue: BinaryHeap<(i64, ObjectId)> = BinaryHeap::new();
    // Marks `id` and queues it the first time it is met; a commit that turns out hidden after
    // it was walked passes that on to the ancestors walked through it.
    let mark = |id: ObjectId, flag: u8, seen: &mut HashMap<ObjectId, Seen>, queue: &mut BinaryHeap<(i64, ObjectId)>| {
        if let Some(known) = seen.get_mut(&id) {
            let newly_hidden = flag == HIDDEN && known.flags & HIDDEN == 0;
            known.flags |= flag;
            if newly_hidden && known.flags & WALKED != 0 {
                let mut stack = known.parents.clone();
                while let Some(p) = stack.pop() {
                    if let Some(parent) = seen.get_mut(&p)
                        && parent.flags & HIDDEN == 0
                    {
                        parent.flags |= HIDDEN;
                        if parent.flags & WALKED != 0 {
                            stack.extend(parent.parents.iter().copied());
                        }
                    }
                }
            }
            return;
        }
        // Past a shallow clone's boundary the parent isn't there: as if there were none.
        let Ok(commit) = repo.find_commit(id) else {
            return;
        };
        let time = commit.time().map(|t| t.seconds).unwrap_or_default();
        let parents = commit.parent_ids().map(|p| p.detach()).collect();
        seen.insert(id, Seen { flags: flag, parents });
        queue.push((time, id));
    };
    for &id in hidden {
        mark(id, HIDDEN, &mut seen, &mut queue);
    }
    for &id in local {
        mark(id, LOCAL, &mut seen, &mut queue);
    }
    let mut found = Vec::new();
    let mut slop = SLOP;
    while let Some((_, id)) = queue.pop() {
        let (flags, parents) = {
            let commit = seen.get_mut(&id).expect("queued commits are seen");
            commit.flags |= WALKED;
            (commit.flags, commit.parents.clone())
        };
        let flag = if flags & HIDDEN != 0 { HIDDEN } else { LOCAL };
        for parent in parents {
            mark(parent, flag, &mut seen, &mut queue);
        }
        if flag == LOCAL {
            found.push(id);
            if found.len() > limit {
                return None;
            }
        }
        if queue.iter().all(|(_, id)| seen[id].flags & HIDDEN != 0) {
            slop -= 1;
            if slop == 0 {
                break;
            }
        } else {
            slop = SLOP;
        }
    }
    found.retain(|id| seen[id].flags & HIDDEN == 0);
    Some(found)
}

/// What the answers of [`Repo::settle_unpushed`] hold for: the tips both ways.
fn tips_key(local: &[ObjectId], remote: &[ObjectId]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (local, remote).hash(&mut hasher);
    hasher.finish()
}

impl Repo {
    /// Commits reachable from local branches or `HEAD` but from no remote-tracking branch, and
    /// whether some of `nodes` are still unknown. Without any remote-tracking branches nothing
    /// counts as unpushed. The walk stops past [`UNPUSHED_WALK`] commits, as when the
    /// remote-tracking branches are far behind or of another history: every commit it met is
    /// unpushed, and the loaded ones it did not meet take what [`Repo::settle_unpushed`] found
    /// for the same tips, or wait for it.
    fn unpushed_commits(
        &self,
        repo: &gix::Repository,
        refs: &[RefInfo],
        head: &HeadInfo,
        nodes: &[Node],
    ) -> Result<(HashSet<ObjectId>, bool)> {
        let (local, remote) = unpushed_tips(refs, head);
        if remote.is_empty() || local.is_empty() {
            return Ok((HashSet::new(), false));
        }
        if let Some(found) = only_from(repo, &local, &remote, UNPUSHED_WALK) {
            return Ok((found.into_iter().collect(), false));
        }
        // Many: gix's walk, which knows each commit it gives is unpushed, as far as the limit.
        let walk = repo
            .rev_walk(local.iter().copied())
            .with_hidden(remote.iter().copied())
            .all()
            .map_err(Error::git)?;
        let mut out = HashSet::new();
        for info in walk {
            out.insert(info.map_err(Error::git)?.id);
            if out.len() > UNPUSHED_WALK {
                break;
            }
        }
        if out.len() <= UNPUSHED_WALK {
            return Ok((out, false));
        }
        let answers = self.unpushed.0.lock().expect("unpushed lock");
        let known = (answers.tips == tips_key(&local, &remote)).then_some(&answers.unpushed);
        let mut pending = false;
        let mut extra = Vec::new();
        for node in nodes.iter().filter(|n| !out.contains(&n.id)) {
            match known.and_then(|k| k.get(&node.id)) {
                Some(true) => extra.push(node.id),
                Some(false) => {}
                None => pending = true,
            }
        }
        out.extend(extra);
        Ok((out, pending))
    }

    /// Which of `ids`, loaded commits the history could not tell, are unpushed: the whole walk
    /// of unpushed commits, which can be long, stopping when `cancel` is set or every one of
    /// `ids` is met. The answers are kept for the history's next load; whether any were added.
    pub fn settle_unpushed(&self, ids: &[String], cancel: &AtomicBool) -> Result<bool> {
        let (local, remote) = unpushed_tips(&self.refs()?, &self.head()?);
        let tips = tips_key(&local, &remote);
        let mut asked: HashMap<ObjectId, bool> = ids
            .iter()
            .filter_map(|id| ObjectId::from_hex(id.as_bytes()).ok())
            .map(|id| (id, false))
            .collect();
        {
            let answers = self.unpushed.0.lock().expect("unpushed lock");
            if answers.tips == tips {
                asked.retain(|id, _| !answers.unpushed.contains_key(id));
            }
        }
        if asked.is_empty() || remote.is_empty() || local.is_empty() {
            return Ok(false);
        }
        let repo = self.local();
        let walk = repo
            .rev_walk(local.iter().copied())
            .with_hidden(remote.iter().copied())
            .all()
            .map_err(Error::git)?;
        let mut left = asked.len();
        for (n, info) in walk.enumerate() {
            if n % 1024 == 0 && cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            if let Some(found) = asked.get_mut(&info.map_err(Error::git)?.id) {
                *found = true;
                left -= 1;
                if left == 0 {
                    break;
                }
            }
        }
        let mut answers = self.unpushed.0.lock().expect("unpushed lock");
        if answers.tips != tips {
            answers.tips = tips;
            answers.unpushed.clear();
        }
        answers.unpushed.extend(asked);
        answers.added += 1;
        Ok(true)
    }
}

/// Branch name that colors a line starting at a commit with these labels.
fn tip_name(labels: &[Label], remotes: &[String]) -> Option<String> {
    labels
        .iter()
        .find(|l| l.kind == RefKind::Local)
        .map(|l| l.name.clone())
        .or_else(|| {
            labels
                .iter()
                .find(|l| l.kind == RefKind::Remote)
                .map(|l| strip_remote(&l.name, remotes).to_owned())
        })
}

/// `origin/feature/x` -> `feature/x` for a known remote.
fn strip_remote<'a>(name: &'a str, remotes: &[String]) -> &'a str {
    remotes
        .iter()
        .find_map(|remote| {
            name.strip_prefix(remote.as_str())
                .and_then(|rest| rest.strip_prefix('/'))
        })
        .unwrap_or(name)
}

/// The branch a merge commit brought in, read from its default message.
fn merged_branch(summary: &str, remotes: &[String]) -> Option<String> {
    if let Some(rest) = summary.strip_prefix("Merge pull request #") {
        // "Merge pull request #12 from owner/branch"
        let from = rest.split_once(" from ")?.1;
        let branch = from.split_once('/').map_or(from, |(_, b)| b);
        return Some(branch.trim().to_owned());
    }
    for prefix in ["Merge branch '", "Merge remote-tracking branch '"] {
        if let Some(rest) = summary.strip_prefix(prefix) {
            let name = rest.split_once('\'')?.0;
            return Some(strip_remote(name, remotes).to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {

    fn branch(name: &str, kind: RefKind) -> RefInfo {
        RefInfo {
            name: name.to_owned(),
            kind,
            target: String::new(),
            remote: (kind == RefKind::Remote).then(|| name.split('/').next().unwrap_or_default().to_owned()),
            tracking: None,
        }
    }

    #[test]
    fn auto_production_prefers_local_then_origin_then_other_remotes() {
        let refs = [
            branch("upstream/main", RefKind::Remote),
            branch("origin/master", RefKind::Remote),
            branch("feature", RefKind::Local),
        ];
        assert_eq!(pick_trunk(&refs, None).map(|r| r.name.as_str()), Some("origin/master"));
        let refs = [
            branch("upstream/main", RefKind::Remote),
            branch("feature", RefKind::Local),
        ];
        assert_eq!(pick_trunk(&refs, None).map(|r| r.name.as_str()), Some("upstream/main"));
        let refs = [branch("master", RefKind::Local), branch("origin/main", RefKind::Remote)];
        assert_eq!(pick_trunk(&refs, None).map(|r| r.name.as_str()), Some("master"));
        let refs = [branch("feature", RefKind::Local)];
        assert!(pick_trunk(&refs, None).is_none());
    }

    #[test]
    fn a_chosen_production_branch_is_matched_by_its_full_ref() {
        let refs = [
            branch("main", RefKind::Local),
            branch("origin/develop", RefKind::Remote),
            branch("develop", RefKind::Local),
        ];
        let pick = |chosen| pick_trunk(&refs, Some(chosen)).map(|r| (r.name.as_str(), r.kind));
        assert_eq!(
            pick("refs/remotes/origin/develop"),
            Some(("origin/develop", RefKind::Remote))
        );
        assert_eq!(pick("refs/heads/develop"), Some(("develop", RefKind::Local)));
        assert_eq!(pick("refs/heads/gone"), None);
    }
    use super::*;

    #[test]
    fn merged_branch_names() {
        let remotes = vec!["origin".to_owned()];
        assert_eq!(
            merged_branch("Merge branch 'feature/x'", &remotes).as_deref(),
            Some("feature/x")
        );
        assert_eq!(merged_branch("Merge branch 'a' into b", &remotes).as_deref(), Some("a"));
        assert_eq!(
            merged_branch("Merge remote-tracking branch 'origin/fix/y'", &remotes).as_deref(),
            Some("fix/y")
        );
        assert_eq!(
            merged_branch("Merge pull request #405 from acme/fix/retry-backoff", &remotes).as_deref(),
            Some("fix/retry-backoff")
        );
        assert_eq!(merged_branch("Bump tokio", &remotes), None);
    }
}
