//! Editing a stack: the branches built one on another on top of the trunk, rewritten as a whole
//! by an interactive rebase that is planned on screen. Commits can be reordered, reworded,
//! squashed, dropped or moved into another branch of the stack, and the stack can move onto the
//! trunk's newest commit.
//!
//! Oxbow writes the rebase's todo list itself and hands it to git as the sequence editor, with
//! an `update-ref` line under each branch's last commit, so every branch of the stack moves with
//! its commits. Before anything runs, a dry run replays the plan in memory with
//! `git merge-tree`: it finds the first conflict, the commits that would become empty and which
//! branches would need a force push.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::action::{BranchPush, RemoteBranch};
use crate::cli::{GitCommand, shell_quote};
use crate::error::{Error, Result};
use crate::plan::paragraphs;
use crate::remote::CommitBrief;
use crate::repo::{RefKind, Repo};
use crate::split::{StackSplit, split_of, split_part};

/// More commits than this are not edited on screen.
const MAX_COMMITS: usize = 300;

/// Where the todo list waits for git, inside the git directory.
const TODO_FILE: &str = "oxbow/rebase-todo";

/// The branch to go back to once a stack rebase that stopped is over: the top, then that branch.
const RETURN_FILE: &str = "oxbow/edit-stack-return";

/// The restack under way, as JSON: shared by every worktree, like the branches it moves.
const RESTACK_FILE: &str = "oxbow/restack.json";

/// A stack of branches on top of the trunk, as the Edit Stack screen shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stack {
    /// The branch at the top of the stack, the one the rebase rewrites.
    pub top: String,
    /// The checked-out branch.
    pub head: Option<String>,
    /// The branch the stack is built on, e.g. `main` or `origin/main`.
    pub trunk: String,
    /// The trunk commit the stack starts from.
    pub base: CommitBrief,
    /// The trunk's newest commit.
    pub trunk_tip: CommitBrief,
    /// Commits on the trunk since the stack's base.
    pub newer: usize,
    /// The stack's commits, newest first.
    pub commits: Vec<StackCommit>,
    /// Its branches, top first. A branch owns the commits under it down to the next one.
    pub branches: Vec<StackBranch>,
    /// Branches built on the stack that fork from it, so the rebase leaves them where they are.
    pub left_behind: Vec<String>,
    /// There are uncommitted changes; the rebase puts them aside and back.
    pub dirty: bool,
    /// Merges of the trunk into the stack, oldest first. They only bring in trunk commits the
    /// stack is replayed on anyway, so the rebase leaves them out and the line comes out straight.
    pub merges: Vec<StackMerge>,
}

/// A merge commit of the trunk into the stack, left out by the rebase.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StackMerge {
    pub id: String,
    pub summary: String,
    /// The branch of the stack it is on.
    pub branch: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StackCommit {
    pub id: String,
    pub summary: String,
    /// The whole message.
    pub message: String,
    pub author_name: String,
    pub time: i64,
    pub additions: u32,
    pub deletions: u32,
    /// The branch of the stack it belongs to.
    pub branch: String,
    /// A remote branch has it, so rewriting it means a force push.
    pub pushed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StackBranch {
    pub name: String,
    /// The commit it points at.
    pub tip: String,
    /// The stack commit its line sits above: the tip, or for a tip at a merge of the trunk, the
    /// commit the merge was made on.
    pub at: String,
    /// Its upstream, when it has one that still exists.
    pub upstream: Option<RemoteBranch>,
    /// Commits the upstream does not have yet.
    pub ahead: u32,
}

/// What happens to one commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepAction {
    Pick,
    /// Pick with a new message.
    Reword,
    /// Pick, then stop so the commit can be changed.
    Edit,
    /// Fold into the commit before it, both messages kept.
    Squash,
    /// Fold into the commit before it, keeping only that one's message.
    Fixup,
    Drop,
}

/// One line of a plan, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum StackStep {
    /// `message` is the commit's new message: for a reword, or the result of the squashes into it.
    Commit {
        id: String,
        action: StepAction,
        #[serde(default)]
        message: Option<String>,
    },
    /// The branch ends at the commit before this line.
    Branch { name: String },
}

/// A stack as it should be after the rebase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackPlan {
    /// The branch the rebase rewrites, the stack's top.
    pub top: String,
    /// The commit the stack starts from now.
    pub base: String,
    /// The commit it should start from; `base` keeps it where it is.
    pub onto: String,
    /// How `onto` is called in the command, e.g. `main`; a short sha without it.
    #[serde(default)]
    pub onto_name: Option<String>,
    /// The branch to check out again afterwards, when it is not the top.
    #[serde(default)]
    pub head: Option<String>,
    /// A commit split in two has a second step, its new commit, named by [`split_part`].
    pub steps: Vec<StackStep>,
    /// Commits whose picked changes become a commit of their own right after them.
    #[serde(default)]
    pub splits: Vec<StackSplit>,
}

/// What a restack after a merged pull request does, kept in the git directory from its rebase
/// until its last step: when the push or the new base on GitHub fails, e.g. without network, it
/// goes on from that step, also after a restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Restack {
    /// The merged pull request whose commits leave the stack.
    pub merged: u64,
    /// The branch it merged into, the new base of the pull requests that were built on it.
    pub base: String,
    /// The branches the rebase moves.
    pub branches: Vec<String>,
    /// The merged commits the rebase leaves out.
    pub drop: Vec<String>,
    /// Where the moved branches are pushed, each to its branch there.
    pub remote: Option<String>,
    pub push: Vec<BranchPush>,
    /// Pull requests that went into the merged branch and go into `base` next.
    pub retarget: Vec<u64>,
    /// The step that failed last, until it runs again.
    pub failed: Option<RestackFailure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RestackFailure {
    pub step: RestackStep,
    /// What went wrong, in git's or GitHub's words.
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RestackStep {
    Push,
    Base,
}

/// A kept restack and how far it got here. The base on GitHub is the screen's to check.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestackState {
    pub restack: Restack,
    /// The rebase stopped, e.g. on a conflict, and waits to be continued.
    pub rebasing: bool,
    /// Pushes still to make: branches whose remote branch isn't at their tip.
    pub unpushed: Vec<String>,
}

/// A branch that has a merged pull request, for a restack to move onto.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Landing {
    pub name: String,
    pub tip: CommitBrief,
}

