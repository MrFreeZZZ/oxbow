use std::collections::{BinaryHeap, HashMap, HashSet};

use gix::ObjectId;
use gix::bstr::ByteSlice;
use gix::revision::walk::Sorting;
use gix::traverse::commit::simple::CommitTimeOrder;
use serde::Serialize;

use crate::error::{Error, Result};
use crate::graph::{self, GraphCommit, RowLayout};
use crate::repo::{HeadInfo, RefInfo, RefKind, Repo, StashInfo};

/// Options for loading the history.
#[derive(Debug, Clone)]
pub struct HistoryOptions {
    /// Maximum number of commits to load, newest first.
    pub limit: usize,
}

impl Default for HistoryOptions {
    fn default() -> Self {
        HistoryOptions { limit: 10_000 }
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
    pub refs: Vec<RefInfo>,
    pub remotes: Vec<String>,
    /// Stash entries, newest first. Each one is also a row of the graph.
    pub stashes: Vec<StashInfo>,
    pub rows: Vec<HistoryRow>,
    /// More commits exist beyond `rows`.
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
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
    pub graph: RowLayout,
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

impl Repo {
    /// Load the commit history of all branches and tags with its graph layout.
    pub fn history(&self, options: &HistoryOptions) -> Result<History> {
        let repo = self.local();
        let head = self.head()?;
        let refs = self.refs()?;
        let remotes = self.remotes();
        let stashes = self.stashes()?;

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
            for info in walk {
                let info = info.map_err(Error::git)?;
                if nodes.len() == options.limit {
                    truncated = true;
                    break;
                }
                nodes.push(Node {
                    id: info.id,
                    parents: info.parent_ids.iter().copied().collect(),
                    time: info.commit_time.unwrap_or_default(),
                });
            }
        }
        // Stashes hang off the commit they were made on. Only that first parent is drawn: the other
        // parents (the index and the untracked files) are internal to the stash.
        let mut stash_ids = HashSet::new();
        for stash in &stashes {
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
        let trunk_ref = pick_trunk(&refs, &head);
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

        let unpushed = unpushed_commits(&repo, &refs, &head)?;

        // Labels per commit.
        let mut labels: HashMap<ObjectId, Vec<Label>> = HashMap::new();
        for r in &refs {
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
        for stash in &stashes {
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

        let mut graph_input = Vec::with_capacity(nodes.len());
        let mut details = Vec::with_capacity(nodes.len());
        for (row, node) in nodes.iter().enumerate() {
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
            let author_time = author.time().map(|t| t.seconds).unwrap_or(node.time);
            let row_labels = labels.get(&node.id);
            graph_input.push(GraphCommit {
                parents: node.parents.iter().map(|p| row_of.get(p).copied()).collect(),
                trunk: on_trunk[row],
                tip_name: row_labels.and_then(|l| tip_name(l, &remotes)),
                merged_name: (node.parents.len() > 1)
                    .then(|| merged_branch(&summary, &remotes))
                    .flatten(),
                unpushed: unpushed.contains(&node.id) || stash_ids.contains(&node.id),
                stash: stash_ids.contains(&node.id),
            });
            details.push((
                summary,
                author.name.to_str_lossy().into_owned(),
                author.email.to_str_lossy().into_owned(),
                author_time,
            ));
        }

        let layout = graph::layout(&graph_input);
        // Trunk commits belong to the trunk branch, named without its remote (`origin/main` -> `main`).
        let trunk_name = trunk_ref.map(|r| match r.kind {
            RefKind::Remote => strip_remote(&r.name, &remotes).to_owned(),
            _ => r.name.clone(),
        });
        let rows = nodes
            .iter()
            .zip(details)
            .zip(layout)
            .enumerate()
            .map(|(row, ((node, (summary, author_name, author_email, time)), graph))| {
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
                    graph,
                }
            })
            .collect();

        Ok(History {
            head,
            trunk: trunk_ref.map(|r| r.name.clone()),
            trunk_tip_row,
            refs,
            remotes,
            stashes,
            rows,
            truncated,
        })
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

/// The branch drawn as the trunk: `main` or `master`, local first, then the remote's default.
fn pick_trunk<'a>(refs: &'a [RefInfo], head: &HeadInfo) -> Option<&'a RefInfo> {
    let find = |kind: RefKind, name: &str| refs.iter().find(|r| r.kind == kind && r.name == name);
    find(RefKind::Local, "main")
        .or_else(|| find(RefKind::Local, "master"))
        .or_else(|| find(RefKind::Remote, "origin/main"))
        .or_else(|| find(RefKind::Remote, "origin/master"))
        .or_else(|| head.branch.as_deref().and_then(|b| find(RefKind::Local, b)))
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
