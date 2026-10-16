use std::collections::{BinaryHeap, HashMap, HashSet};
use std::sync::Arc;

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

/// Options for loading the history.
#[derive(Debug, Clone)]
pub struct HistoryOptions {
    /// Maximum number of commits to load, newest first.
    pub limit: usize,
    /// A commit that must be loaded even when it is older than `limit` commits: the history goes
    /// on to the end of the page it is on. Ignored when no branch, tag or `HEAD` reaches it.
    pub until: Option<String>,
}

impl Default for HistoryOptions {
    fn default() -> Self {
        HistoryOptions {
            limit: PAGE,
            until: None,
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
    /// Every working folder of the repository, the main one first. Just this one when there are
    /// no others, or when git can't list them.
    pub worktrees: Vec<Worktree>,
    /// The repository's submodules, in the order of `.gitmodules`.
    pub submodules: Vec<Submodule>,
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
    /// list them. The Changes panel shows these, so a reload asks git for them once.
    pub changes: Option<WorkingTree>,
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
    pub conflicted: usize,
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
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    workdir.hash(&mut hasher);
    options.limit.hash(&mut hasher);
    options.until.hash(&mut hasher);
    head.branch.hash(&mut hasher);
    head.commit.hash(&mut hasher);
    for r in refs {
        (&r.name, r.kind as u8, &r.target, &r.remote).hash(&mut hasher);
    }
    for stash in stashes {
        (&stash.id, stash.index).hash(&mut hasher);
    }
    remotes.hash(&mut hasher);
    (rebasing, changes, production, shallow, graph::branch_colors()).hash(&mut hasher);
    hasher.finish()
}

/// What the row of uncommitted changes says: the operation under way and the file counts.
fn fill_worktree_row(row: &mut HistoryRow, tree: &WorkingTree, operation: Option<&Operation>, branch: Option<&str>) {
    row.summary = operation
        .map_or("Uncommitted changes", |op| match op.kind {
            OperationKind::Merge => "Merge in progress",
            OperationKind::Squash => "Squash merge in progress",
            OperationKind::Rebase => "Rebase in progress",
            OperationKind::CherryPick => "Cherry-pick in progress",
            OperationKind::Revert => "Revert in progress",
            OperationKind::StashApply => "Stash apply in progress",
        })
        .to_owned();
    row.time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    row.graph.branch = branch.map(str::to_owned);
    row.worktree = Some(WorktreeSummary {
        files: tree.file_count(),
        staged: tree.staged.len(),
        unstaged: tree.unstaged.len(),
        conflicted: tree.conflicted.len(),
    });
}

impl Repo {
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
        let repo = self.local();
        let head = self.head()?;
        let mut refs = self.refs()?;
        // A failing `git for-each-ref` only hides the upstreams and their counts.
        let mut upstreams = self.branch_tracking().unwrap_or_default();
        for r in refs.iter_mut().filter(|r| r.kind == RefKind::Local) {
            r.tracking = upstreams.remove(&r.name);
        }
        let remotes = self.remotes();
        let stashes = self.stashes()?;
        // A failure to read the operation's state only hides its banner.
        let operation = self.operation().ok().flatten();
        let rebasing = operation.as_ref().is_some_and(|op| op.kind == OperationKind::Rebase);
        let bisect = self.bisect().ok().flatten();
        let worktrees = self.worktrees().unwrap_or_default();
        let submodules = self.submodules();
        // Uncommitted changes get a row of their own on top. A failing `git status` only hides the row.
        let changes = self.working_tree().ok();
        let dirty = changes.as_ref().is_some_and(|tree| !tree.is_empty());
        let production = self.production_setting();
        let shallow = repo.is_shallow();
        // A deeper fetch moves where the history stops without moving any ref.
        let boundary = self.shallow_stamp();

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
        );
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
        if let (Some(row), Some(tree)) = (rows.first_mut().filter(|r| r.worktree.is_some()), &changes) {
            fill_worktree_row(row, tree, operation.as_ref(), head.branch.as_deref());
        }

        let previous_branch = head.branch.is_none().then(|| self.previous_branch(&refs)).flatten();
        // The checked-out branch's upstream was read with the others. A failing `git for-each-ref`
        // only hides the ahead/behind counts.
        let tracking = match &head.branch {
            Some(branch) => refs
                .iter()
                .find(|r| r.kind == RefKind::Local && &r.name == branch)
                .and_then(|r| r.tracking.clone())
                .map(|mut t| {
                    if t.behind > 0 {
                        self.find_rewrite(branch, &mut t);
                    }
                    t
                }),
            None => None,
        };
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
            changes,
            snapshot: 0,
            key: key_text,
            same,
            pieces,
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

        let unpushed = unpushed_commits(repo, refs, head)?;

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
            });
            details.push((summary, author_name, author_email, author_time));
        }

        // Uncommitted changes go on a side line down to HEAD, in the color of HEAD's line.
        let head_id = head
            .commit
            .as_deref()
            .and_then(|h| ObjectId::from_hex(h.as_bytes()).ok());
        let head_row = head_id.and_then(|id| row_of.get(&id).copied());
        let mut worktree_row = None;
        let mut graph = if dirty {
            // One layout with the row of uncommitted changes on top: it takes the color of
            // HEAD's line as the layout finds it, or of the branch about to be born.
            let color = match head_row {
                Some(_) => None,
                None => Some(head.branch.as_deref().map_or(graph::TRUNK_COLOR, graph::color_for_name)),
            };
            let mut shifted = Vec::with_capacity(graph_input.len() + 1);
            shifted.push(GraphCommit {
                parents: vec![head_row.map(|r| r + 1)],
                side: true,
                color,
                unpushed: true,
                worktree: true,
                ..Default::default()
            });
            shifted.extend(graph_input.into_iter().map(|mut c| {
                c.parents = c.parents.into_iter().map(|p| p.map(|r| r + 1)).collect();
                c
            }));
            graph::layout(&shifted, head_row.map(|r| r + 1))
        } else {
            graph::layout(&graph_input, head_row)
        };
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
                    conflicted: 0,
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
        })
    }
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

/// Commits reachable from local branches or `HEAD` but from no remote-tracking branch.
/// Without any remote-tracking branches nothing counts as unpushed.
fn unpushed_commits(repo: &gix::Repository, refs: &[RefInfo], head: &HeadInfo) -> Result<HashSet<ObjectId>> {
    let ids = |kind: RefKind| -> Vec<ObjectId> {
        refs.iter()
            .filter(|r| r.kind == kind)
            .filter_map(|r| ObjectId::from_hex(r.target.as_bytes()).ok())
            .collect()
    };
    let remote = ids(RefKind::Remote);
    if remote.is_empty() {
        return Ok(HashSet::new());
    }
    let mut local = ids(RefKind::Local);
    if let Some(id) = head
        .commit
        .as_deref()
        .and_then(|h| ObjectId::from_hex(h.as_bytes()).ok())
    {
        local.push(id);
    }
    let walk = repo.rev_walk(local).with_hidden(remote).all().map_err(Error::git)?;
    let mut out = HashSet::new();
    for info in walk {
        out.insert(info.map_err(Error::git)?.id);
    }
    Ok(out)
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