/// What a plan does, worked out by a dry run.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StackPreview {
    /// The plan changes anything at all.
    pub changed: bool,
    /// Commits that get a new sha.
    pub rewritten: usize,
    pub squashed: usize,
    pub dropped: usize,
    pub reworded: usize,
    /// Commits in a new place or a new branch.
    pub moved: usize,
    /// Commits the rebase stops at, to be changed by hand.
    pub edits: usize,
    /// Commits split in two.
    pub split: usize,
    /// Commits that end up with no changes (the trunk has them already): left out.
    pub emptied: Vec<String>,
    /// The first commit that would stop on conflicts; the dry run stops there.
    pub conflict: Option<StackConflict>,
    /// The branches afterwards, top first.
    pub branches: Vec<BranchAfter>,
    /// A commit that becomes empty stays because something folds into it.
    #[serde(skip)]
    keep_empty: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StackConflict {
    pub id: String,
    pub summary: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchAfter {
    pub name: String,
    /// Its own commits afterwards.
    pub commits: usize,
    /// It will point at another commit.
    pub moves: bool,
    /// The commit it points at afterwards, as it is now: one of the stack's, which the rebase
    /// gives a new sha when `moves`, or the commit the stack starts from when it ends up empty.
    pub tip_from: String,
    /// That commit keeps its sha: the branch ends on it as it is.
    pub same_sha: bool,
    /// Its remote branch has commits the rebase replaces, so pushing it needs a force push.
    pub force_push: Option<RemoteBranch>,
}

/// A line of the todo list as Oxbow lays it out, oldest first.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Emit {
    Pick {
        id: String,
        edit: bool,
    },
    /// A squash or fixup, right after the commit it folds into.
    Fold {
        id: String,
        squash: bool,
    },
    Drop {
        id: String,
    },
    Ref {
        name: String,
    },
}

/// A stack commit as git lists it, for working out a plan.
#[derive(Debug, Clone)]
struct Listed {
    id: String,
    parent: String,
    /// A merge's other parents.
    others: Vec<String>,
    tree: String,
    message: String,
}

/// What `git log` says about a range, oldest first.
struct Range {
    /// Its commits without the merges.
    commits: Vec<Listed>,
    merges: Vec<Listed>,
    base_tree: String,
    /// Commits split in two, which their parts stand in for in `commits`.
    split: Vec<Listed>,
}

/// A plan with its splits made: the steps and the range name the parts by their commits.
struct Resolved {
    plan: StackPlan,
    range: Range,
    /// Each part's commit, by its step id: the split commit's own id for the rest, and
    /// [`split_part`] of it for the new commit.
    commit_of: HashMap<String, String>,
    /// And back, for what the dry run tells the screen.
    step_of: HashMap<String, String>,
}

impl Resolved {
    fn shown(&self, id: &str) -> String {
        self.step_of.get(id).cloned().unwrap_or_else(|| id.to_owned())
    }
}

impl Repo {
    /// The stack `branch` belongs to (the checked-out branch without one): its commits since it
    /// left the trunk and the branches among them, up to the last branch built on it in a line.
    pub fn stack(&self, branch: Option<&str>) -> Result<Stack> {
        let head = self.head()?;
        let branch = match branch {
            Some(branch) => branch.to_owned(),
            None => head
                .branch
                .clone()
                .ok_or_else(|| Error::Git("check out a branch first: HEAD is not on one".into()))?,
        };
        let refs = self.refs()?;
        let production = self.production_setting();
        let trunk = crate::history::pick_trunk(&refs, production.as_deref()).ok_or_else(|| {
            Error::Git("there is no main branch to stack on: no main or master, and none chosen in Settings".into())
        })?;
        if trunk.kind == RefKind::Local && trunk.name == branch {
            return Err(Error::Git(format!(
                "{branch} is the main branch: there is no stack on top of it to edit"
            )));
        }
        let trunk = trunk.clone();
        let base = self
            .run(&GitCommand::new(["merge-base", &trunk.name, &branch]))
            .map(|out| out.stdout.trim().to_owned())
            .map_err(|_| Error::Git(format!("{branch} has no commit in common with {}", trunk.name)))?;
        if self.is_ancestor(&branch, &trunk.name) {
            return Err(Error::Git(format!(
                "{branch} has no commits of its own: {} has them all",
                trunk.name
            )));
        }

        // Branches built on `branch` in a line make the stack taller; a fork stops it.
        let local: Vec<_> = refs.iter().filter(|r| r.kind == RefKind::Local).collect();
        let containing = self.run(&GitCommand::new([
            "for-each-ref",
            "--format=%(refname:short)",
            "--contains",
            &branch,
            "refs/heads",
        ]))?;
        let mut above: Vec<(usize, String)> = Vec::new();
        for name in containing.stdout.lines().map(str::trim) {
            if name.is_empty() || name == branch || name == trunk.name {
                continue;
            }
            let count = self.count(&format!("{branch}..{name}"))?;
            if count > 0 {
                above.push((count, name.to_owned()));
            }
        }
        above.sort();
        // Climb while the branches above all go one way; at a fork the stack ends.
        let mut top = branch.clone();
        loop {
            let higher: Vec<&String> = above
                .iter()
                .map(|(_, name)| name)
                .filter(|name| **name != top && self.is_ancestor(&top, name))
                .collect();
            let Some(next) = higher.first() else { break };
            let one_way = higher.iter().all(|name| self.is_ancestor(next, name));
            if !one_way {
                break;
            }
            // A merge Edit Stack can't leave out ends it too: the branches below stay editable.
            let merges = self.list_range(&top, next)?.merges;
            if self.blocking_merge(&merges, &base, &trunk.name, next).is_some() {
                break;
            }
            top = (*next).clone();
        }
        let range = format!("{base}..{top}");
        let listed = self.list_range(&base, &top)?;
        if let Some((why, merge)) = self.blocking_merge(&listed.merges, &base, &trunk.name, &top) {
            return Err(Error::Git(why + &self.below_merge(merge, &base)));
        }
        if listed.commits.len() > MAX_COMMITS {
            return Err(Error::Git(format!(
                "{} commits are more than Edit Stack shows; rebase them in a terminal",
                listed.commits.len()
            )));
        }
        let stats = self.range_stats(&range)?;
        let unpushed: HashSet<String> = if self.remotes().is_empty() {
            HashSet::new()
        } else {
            self.run(&GitCommand::new(["rev-list", &range, "--not", "--remotes"]))?
                .stdout
                .lines()
                .map(str::to_owned)
                .collect()
        };
        let has_remotes = !self.remotes().is_empty();

        // The branches of the stack: the top, and every local branch whose tip is in it.
        let ids: HashSet<&str> = listed.commits.iter().map(|c| c.id.as_str()).collect();
        let first_parent: HashMap<&str, &str> = listed
            .merges
            .iter()
            .map(|m| (m.id.as_str(), m.parent.as_str()))
            .collect();
        let line_of = |tip: &str| {
            let mut at = tip;
            while let Some(parent) = first_parent.get(at) {
                at = parent;
            }
            at.to_owned()
        };
        let tracking = self.branch_tracking().unwrap_or_default();
        let mut branches: Vec<StackBranch> = local
            .iter()
            .filter(|r| r.name != trunk.name && (r.name == top || ids.contains(line_of(&r.target).as_str())))
            .map(|r| {
                let up = tracking.get(&r.name).filter(|t| !t.gone);
                StackBranch {
                    name: r.name.clone(),
                    tip: r.target.clone(),
                    at: line_of(&r.target),
                    upstream: up.map(|t| RemoteBranch {
                        remote: t.remote.clone(),
                        branch: t.branch.clone(),
                    }),
                    ahead: up.map_or(0, |t| t.ahead),
                }
            })
            .collect();
        let position: HashMap<&str, usize> = listed
            .commits
            .iter()
            .enumerate()
            .map(|(i, c)| (c.id.as_str(), i))
            .collect();
        // Top first; the top branch above any other at its commit.
        branches.sort_by(|a, b| {
            let pa = position.get(a.at.as_str()).copied().unwrap_or(usize::MAX);
            let pb = position.get(b.at.as_str()).copied().unwrap_or(usize::MAX);
            pb.cmp(&pa)
                .then_with(|| (b.name == top).cmp(&(a.name == top)))
                .then_with(|| b.name.cmp(&a.name))
        });

        // Each commit belongs to the lowest branch at or above it.
        let mut commits = Vec::with_capacity(listed.commits.len());
        for (i, c) in listed.commits.iter().enumerate() {
            let owner = branches
                .iter()
                .rev()
                .find(|b| position.get(b.at.as_str()).is_some_and(|&p| p >= i))
                .map_or(top.clone(), |b| b.name.clone());
            let (additions, deletions) = stats.get(&c.id).copied().unwrap_or((0, 0));
            let brief = self.brief_of(&c.id)?;
            commits.push(StackCommit {
                id: c.id.clone(),
                summary: brief.summary,
                message: c.message.clone(),
                author_name: brief.author_name,
                time: brief.time,
                additions,
                deletions,
                branch: owner,
                pushed: has_remotes && !unpushed.contains(&c.id),
            });
        }
        commits.reverse();

        // Branches that share some of the stack's commits but are not in it keep the old ones.
        let mut left_behind = Vec::new();
        if let Some(oldest) = listed.commits.first() {
            let out = self.run(&GitCommand::new([
                "for-each-ref",
                "--format=%(refname:short)",
                "--contains",
                &oldest.id,
                "refs/heads",
            ]))?;
            left_behind = out
                .stdout
                .lines()
                .map(str::trim)
                .filter(|name| !name.is_empty() && *name != trunk.name && !branches.iter().any(|b| b.name == *name))
                .map(str::to_owned)
                .collect();
            left_behind.sort();
        }

        // A merge belongs to the branch at it, else to the branch of the first commit above it.
        let merges = listed
            .merges
            .iter()
            .map(|m| {
                let at = branches.iter().rev().find(|b| b.tip == m.id).map(|b| b.name.clone());
                let branch = at.unwrap_or_else(|| {
                    listed
                        .commits
                        .iter()
                        .find(|c| c.parent == m.id)
                        .and_then(|c| commits.iter().find(|s| s.id == c.id))
                        .map_or(top.clone(), |c| c.branch.clone())
                });
                StackMerge {
                    id: m.id.clone(),
                    summary: m.message.lines().next().unwrap_or_default().to_owned(),
                    branch,
                }
            })
            .collect();

        let newer = self.count(&format!("{base}..{}", trunk.target))?;
        let dirty = self.tracked_changes()?;
        Ok(Stack {
            top,
            head: head.branch,
            trunk: trunk.name.clone(),
            base: self.brief_of(&base)?,
            trunk_tip: self.brief_of(&trunk.target)?,
            newer,
            commits,
            branches,
            left_behind,
            dirty,
            merges,
        })
    }

    /// Why Edit Stack can't leave out one of `merges`, the merge commits on `top` since it left
    /// the trunk at `base`, if it can't, in paragraphs, with the merge. It can when a merge only
    /// brought in trunk commits the stack starts from anyway, and changed nothing of its own then.
    fn blocking_merge<'a>(
        &self,
        merges: &'a [Listed],
        base: &str,
        trunk: &str,
        top: &str,
    ) -> Option<(String, &'a Listed)> {
        merges.iter().find_map(|m| {
            let summary = m.message.lines().next().unwrap_or_default();
            let named = format!("The merge {} “{summary}” on {top}", short(&m.id));
            if !m.others.iter().all(|p| self.is_ancestor(p, base)) {
                let why = format!(
                    "{named} brings in commits {trunk} doesn't have.\n\n\
                     Edit Stack replays commits one by one on a straight line, so it would flatten this merge \
                     and take those commits into the stack as if they were its own.\n\n\
                     Merge that branch into {trunk} first, or rebase in a terminal if flattening it is what \
                     you want."
                );
                return Some((why, m));
            }
            let own_changes = match self.spawn(
                &GitCommand::new(["merge-tree", "--write-tree", "--no-messages", &m.parent, &m.others[0]]),
                None,
            ) {
                Ok(out) if out.status.success() && m.others.len() == 1 => {
                    String::from_utf8_lossy(&out.stdout).trim() != m.tree
                }
                _ => true,
            };
            own_changes.then(|| {
                let why = format!(
                    "{named} brings in {trunk} and also changes files of its own, usually to settle \
                     conflicts.\n\n\
                     Edit Stack replays commits on a straight line without such merges, so those changes would \
                     be lost.\n\n\
                     Rebase {top} onto {trunk} in a terminal and settle the conflicts there."
                );
                (why, m)
            })
        })
    }

    /// Where Edit Stack still opens below `merge`: the branches with commits of their own under it.
    fn below_merge(&self, merge: &Listed, base: &str) -> String {
        let names: Vec<String> = self
            .run(&GitCommand::new([
                "for-each-ref",
                "--format=%(refname:short)",
                "--merged",
                &merge.parent,
                "--no-merged",
                base,
                "refs/heads",
            ]))
            .map(|out| out.stdout.lines().map(str::to_owned).collect())
            .unwrap_or_default();
        match names.as_slice() {
            [] => String::new(),
            [one] => format!("\n\nEdit Stack still opens on {one}, below the merge."),
            [rest @ .., last] => format!(
                "\n\nEdit Stack still opens on {} and {last}, below the merge.",
                rest.join(", ")
            ),
        }
    }

    /// Replay `plan` in memory: what it changes, where it would stop, what to push afterwards.
    pub fn stack_preview(&self, plan: &StackPlan) -> Result<StackPreview> {
        let range = self.check_plan(plan)?;
        let resolved = self.resolve_splits(plan, range)?;
        let (plan, range) = (&resolved.plan, &resolved.range);
        let emits = lay_out(plan)?;
        let by_id: HashMap<&str, &Listed> = range.commits.iter().map(|c| (c.id.as_str(), c)).collect();
        let messages = group_messages(plan, &emits, &by_id, &resolved.commit_of);
        let stack = self.stack(Some(&plan.top))?;
        let mut preview = StackPreview {
            split: plan.splits.len(),
            ..StackPreview::default()
        };

        // The dry run: `cur` is where the rebase would be after each line.
        let mut changed = plan.onto != plan.base;
        let mut cur = plan.onto.clone();
        let mut cur_tree = self.tree_of(&plan.onto)?;
        let mut cur_parent: Option<(String, String)> = None;
        let mut tips: HashMap<String, String> = HashMap::new();
        let mut stopped = false;
        for (i, emit) in emits.iter().enumerate() {
            match emit {
                Emit::Pick { id, edit } => {
                    if *edit {
                        preview.edits += 1;
                    }
                    if stopped {
                        // Past a conflict the rest is replayed once it is resolved.
                        preview.rewritten += 1;
                        continue;
                    }
                    let c = by_id[id.as_str()];
                    if messages.contains_key(&i) {
                        changed = true;
                    }
                    if !changed && c.parent == cur {
                        cur_parent = Some((cur.clone(), cur_tree.clone()));
                        cur = c.id.clone();
                        cur_tree = c.tree.clone();
                        continue;
                    }
                    changed = true;
                    match self.dry_pick(c, &cur)? {
                        Err(files) => {
                            preview.conflict = Some(self.conflict_at(c, files)?);
                            preview.rewritten += 1;
                            stopped = true;
                        }
                        Ok(tree) => {
                            let was_empty = range_tree(range, &c.parent) == c.tree;
                            if tree == cur_tree && !was_empty {
                                // Something folding into it still needs it: it stays, empty.
                                if has_folds(&emits, i) {
                                    preview.keep_empty = true;
                                } else {
                                    preview.emptied.push(c.id.clone());
                                    continue;
                                }
                            }
                            preview.rewritten += 1;
                            let next = self.dry_commit(&tree, &cur)?;
                            cur_parent = Some((cur.clone(), cur_tree.clone()));
                            cur = next;
                            cur_tree = tree;
                        }
                    }
                }
                Emit::Fold { id, .. } => {
                    changed = true;
                    preview.squashed += 1;
                    if stopped {
                        continue;
                    }
                    let c = by_id[id.as_str()];
                    match self.dry_pick(c, &cur)? {
                        Err(files) => {
                            preview.conflict = Some(self.conflict_at(c, files)?);
                            stopped = true;
                        }
                        Ok(tree) => {
                            let (parent, _) = cur_parent.clone().expect("a fold follows a pick");
                            // The commit it folds into was not rewritten yet when it was unchanged.
                            if !is_dry(&cur, range) {
                                preview.rewritten += 1;
                            }
                            cur = self.dry_commit(&tree, &parent)?;
                            cur_tree = tree;
                        }
                    }
                }
                Emit::Drop { .. } => {
                    changed = true;
                    preview.dropped += 1;
                }
                Emit::Ref { name } => {
                    tips.insert(name.clone(), cur.clone());
                }
            }
        }
        tips.insert(plan.top.clone(), cur.clone());
        preview.reworded = plan
            .steps
            .iter()
            .filter(|s| {
                matches!(s, StackStep::Commit { id, action: StepAction::Reword, message: Some(m) }
                    if by_id.get(id.as_str()).is_some_and(|c| c.message.trim() != m.trim()))
            })
            .count();
        let mut original: HashMap<&str, &str> = stack
            .commits
            .iter()
            .map(|c| (c.id.as_str(), c.branch.as_str()))
            .collect();
        // The parts of a split commit start in its branch.
        for (step, commit) in &resolved.commit_of {
            let of = split_of(step).unwrap_or(step);
            if let Some(branch) = stack.commits.iter().find(|c| c.id == of) {
                original.insert(commit.as_str(), branch.branch.as_str());
            }
        }
        preview.moved = moved(range, plan, &emits, &original);
        preview.changed = changed || preview.moved > 0;

        // Branches afterwards, top first, with what they own.
        let mut owned: HashMap<String, usize> = HashMap::new();
        let mut since = 0;
        let empty: HashSet<&str> = preview.emptied.iter().map(String::as_str).collect();
        for emit in &emits {
            match emit {
                Emit::Pick { id, .. } if !empty.contains(id.as_str()) => since += 1,
                Emit::Ref { name } => {
                    owned.insert(name.clone(), since);
                    since = 0;
                }
                _ => {}
            }
        }
        owned.insert(plan.top.clone(), since);
        // What each branch points at afterwards, by the commits as they are now.
        let mut tip_from: HashMap<String, String> = HashMap::new();
        let mut last = plan.onto.clone();
        for emit in &emits {
            match emit {
                Emit::Pick { id, .. } if !empty.contains(id.as_str()) => last = id.clone(),
                Emit::Ref { name } => {
                    tip_from.insert(name.clone(), last.clone());
                }
                _ => {}
            }
        }
        tip_from.insert(plan.top.clone(), last);
        for branch in &stack.branches {
            let Some(new_tip) = tips.get(&branch.name) else {
                continue;
            };
            let moves = !stopped && *new_tip != branch.tip;
            let force_push = match &branch.upstream {
                Some(up) if moves => {
                    let remote = format!("refs/remotes/{}/{}", up.remote, up.branch);
                    (!self.is_ancestor(&remote, new_tip)).then(|| up.clone())
                }
                _ => None,
            };
            preview.branches.push(BranchAfter {
                name: branch.name.clone(),
                commits: owned.get(&branch.name).copied().unwrap_or(0),
                moves: moves || (stopped && preview.changed),
                same_sha: !stopped && tip_from.get(&branch.name) == Some(new_tip),
                tip_from: tip_from
                    .get(&branch.name)
                    .cloned()
                    .unwrap_or_else(|| branch.tip.clone()),
                force_push,
            });
        }
        // The screen knows the parts of a split by their steps.
        for id in &mut preview.emptied {
            *id = resolved.shown(id);
        }
        if let Some(conflict) = &mut preview.conflict {
            conflict.id = resolved.shown(&conflict.id);
        }
        for branch in &mut preview.branches {
            branch.tip_from = resolved.shown(&branch.tip_from);
        }
        Ok(preview)
    }

    /// The rebase that carries out `plan`, and the switch back to the branch that was checked out.
    pub(crate) fn plan_edit_stack(&self, asked: &StackPlan) -> Result<Vec<GitCommand>> {
        let range = self.check_plan(asked)?;
        let resolved = self.resolve_splits(asked, range)?;
        let (plan, range) = (&resolved.plan, &resolved.range);
        let emits = lay_out(plan)?;
        let by_id: HashMap<&str, &Listed> = range.commits.iter().map(|c| (c.id.as_str(), c)).collect();
        let messages = group_messages(plan, &emits, &by_id, &resolved.commit_of);
        let preview = self.stack_preview(asked)?;
        let emptied: HashSet<String> = preview
            .emptied
            .into_iter()
            .map(|id| resolved.commit_of.get(&id).cloned().unwrap_or(id))
            .collect();
        let todo = todo_text(&emits, &messages, &by_id, &emptied);
        let file = self.git_file(TODO_FILE)?;
        // An empty commit that something folds into stays, so the fold has a place to go.
        let keep_empty = preview.keep_empty;

        let mut args = vec!["rebase".to_owned(), "--interactive".to_owned()];
        if self.tracked_changes()? {
            args.push("--autostash".to_owned());
        }
        if keep_empty {
            args.push("--empty=keep".to_owned());
        }
        // The checked commit itself, not its name: a branch can move between the preview and here.
        let onto_shown = match &plan.onto_name {
            Some(name) => format!("{name} ({})", short(&plan.onto)),
            None => short(&plan.onto),
        };
        if plan.onto != plan.base {
            args.extend(["--onto".to_owned(), plan.onto.clone()]);
        }
        // Whole shas in commands: seven characters can match more than one object.
        args.push(plan.base.clone());
        args.push(plan.top.clone());
        let mut comment = if plan.onto != plan.base {
            format!("--onto {onto_shown}: replay the stack on top of it")
        } else {
            "replays the stack where it is, by the list below".to_owned()
        };
        if args.iter().any(|a| a == "--autostash") {
            comment.push_str("; --autostash: uncommitted changes go aside and come back");
        }
        for split in &plan.splits {
            let part = |step: &str| short(resolved.commit_of.get(step).map_or(step, String::as_str));
            comment.push_str(&format!(
                "; {} is split first, with git commit-tree: {} keeps the rest of it, {} gets the picked changes",
                short(&split.id),
                part(&split.id),
                part(&split_part(&split.id)),
            ));
        }
        let rebase = GitCommand::new(args).comment(comment).todo(todo).env(
            "GIT_SEQUENCE_EDITOR",
            format!("cp {}", shell_quote(&file.to_string_lossy())),
        );
        let mut commands = vec![rebase];
        let stops = emits.iter().any(|e| matches!(e, Emit::Pick { edit: true, .. }));
        if let Some(head) = plan.head.as_deref().filter(|h| *h != plan.top)
            && !stops
        {
            commands.push(
                GitCommand::new(["switch", head]).comment(format!("back to {head}, which moved with its commits")),
            );
        }
        Ok(commands)
    }

    /// Note the branch to switch back to when the rebase of `plan` stops before its end, so
    /// Continue, Skip or Abort can finish the job.
    pub(crate) fn remember_return(&self, plan: &StackPlan) {
        if let Some(head) = plan.head.as_deref().filter(|h| *h != plan.top)
            && let Ok(file) = self.git_file(RETURN_FILE)
        {
            let _ = std::fs::write(file, format!("{}\n{head}\n", plan.top));
        }
    }

    /// The branch to switch back to after the stack rebase in progress, if it is one.
    fn pending_return(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.git_file(RETURN_FILE).ok()?).ok()?;
        let mut lines = text.lines();
        let (top, head) = (lines.next()?, lines.next()?);
        let rebasing = std::fs::read_to_string(self.git_file("rebase-merge/head-name").ok()?).ok()?;
        (rebasing.trim() == format!("refs/heads/{top}")).then(|| head.to_owned())
    }

    /// The switch back that goes after Continue, Skip or Abort of a stack rebase that stopped.
    /// With `ending`, the step always ends the rebase (an abort); otherwise only when nothing
    /// left in the todo list stops it again.
    pub(crate) fn return_after(&self, ending: bool) -> Option<GitCommand> {
        let head = self.pending_return()?;
        if !ending {
            let todo = std::fs::read_to_string(self.git_file("rebase-merge/git-rebase-todo").ok()?).ok()?;
            let stops = todo.lines().any(|line| {
                let word = line.split_whitespace().next().unwrap_or_default();
                matches!(word, "edit" | "e" | "break" | "b")
            });
            if stops {
                return None;
            }
        }
        Some(GitCommand::new(["switch", &head]).comment(format!("back to {head}, the branch you were on")))
    }

    /// Forget the branch to switch back to once no rebase is going on.
    pub(crate) fn forget_return(&self) {
        if let Ok(file) = self.git_file(RETURN_FILE)
            && file.exists()
            && !self.git_file("rebase-merge").is_ok_and(|dir| dir.exists())
        {
            let _ = std::fs::remove_file(file);
        }
    }

    /// Write the todo list where the sequence editor of `command` copies it from.
    pub(crate) fn write_todo(&self, command: &GitCommand) -> Result<()> {
        let Some(todo) = &command.todo else { return Ok(()) };
        let file = self.git_file(TODO_FILE)?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|err| Error::Git(format!("could not write the todo list: {err}")))?;
        }
        std::fs::write(&file, todo).map_err(|err| Error::Git(format!("could not write the todo list: {err}")))
    }

    /// Fold the staged changes into `commit`, an older commit of the checked-out branch: a fixup
    /// commit, then a rebase that squashes it in.
    pub(crate) fn plan_add_to_commit(&self, commit: &str) -> Result<Vec<GitCommand>> {
        let head = self.head()?;
        let branch = head
            .branch
            .ok_or_else(|| Error::Git("check out a branch first: HEAD is not on one".into()))?;
        let sha = short(commit);
        let mut commit_args = vec!["commit".to_owned(), format!("--fixup={commit}")];
        if !crate::config::run_hooks() {
            commit_args.push("--no-verify".to_owned());
        }
        let mut rebase = vec![
            "rebase".to_owned(),
            "--interactive".to_owned(),
            "--autosquash".to_owned(),
        ];
        let mut comment = format!("--autosquash: the fixup goes right under {sha} and melts into it");
        if crate::config::git_at_least(2, 38) {
            rebase.push("--update-refs".to_owned());
            comment.push_str("; --update-refs: branches in between move along");
        }
        let tree = self.working_tree()?;
        if tree
            .unstaged
            .iter()
            .any(|f| f.status != crate::commit::FileStatus::Untracked)
        {
            rebase.push("--autostash".to_owned());
        }
        rebase.push(format!("{commit}~1"));
        Ok(vec![
            GitCommand::new(commit_args).comment(format!(
                "a commit named \"fixup! …\" with the staged changes, on {branch}"
            )),
            GitCommand::new(rebase)
                .comment(comment)
                .env("GIT_SEQUENCE_EDITOR", "true"),
        ])
    }

    /// The plan's commits must be the stack's commits as they are now, with a new commit for
    /// each split.
    fn check_plan(&self, plan: &StackPlan) -> Result<Range> {
        let range = self.list_range(&plan.base, &plan.top)?;
        let parts: Vec<String> = plan.splits.iter().map(|s| split_part(&s.id)).collect();
        let mut now: Vec<&str> = range
            .commits
            .iter()
            .map(|c| c.id.as_str())
            .chain(parts.iter().map(String::as_str))
            .collect();
        let mut planned: Vec<&str> = plan
            .steps
            .iter()
            .filter_map(|s| match s {
                StackStep::Commit { id, .. } => Some(id.as_str()),
                StackStep::Branch { .. } => None,
            })
            .collect();
        now.sort_unstable();
        planned.sort_unstable();
        let splits: HashSet<&str> = plan.splits.iter().map(|s| s.id.as_str()).collect();
        if now != planned || splits.len() != plan.splits.len() {
            return Err(Error::Git(format!(
                "{} changed since the plan was made: reload and try again",
                plan.top
            )));
        }
        Ok(range)
    }

    /// Make the commits of the plan's splits, and put them in place of the commits they split,
    /// in the plan's steps and in the range.
    fn resolve_splits(&self, plan: &StackPlan, mut range: Range) -> Result<Resolved> {
        let mut plan = plan.clone();
        let mut commit_of = HashMap::new();
        for split in &plan.splits {
            let at = range
                .commits
                .iter()
                .position(|c| c.id == split.id)
                .ok_or_else(|| Error::Git(format!("{} is not a commit of the stack", short(&split.id))))?;
            let made = self.split_commits(split)?;
            let original = range.commits[at].clone();
            let rest = Listed {
                id: made.rest.clone(),
                parent: original.parent.clone(),
                others: Vec::new(),
                tree: made.rest_tree,
                message: original.message.clone(),
            };
            let picked = Listed {
                id: made.picked.clone(),
                parent: made.rest.clone(),
                others: Vec::new(),
                tree: original.tree.clone(),
                message: split.message.trim_end().to_owned(),
            };
            range.commits.splice(at..=at, [rest, picked]);
            range.split.push(original);
            commit_of.insert(split.id.clone(), made.rest);
            commit_of.insert(split_part(&split.id), made.picked);
        }
        for step in &mut plan.steps {
            if let StackStep::Commit { id, .. } = step
                && let Some(commit) = commit_of.get(id.as_str())
            {
                *id = commit.clone();
            }
        }
        let step_of = commit_of
            .iter()
            .map(|(step, commit)| (commit.clone(), step.clone()))
            .collect();
        Ok(Resolved {
            plan,
            range,
            commit_of,
            step_of,
        })
    }

    /// The commits of `base..top`, oldest first.
    fn list_range(&self, base: &str, top: &str) -> Result<Range> {
        // Fields end in NUL: git prints a message only up to its first NUL, so a message can't
        // hold one, while any other byte, \x1e or \x1f too, may be in it.
        let out = self.run(&GitCommand::new([
            "log".to_owned(),
            "--reverse".to_owned(),
            "--format=%H%x00%P%x00%T%x00%B%x00".to_owned(),
            "--end-of-options".to_owned(),
            format!("{base}..{top}"),
            "--".to_owned(),
        ]))?;
        let fields: Vec<&str> = out.stdout.split('\0').collect();
        let (records, _) = fields.as_chunks::<4>();
        let commits = records
            .iter()
            .map(|[id, parents, tree, message]| {
                let id = id.trim_start_matches('\n');
                let mut parents = parents.split_whitespace().map(str::to_owned);
                let listed = Listed {
                    id: id.to_owned(),
                    parent: parents.next().unwrap_or_default(),
                    others: parents.collect(),
                    tree: (*tree).to_owned(),
                    message: message.trim_end().to_owned(),
                };
                let ids_only = crate::cli::is_oid(id)
                    && crate::cli::is_oid(tree)
                    && (listed.parent.is_empty() || crate::cli::is_oid(&listed.parent))
                    && listed.others.iter().all(|p| crate::cli::is_oid(p));
                if ids_only {
                    Ok(listed)
                } else {
                    Err(Error::Git(format!("could not read the commits of {base}..{top}")))
                }
            })
            .collect::<Result<Vec<_>>>()?;
        let (merges, commits) = commits.into_iter().partition(|c| !c.others.is_empty());
        Ok(Range {
            commits,
            merges,
            base_tree: self.tree_of(base)?,
            split: Vec::new(),
        })
    }

    /// Added and deleted lines per commit of `range`.
    fn range_stats(&self, range: &str) -> Result<HashMap<String, (u32, u32)>> {
        let out = self.run(&GitCommand::new([
            "log",
            "--format=%x1e%H",
            "--numstat",
            "--end-of-options",
            range,
            "--",
        ]))?;
        let mut stats = HashMap::new();
        for record in out.stdout.split('\u{1e}').filter(|r| !r.trim().is_empty()) {
            let mut lines = record.lines();
            let Some(id) = lines.next() else { continue };
            let (mut add, mut del) = (0u32, 0u32);
            for line in lines {
                let mut parts = line.split('\t');
                add += parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                del += parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
            }
            stats.insert(id.trim().to_owned(), (add, del));
        }
        Ok(stats)
    }

    /// `commit` replayed on `onto` in memory: the tree, or the files that conflict.
    fn dry_pick(&self, commit: &Listed, onto: &str) -> Result<std::result::Result<String, Vec<String>>> {
        let out = self.spawn(
            &GitCommand::new([
                "merge-tree",
                "--write-tree",
                "--name-only",
                "--no-messages",
                "-z",
                "--merge-base",
                &commit.parent,
                onto,
                &commit.id,
            ]),
            None,
        )?;
        let text = String::from_utf8_lossy(&out.stdout);
        let mut fields = text.split('\0');
        let tree = fields.next().unwrap_or_default().trim().to_owned();
        match out.status.code() {
            Some(0) => Ok(Ok(tree)),
            Some(1) => {
                let mut files: Vec<String> = fields.filter(|p| !p.is_empty()).map(str::to_owned).collect();
                files.dedup();
                Ok(Err(files))
            }
            _ => Err(Error::Git(format!(
                "the dry run could not replay {}: {}",
                short(&commit.id),
                String::from_utf8_lossy(&out.stderr).trim()
            ))),
        }
    }

    /// A throwaway commit of `tree` on `parent` for the dry run. Same input, same commit, so
    /// running the dry run again adds nothing; git cleans them up as unreachable.
    fn dry_commit(&self, tree: &str, parent: &str) -> Result<String> {
        let command = GitCommand::new(["commit-tree", tree, "-p", parent, "-m", "Oxbow dry run"])
            .env("GIT_AUTHOR_NAME", "Oxbow")
            .env("GIT_AUTHOR_EMAIL", "oxbow@localhost")
            .env("GIT_COMMITTER_NAME", "Oxbow")
            .env("GIT_COMMITTER_EMAIL", "oxbow@localhost")
            .env("GIT_AUTHOR_DATE", "1700000000 +0000")
            .env("GIT_COMMITTER_DATE", "1700000000 +0000");
        Ok(self.run(&command)?.stdout.trim().to_owned())
    }

    fn conflict_at(&self, commit: &Listed, files: Vec<String>) -> Result<StackConflict> {
        Ok(StackConflict {
            id: commit.id.clone(),
            summary: commit.message.lines().next().unwrap_or_default().to_owned(),
            files,
        })
    }

    fn tree_of(&self, rev: &str) -> Result<String> {
        Ok(self
            .run(&GitCommand::new(["rev-parse".to_owned(), format!("{rev}^{{tree}}")]))?
            .stdout
            .trim()
            .to_owned())
    }

    /// Where a stack goes once a pull request of it landed as `merged` (GitHub's merge commit):
    /// the first of `candidates` (`main`, `origin/main`) that has it, with its newest commit.
    /// `None` while none has it, e.g. before a fetch: the stack must not drop the merged commits
    /// onto a branch without them.
    pub fn landed_on(&self, merged: &str, candidates: &[String]) -> Option<Landing> {
        candidates
            .iter()
            .find(|name| self.is_ancestor(merged, name))
            .and_then(|name| {
                Some(Landing {
                    name: name.clone(),
                    tip: self.brief_of(name).ok()?,
                })
            })
    }

    /// The restack under way and how far it got. One whose rebase never ran or was aborted (the
    /// branches still have the merged commits) is forgotten, as is one that can't be read.
    pub fn restack(&self) -> Option<RestackState> {
        let file = self.git_file(RESTACK_FILE).ok()?;
        let text = std::fs::read_to_string(&file).ok()?;
        let Ok(restack) = serde_json::from_str::<Restack>(&text) else {
            let _ = std::fs::remove_file(&file);
            return None;
        };
        let rebasing = self.operation().ok().flatten().is_some_and(|op| {
            op.kind == crate::operation::OperationKind::Rebase
                && op.branch.as_ref().is_some_and(|b| restack.branches.contains(b))
        });
        let tip = |name: &str| self.rev_parse(name);
        let rebased = restack
            .branches
            .iter()
            .filter(|b| tip(&format!("refs/heads/{b}")).is_some())
            .all(|b| {
                restack
                    .drop
                    .iter()
                    .all(|id| !self.is_ancestor(id, &format!("refs/heads/{b}")))
            });
        if !rebasing && !rebased {
            let _ = std::fs::remove_file(&file);
            return None;
        }
        let unpushed = match &restack.remote {
            Some(remote) => restack
                .push
                .iter()
                .filter(|p| {
                    let here = tip(&format!("refs/heads/{}", p.branch));
                    here.is_some() && here != tip(&format!("refs/remotes/{remote}/{}", p.upstream))
                })
                .map(|p| p.branch.clone())
                .collect(),
            None => Vec::new(),
        };
        Some(RestackState {
            restack,
            rebasing,
            unpushed,
        })
    }

    /// Keep `restack` for its next step, or forget it with `None` once it is done.
    pub fn save_restack(&self, restack: Option<&Restack>) -> Result<()> {
        let file = self.git_file(RESTACK_FILE)?;
        let failed = |err: std::io::Error| Error::Git(format!("could not keep the restack: {err}"));
        let Some(restack) = restack else {
            return match std::fs::remove_file(&file) {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(failed(err)),
                _ => Ok(()),
            };
        };
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(failed)?;
        }
        // Written whole, then moved in, so a crash never leaves half a file.
        let next = file.with_extension("json.new");
        let text = serde_json::to_vec_pretty(restack).map_err(|err| failed(err.into()))?;
        std::fs::write(&next, text).map_err(failed)?;
        std::fs::rename(&next, &file).map_err(failed)
    }

    fn rev_parse(&self, rev: &str) -> Option<String> {
        let out = self
            .spawn(&GitCommand::new(["rev-parse", "--verify", "--quiet", rev]), None)
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    fn brief_of(&self, rev: &str) -> Result<CommitBrief> {
        let out = self.run(&GitCommand::new([
            "log",
            "-1",
            "--format=%H%x00%s%x00%an%x00%ct",
            "--end-of-options",
            rev,
            "--",
        ]))?;
        crate::remote::parse_briefs(&out.stdout)
            .into_iter()
            .next()
            .ok_or_else(|| Error::UnknownCommit(rev.to_owned()))
    }

    fn count(&self, range: &str) -> Result<usize> {
        Ok(self
            .run(&GitCommand::new(["rev-list", "--count", "--end-of-options", range]))?
            .stdout
            .trim()
            .parse()
            .unwrap_or(0))
    }

    /// Uncommitted changes to tracked files: what a rebase needs put aside. New files stay.
    pub(crate) fn tracked_changes(&self) -> Result<bool> {
        let tree = self.working_tree()?;
        Ok(!tree.staged.is_empty()
            || !tree.conflicted.is_empty()
            || tree
                .unstaged
                .iter()
                .any(|f| f.status != crate::commit::FileStatus::Untracked))
    }

    pub(crate) fn has_merges(&self, range: &str) -> Result<bool> {
        let out = self.run(&GitCommand::new([
            "rev-list",
            "--min-parents=2",
            "--max-count=1",
            "--end-of-options",
            range,
        ]))?;
        Ok(!out.stdout.trim().is_empty())
    }

    pub(crate) fn is_ancestor(&self, ancestor: &str, of: &str) -> bool {
        self.spawn(&GitCommand::new(["merge-base", "--is-ancestor", ancestor, of]), None)
            .is_ok_and(|out| out.status.success())
    }

    /// A file in the git directory, for this worktree.
    pub(crate) fn git_file(&self, name: &str) -> Result<std::path::PathBuf> {
        self.git_path(name)
    }
}

/// The plan as todo lines: squashes and fixups move right under the commit they fold into,
/// branch lines become `update-ref`, except for the top, which the rebase moves itself.
fn lay_out(plan: &StackPlan) -> Result<Vec<Emit>> {
    let mut out: Vec<Emit> = Vec::new();
    for step in &plan.steps {
        match step {
            StackStep::Branch { name } => {
                if *name != plan.top {
                    out.push(Emit::Ref { name: name.clone() });
                }
            }
            StackStep::Commit { id, action, .. } => match action {
                StepAction::Drop => out.push(Emit::Drop { id: id.clone() }),
                StepAction::Squash | StepAction::Fixup => {
                    let at = out
                        .iter()
                        .rposition(|e| matches!(e, Emit::Pick { .. } | Emit::Fold { .. }))
                        .ok_or_else(|| Error::Git("the oldest commit has nothing before it to squash into".into()))?;
                    out.insert(
                        at + 1,
                        Emit::Fold {
                            id: id.clone(),
                            squash: *action == StepAction::Squash,
                        },
                    );
                }
                StepAction::Pick | StepAction::Reword | StepAction::Edit => out.push(Emit::Pick {
                    id: id.clone(),
                    edit: *action == StepAction::Edit,
                }),
            },
        }
    }
    Ok(out)
}

/// Whether squashes or fixups follow the pick at `i`.
fn has_folds(emits: &[Emit], i: usize) -> bool {
    matches!(emits.get(i + 1), Some(Emit::Fold { .. }))
}

/// The new message of each pick (by its index in `emits`) whose message changes: a reword, or
/// the combined message of squashes into it. The parts of a split always get one, so the
/// rebase commits them again: they were made unsigned, by the split commit's committer.
fn group_messages(
    plan: &StackPlan,
    emits: &[Emit],
    by_id: &HashMap<&str, &Listed>,
    parts: &HashMap<String, String>,
) -> HashMap<usize, String> {
    let parts: HashSet<&str> = parts.values().map(String::as_str).collect();
    let asked: HashMap<&str, &str> = plan
        .steps
        .iter()
        .filter_map(|s| match s {
            StackStep::Commit {
                id,
                action,
                message: Some(m),
            } if !matches!(action, StepAction::Edit | StepAction::Drop) && !m.trim().is_empty() => {
                Some((id.as_str(), m.as_str()))
            }
            _ => None,
        })
        .collect();
    let mut out = HashMap::new();
    for (i, emit) in emits.iter().enumerate() {
        let Emit::Pick { id, edit: false } = emit else { continue };
        let original = by_id.get(id.as_str()).map_or("", |c| c.message.as_str());
        let squashed: Vec<&str> = emits[i + 1..]
            .iter()
            .map_while(|e| match e {
                Emit::Fold { id, squash } => Some((id, *squash)),
                _ => None,
            })
            .filter(|(_, squash)| *squash)
            .filter_map(|(id, _)| by_id.get(id.as_str()).map(|c| c.message.as_str()))
            .collect();
        let message = match asked.get(id.as_str()) {
            Some(m) if m.trim() != original.trim() || !squashed.is_empty() => Some(m.trim().to_owned()),
            _ if !squashed.is_empty() => Some(
                std::iter::once(original)
                    .chain(squashed)
                    .map(str::trim)
                    .collect::<Vec<_>>()
                    .join("\n\n"),
            ),
            Some(m) if parts.contains(id.as_str()) => Some(m.trim().to_owned()),
            _ if parts.contains(id.as_str()) => Some(original.trim().to_owned()),
            _ => None,
        };
        if let Some(message) = message {
            out.insert(i, message);
        }
    }
    out
}

/// The todo list git gets, one line per step.
fn todo_text(
    emits: &[Emit],
    messages: &HashMap<usize, String>,
    by_id: &HashMap<&str, &Listed>,
    emptied: &HashSet<String>,
) -> String {
    let subject = |id: &str| {
        by_id
            .get(id)
            .and_then(|c| c.message.lines().next())
            .unwrap_or_default()
            .to_owned()
    };
    let mut lines = Vec::new();
    let mut pending: Option<String> = None;
    for (i, emit) in emits.iter().enumerate() {
        // A new message goes on once the commit and everything folded into it are in.
        if !matches!(emit, Emit::Fold { .. })
            && let Some(message) = pending.take()
        {
            lines.push(reword_line(&message));
        }
        match emit {
            Emit::Pick { id, edit } => {
                let word = if emptied.contains(id) {
                    "drop"
                } else if *edit {
                    "edit"
                } else {
                    "pick"
                };
                lines.push(format!("{word} {} {}", short_todo(id), subject(id)));
                if word != "drop" {
                    pending = messages.get(&i).cloned();
                }
            }
            Emit::Fold { id, .. } => lines.push(format!("fixup {} {}", short_todo(id), subject(id))),
            Emit::Drop { id } => lines.push(format!("drop {} {}", short_todo(id), subject(id))),
            Emit::Ref { name } => lines.push(format!("update-ref refs/heads/{name}")),
        }
    }
    if let Some(message) = pending {
        lines.push(reword_line(&message));
    }
    lines.join("\n") + "\n"
}

/// An `exec` line that gives the commit just made `message`. A todo line is one line, so a
/// message with line breaks inside a paragraph is printed into the commit line by line.
fn reword_line(message: &str) -> String {
    const AMEND: &str = "git commit --amend --only --no-verify --allow-empty";
    let paragraphs = paragraphs(message);
    if paragraphs.iter().all(|p| !p.contains('\n')) {
        let flags: Vec<String> = paragraphs.iter().map(|p| format!("-m {}", shell_quote(p))).collect();
        return format!("exec {AMEND} {}", flags.join(" "));
    }
    let lines: Vec<String> = paragraphs
        .iter()
        .flat_map(|p| std::iter::once(String::new()).chain(p.split('\n').map(shell_quote)))
        .skip(1)
        .map(|l| if l.is_empty() { "''".to_owned() } else { l })
        .collect();
    format!("exec printf '%s\\n' {} | {AMEND} -F -", lines.join(" "))
}

/// How many commits are in a new place: out of their old order, or under another branch.
fn moved(range: &Range, plan: &StackPlan, emits: &[Emit], original: &HashMap<&str, &str>) -> usize {
    let old_index: HashMap<&str, usize> = range
        .commits
        .iter()
        .enumerate()
        .map(|(i, c)| (c.id.as_str(), i))
        .collect();
    // The picks in their new order, by their old position; squashes count as squashed.
    let picks: Vec<&str> = emits
        .iter()
        .filter_map(|e| match e {
            Emit::Pick { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    let order: Vec<usize> = picks.iter().filter_map(|id| old_index.get(id).copied()).collect();
    let keep = longest_increasing(&order);
    let mut out: HashSet<&str> = picks
        .iter()
        .enumerate()
        .filter(|(i, _)| !keep.contains(i))
        .map(|(_, id)| *id)
        .collect();

    // The branch each commit is under in the plan: the next branch line above it.
    let mut waiting: Vec<&str> = Vec::new();
    for step in &plan.steps {
        match step {
            StackStep::Commit { id, action, .. }
                if !matches!(action, StepAction::Squash | StepAction::Fixup | StepAction::Drop) =>
            {
                waiting.push(id.as_str())
            }
            StackStep::Commit { .. } => {}
            StackStep::Branch { name } => {
                for id in waiting.drain(..) {
                    if original.get(id).is_some_and(|b| b != name) {
                        out.insert(id);
                    }
                }
            }
        }
    }
    for id in waiting {
        if original.get(id).is_some_and(|b| *b != plan.top) {
            out.insert(id);
        }
    }
    out.len()
}

/// Positions in `values` of one longest increasing subsequence.
fn longest_increasing(values: &[usize]) -> HashSet<usize> {
    // Patience sorting: `tails[k]` is the position ending the best run of length k + 1.
    let mut tails: Vec<usize> = Vec::new();
    let mut before: Vec<Option<usize>> = vec![None; values.len()];
    for (i, &v) in values.iter().enumerate() {
        let k = tails.partition_point(|&t| values[t] < v);
        before[i] = k.checked_sub(1).map(|k| tails[k]);
        if k == tails.len() {
            tails.push(i);
        } else {
            tails[k] = i;
        }
    }
    let mut out = HashSet::new();
    let mut at = tails.last().copied();
    while let Some(i) = at {
        out.insert(i);
        at = before[i];
    }
    out
}

/// The tree of `id`, a commit of the range or its base.
fn range_tree(range: &Range, id: &str) -> String {
    range
        .commits
        .iter()
        .chain(&range.merges)
        .chain(&range.split)
        .find(|c| c.id == id)
        .map_or_else(|| range.base_tree.clone(), |c| c.tree.clone())
}

/// Whether `id` is a commit of the dry run rather than one of the stack.
fn is_dry(id: &str, range: &Range) -> bool {
    !range.commits.iter().any(|c| c.id == id)
}

fn short(id: &str) -> String {
    let full = id.len() == 40 && id.bytes().all(|b| b.is_ascii_hexdigit());
    if full { id[..7].to_owned() } else { id.to_owned() }
}

/// A sha in a todo line: long enough not to clash in a big repository.
fn short_todo(id: &str) -> &str {
    &id[..id.len().min(12)]
}
