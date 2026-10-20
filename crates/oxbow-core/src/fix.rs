//! Fix a Mistake: plans for the moments people search the web in a panic. A commit on the wrong
//! branch, a file left out of the last commit, a commit to take back with its changes kept, a
//! pushed commit to undo for everyone, work made without a branch.
//!
//! Each fix is a few steps of commands Oxbow already runs elsewhere (switch, branch, cherry-pick,
//! revert, reset, stash), chosen from how the repository looks: whether the commits are pushed,
//! and whether uncommitted files are in a step's way. The plan is made from the state the sheet
//! showed, so before anything runs that state is checked again: if HEAD, the branch, its upstream,
//! the remote branches or the changed files moved meanwhile, nothing runs and the sheet asks again
//! with a new plan.
//!
//! A step can stop on conflicts part way. The steps left are written down then, so Continue runs
//! them after the picked or reverted commit, and Abort goes back to the branch the fix started on
//! with the files that were put aside.

use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::cli::{GitCommand, literal};
use crate::config;
use crate::edit::{ResetMode, plan_reset};
use crate::error::{Error, Result};
use crate::operation::OperationKind;
use crate::reflog::ReflogEntry;
use crate::repo::Repo;
use crate::worktree::Action;

/// One way out of a typical mistake, with what the user picked in its sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Fix {
    /// Take the last `count` commits of the checked-out branch off it and put them on `to`: a new
    /// branch made for them with `create`, an existing one otherwise. Commits that are already on
    /// a remote are reverted on the branch instead of taken off it, and copied to `to`.
    MoveCommits { count: usize, to: String, create: bool },
    /// Stage `paths` and fold them into the last commit; its message stays.
    AddToLast {
        paths: Vec<String>,
        /// Go ahead even though the secrets check finds something, this one time.
        #[serde(default)]
        allow_secrets: bool,
    },
    /// Take the last commit off the branch; its changes stay staged (`Soft`) or in the files.
    UndoLast { mode: ResetMode },
    /// Add a commit that reverses `commit`, then, with `push`, push the branch to its upstream.
    RevertPushed { commit: String, push: bool },
    /// Keep work made without a branch on the new branch `name`: where `HEAD` is, switching to
    /// it, or at `commit`, a commit only the reflog still has, switching with `switch`.
    SaveDetached {
        name: String,
        commit: Option<String>,
        #[serde(default)]
        switch: bool,
    },
}

/// How the repository looked when a fix was planned. It must look the same when the fix runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixExpect {
    pub head: Option<String>,
    pub branch: Option<String>,
    /// Where the checked-out branch's upstream is; `None` without one.
    pub upstream: Option<String>,
    /// A fingerprint of every remote-tracking branch and where it is: what decides whether
    /// commits are pushed, and so whether they are moved or reverted.
    #[serde(default)]
    pub remotes: String,
    /// A fingerprint of the paths with uncommitted changes and whether each is staged, unstaged
    /// or conflicted: what decides whether a step needs them put aside.
    pub files: String,
}

/// A file with uncommitted changes, for picking what goes into the last commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixFile {
    pub path: String,
    /// `modified`, `new`, `deleted` or `renamed`.
    pub status: String,
    /// Some of its changes are staged.
    pub staged: bool,
    /// Some of its changes are not staged: picking it stages them.
    pub unstaged: bool,
}

/// What Fix a Mistake looks at before it offers anything.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixState {
    pub expect: FixExpect,
    /// Files with uncommitted changes; conflicted ones are left out.
    pub changed: Vec<FixFile>,
    /// Commits made without a branch that no branch, tag or remote branch has any more, newest
    /// first: only the reflog keeps them, until git's clean-up.
    pub lost: Vec<ReflogEntry>,
}

/// One step of a fix in words, and how many of the plan's commands are its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixStep {
    #[serde(flatten)]
    pub what: StepKind,
    pub commands: usize,
}

/// What a step of a fix does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum StepKind {
    /// Put the uncommitted files aside in the stash, since a later step changes some of them.
    Stash {
        files: usize,
    },
    /// A new branch: at `HEAD` without `at`, else at that commit.
    CreateBranch {
        name: String,
        at: Option<String>,
        switch: bool,
    },
    /// Point a branch that isn't checked out at another commit.
    MoveBranch {
        branch: String,
        to: String,
    },
    Switch {
        branch: String,
    },
    /// Copy commits onto the checked-out branch.
    Copy {
        count: usize,
    },
    /// Commits that reverse `count` commits, on `branch`.
    Revert {
        count: usize,
        branch: Option<String>,
    },
    Stage {
        paths: Vec<String>,
    },
    /// Replace the last commit with one that has the staged changes too.
    Amend,
    Reset {
        mode: ResetMode,
    },
    Push {
        remote: String,
        branch: String,
    },
    /// Bring the stashed files back.
    Pop,
}

/// A plan of a fix: its commands, and the steps they make up.
pub(crate) struct FixPlan {
    pub commands: Vec<GitCommand>,
    pub steps: Vec<FixStep>,
}

impl FixPlan {
    fn new() -> Self {
        FixPlan {
            commands: Vec::new(),
            steps: Vec::new(),
        }
    }

    fn step(&mut self, what: StepKind, commands: Vec<GitCommand>) {
        self.steps.push(FixStep {
            what,
            commands: commands.len(),
        });
        self.commands.extend(commands);
    }
}

/// How the stash looks before a fix puts files aside: the pop at its end runs only once the stash
/// moved, so a stash that saved nothing never brings back an older one.
struct Aside {
    files: usize,
    before: Option<String>,
}

impl Repo {
    /// The state Fix a Mistake plans from.
    pub fn fix_state(&self) -> Result<FixState> {
        let tree = self.working_tree()?;
        let mut changed: Vec<FixFile> = Vec::new();
        for (file, staged) in tree
            .staged
            .iter()
            .map(|f| (f, true))
            .chain(tree.unstaged.iter().map(|f| (f, false)))
        {
            if let Some(seen) = changed.iter_mut().find(|c| c.path == file.path) {
                seen.staged |= staged;
                seen.unstaged |= !staged;
                continue;
            }
            let status = match file.status {
                crate::commit::FileStatus::Added | crate::commit::FileStatus::Untracked => "new",
                crate::commit::FileStatus::Deleted => "deleted",
                crate::commit::FileStatus::Renamed => "renamed",
                _ => "modified",
            };
            changed.push(FixFile {
                path: file.path.clone(),
                status: status.to_owned(),
                staged,
                unstaged: !staged,
            });
        }
        changed.sort_by(|a, b| a.path.cmp(&b.path));
        let lost = self
            .reflog_entries()
            .unwrap_or_default()
            .into_iter()
            // Only HEAD's reflog has it: it was committed on no branch, or its branch is gone.
            // Amends and rebases leave old versions behind too; those are not work to save.
            .filter(|e| e.lost && e.within.is_none() && e.source == "HEAD" && e.kind == "commit")
            .take(5)
            .collect();
        Ok(FixState {
            expect: self.fix_expect_from(&tree)?,
            changed,
            lost,
        })
    }

    fn fix_expect_from(&self, tree: &crate::worktree::WorkingTree) -> Result<FixExpect> {
        let head = self.head()?;
        let upstream = head.branch.as_deref().and_then(|branch| self.upstream_tip(branch));
        let paths: BTreeSet<(u8, &str)> = [&tree.staged, &tree.unstaged, &tree.conflicted]
            .into_iter()
            .enumerate()
            .flat_map(|(side, files)| files.iter().map(move |f| (side as u8, f.path.as_str())))
            .collect();
        let remotes = self
            .run(&GitCommand::new([
                "for-each-ref",
                "--format=%(objectname) %(refname)",
                "refs/remotes",
            ]))?
            .stdout;
        Ok(FixExpect {
            head: head.commit,
            branch: head.branch,
            upstream,
            remotes: fingerprint(&remotes),
            files: fingerprint(&paths),
        })
    }

    /// Where `branch`'s upstream is, as of the last fetch.
    fn upstream_tip(&self, branch: &str) -> Option<String> {
        let out = self
            .run(&GitCommand::new([
                "rev-parse",
                "--verify",
                "-q",
                &format!("{branch}@{{upstream}}"),
            ]))
            .ok()?;
        Some(out.stdout.trim().to_owned()).filter(|s| !s.is_empty())
    }

    /// Refuse to run a fix planned for another state: something changed since the sheet showed it.
    pub(crate) fn check_fix(&self, expect: &FixExpect) -> Result<()> {
        let now = self.fix_expect_from(&self.working_tree()?)?;
        let what = if now.head != expect.head || now.branch != expect.branch {
            "HEAD moved"
        } else if now.upstream != expect.upstream {
            "the upstream branch moved"
        } else if now.remotes != expect.remotes {
            "a remote branch moved"
        } else if now.files != expect.files {
            "the changed files are different"
        } else {
            return Ok(());
        };
        Err(Error::Changed(format!(
            "{what} since the plan was made, so nothing ran. Look at the new plan."
        )))
    }

    pub(crate) fn plan_fix(&self, fix: &Fix) -> Result<FixPlan> {
        match fix {
            Fix::MoveCommits { count, to, create } => self.plan_move(*count, to, *create),
            Fix::AddToLast { paths, .. } => self.plan_add_to_last(paths),
            Fix::UndoLast { mode } => {
                let head = self
                    .head()?
                    .commit
                    .ok_or_else(|| Error::Git("there is no commit yet".into()))?;
                if self.parent_ids(&head)?.is_empty() {
                    return Err(Error::Git(
                        "this is the first commit of the repository: there is nothing before it to go back to".into(),
                    ));
                }
                let mut plan = FixPlan::new();
                plan.step(StepKind::Reset { mode: *mode }, plan_reset("HEAD~1", *mode));
                Ok(plan)
            }
            Fix::RevertPushed { commit, push } => self.plan_revert_pushed(commit, *push),
            Fix::SaveDetached { name, commit, switch } => {
                let mut plan = FixPlan::new();
                match commit {
                    None => {
                        if self.head()?.branch.is_some() {
                            return Err(Error::Git("HEAD is on a branch already".into()));
                        }
                        plan.step(
                            StepKind::CreateBranch {
                                name: name.clone(),
                                at: None,
                                switch: true,
                            },
                            vec![
                                GitCommand::new(["switch", "-c", name])
                                    .comment("-c: a new branch where HEAD is, and switch to it; your files stay"),
                            ],
                        );
                    }
                    Some(commit) => {
                        plan.step(
                            StepKind::CreateBranch {
                                name: name.clone(),
                                at: Some(commit.clone()),
                                switch: false,
                            },
                            vec![
                                GitCommand::new(["branch".to_owned(), name.clone(), commit.clone()])
                                    .comment("a branch keeps the commit and everything before it"),
                            ],
                        );
                        if *switch {
                            plan.step(
                                StepKind::Switch { branch: name.clone() },
                                vec![GitCommand::new(["switch", name])],
                            );
                        }
                    }
                }
                Ok(plan)
            }
        }
    }

    fn plan_move(&self, count: usize, to: &str, create: bool) -> Result<FixPlan> {
        let head = self.head()?;
        let branch = head
            .branch
            .ok_or_else(|| Error::Git("HEAD is not on a branch, so there is nothing to move off one".into()))?;
        let tip = head.commit.ok_or_else(|| Error::Git("there is no commit yet".into()))?;
        if count == 0 {
            return Err(Error::Git("pick at least one commit".into()));
        }
        if to == branch {
            return Err(Error::Git(format!("the commits are on {branch} already")));
        }
        let base = self
            .rev(&format!("{tip}~{count}"))
            .ok_or_else(|| Error::Git(format!("{branch} has fewer than {} commits", count + 1)))?;
        let oldest = self
            .rev(&format!("{tip}~{}", count - 1))
            .ok_or_else(|| Error::Git("no such commit".into()))?;
        // The newest of them on a remote means all of them are there; the oldest, some of them.
        let pushed = self.on_a_remote(&oldest)?;
        let merges = self
            .run(&GitCommand::new([
                "rev-list",
                "--count",
                "--min-parents=2",
                &format!("{base}..{tip}"),
            ]))?
            .stdout
            .trim()
            .parse::<usize>()
            .unwrap_or(0);
        if (pushed || !create) && merges > 0 {
            return Err(Error::Git(
                "a merge commit is among them, and merges can't be copied one by one: use Edit Stack instead".into(),
            ));
        }
        // Whole shas in commands: seven characters can match more than one object.
        let range = if count == 1 {
            tip.clone()
        } else {
            format!("{base}..{tip}")
        };
        let them = if count == 1 { "it" } else { "them" };

        let mut plan = FixPlan::new();
        let create_step = |plan: &mut FixPlan| {
            plan.step(
                StepKind::CreateBranch {
                    name: to.to_owned(),
                    at: None,
                    switch: true,
                },
                vec![GitCommand::new(["switch", "-c", to]).comment("-c: create the branch here and switch to it")],
            );
        };
        let copy_step = |plan: &mut FixPlan| {
            plan.step(
                StepKind::Copy { count },
                vec![GitCommand::new(["cherry-pick", range.as_str()]).comment(if count == 1 {
                    "the same changes and message, as a new commit".to_owned()
                } else {
                    format!(
                        "{}..{}: the commits after the first one up to the second, as new commits",
                        short(&base),
                        short(&tip)
                    )
                })],
            );
        };

        if !pushed && create {
            // Only refs move: the new branch is where the old one is now, so no file changes.
            create_step(&mut plan);
            plan.step(
                StepKind::MoveBranch {
                    branch: branch.clone(),
                    to: base.clone(),
                },
                vec![
                    GitCommand::new(["branch".to_owned(), "-f".to_owned(), branch.clone(), base.clone()]).comment(
                        format!("-f: point an existing branch somewhere else; {branch} isn’t checked out now"),
                    ),
                ],
            );
            return Ok(plan);
        }

        let mut touched = self.changed_between(&base, &tip)?;
        if !create {
            touched.extend(self.changed_between(&tip, to)?);
        }
        let aside = self.aside_for(&touched)?;
        if let Some(aside) = &aside {
            self.stash_step(&mut plan, aside, &format!("Before moving commits to {to}"));
        }
        if pushed {
            plan.step(
                StepKind::Revert {
                    count,
                    branch: Some(branch.clone()),
                },
                vec![
                    GitCommand::new(["revert", "--no-edit", range.as_str()]).comment(format!(
                        "commits that reverse {them}; the originals stay in {branch}’s history, as on the remote"
                    )),
                ],
            );
        }
        if create {
            create_step(&mut plan);
        } else {
            plan.step(
                StepKind::Switch { branch: to.to_owned() },
                vec![GitCommand::new(["switch", to])],
            );
        }
        copy_step(&mut plan);
        if !pushed {
            plan.step(
                StepKind::MoveBranch {
                    branch: branch.clone(),
                    to: base.clone(),
                },
                vec![
                    GitCommand::new(["branch".to_owned(), "-f".to_owned(), branch.clone(), base.clone()]).comment(
                        format!("-f: point an existing branch somewhere else; {branch} isn’t checked out now"),
                    ),
                ],
            );
        }
        if let Some(aside) = &aside {
            self.pop_step(&mut plan, aside);
        }
        Ok(plan)
    }

    fn plan_add_to_last(&self, paths: &[String]) -> Result<FixPlan> {
        if self.head()?.commit.is_none() {
            return Err(Error::Git("there is no commit yet".into()));
        }
        let mut plan = FixPlan::new();
        let mut args: Vec<String> = ["commit", "--amend", "--no-edit"].map(str::to_owned).into();
        let mut comment = "--no-edit: keep the message as it is".to_owned();
        if !config::run_hooks() {
            args.push("--no-verify".to_owned());
            comment.push_str("; --no-verify: Git hooks are turned off in Settings");
        }
        if !paths.is_empty() {
            let add = GitCommand::new(
                ["add".to_owned(), "--".to_owned()]
                    .into_iter()
                    .chain(paths.iter().map(|p| literal(p))),
            )
            .comment("stage them, new files included");
            plan.step(StepKind::Stage { paths: paths.to_vec() }, vec![add]);
            // Only the picked files: other staged changes stay staged, out of the commit.
            args.push("--only".to_owned());
            args.push("--".to_owned());
            args.extend(self.amend_paths(paths)?.iter().map(|p| literal(p)));
            comment.push_str("; --only: just these files, other staged changes stay staged");
        }
        let mut amend = GitCommand::new(args).comment(comment);
        amend.secrets_check = true;
        plan.step(StepKind::Amend, vec![amend]);
        Ok(plan)
    }

    /// The picked files of Add to the Last Commit, with the old path of a staged rename: the
    /// commit takes both halves of it.
    pub(crate) fn amend_paths(&self, paths: &[String]) -> Result<Vec<String>> {
        let tree = self.working_tree()?;
        let mut out = paths.to_vec();
        for file in &tree.staged {
            if let Some(old) = file.old_path.as_ref().filter(|_| paths.contains(&file.path))
                && !out.contains(old)
            {
                out.push(old.clone());
            }
        }
        Ok(out)
    }

    fn plan_revert_pushed(&self, commit: &str, push: bool) -> Result<FixPlan> {
        let head = self.head()?;
        let mut plan = FixPlan::new();
        let parent = self.parent_ids(commit)?.into_iter().next();
        let touched = match &parent {
            Some(parent) => self.changed_between(parent, commit)?,
            None => self.all_paths(commit)?,
        };
        let aside = self.aside_for(&touched)?;
        if let Some(aside) = &aside {
            self.stash_step(&mut plan, aside, &format!("Before reverting {}", short(commit)));
        }
        plan.step(
            StepKind::Revert {
                count: 1,
                branch: head.branch.clone(),
            },
            self.plan_revert(commit)?,
        );
        if push {
            let branch = head
                .branch
                .ok_or_else(|| Error::Git("HEAD is not on a branch, so there is nothing to push".into()))?;
            let tracking = self
                .tracking()?
                .filter(|t| !t.gone)
                .ok_or_else(|| Error::Git(format!("{branch} has no upstream to push to")))?;
            let push = Action::Push {
                remote: tracking.remote.clone(),
                branch: branch.clone(),
                upstream: tracking.branch.clone(),
                set_upstream: false,
                force: false,
                lease: None,
                no_verify: false,
                submodules: false,
            };
            plan.step(
                StepKind::Push {
                    remote: tracking.remote,
                    branch,
                },
                self.plan(&push)?.commands,
            );
        }
        if let Some(aside) = &aside {
            self.pop_step(&mut plan, aside);
        }
        Ok(plan)
    }

    /// Whether uncommitted files are in the way of a cherry-pick or revert that changes
    /// `touched`: then they go in the stash first. Untracked files count, since a branch may have
    /// a file of that name. Any staged change counts, wherever it is: git picks and reverts only
    /// with nothing staged.
    fn aside_for(&self, touched: &BTreeSet<String>) -> Result<Option<Aside>> {
        let tree = self.working_tree()?;
        let dirty: BTreeSet<&str> = tree
            .staged
            .iter()
            .chain(&tree.unstaged)
            .flat_map(|f| std::iter::once(f.path.as_str()).chain(f.old_path.as_deref()))
            .collect();
        if tree.staged.is_empty() && !dirty.iter().any(|p| touched.contains(*p)) {
            return Ok(None);
        }
        Ok(Some(Aside {
            files: tree.file_count(),
            before: self.rev("refs/stash"),
        }))
    }

    fn stash_step(&self, plan: &mut FixPlan, aside: &Aside, message: &str) {
        plan.step(
            StepKind::Stash { files: aside.files },
            vec![
                GitCommand::new(["stash", "push", "--include-untracked", "-m", message])
                    .comment("a later step needs your uncommitted files out of the way; they wait in the stash"),
            ],
        );
    }

    fn pop_step(&self, plan: &mut FixPlan, aside: &Aside) {
        let mut pop = GitCommand::new(["stash", "pop", "--index"])
            .comment("--index: your uncommitted files come back, the staged ones staged too");
        pop.only_if_moved = Some(("refs/stash".to_owned(), aside.before.clone()));
        plan.step(StepKind::Pop, vec![pop]);
    }

    /// Paths that differ between two commits or branches, both sides of a rename.
    fn changed_between(&self, from: &str, to: &str) -> Result<BTreeSet<String>> {
        let out = self.run(&GitCommand::new([
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            from,
            to,
            "--",
        ]))?;
        Ok(out
            .stdout
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect())
    }

    /// Every path of a commit's tree, for a root commit, which changes them all.
    fn all_paths(&self, commit: &str) -> Result<BTreeSet<String>> {
        let out = self.run(&GitCommand::new(["ls-tree", "-r", "-z", "--name-only", commit]))?;
        Ok(out
            .stdout
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect())
    }

    fn parent_ids(&self, commit: &str) -> Result<Vec<String>> {
        let out = self.run(&GitCommand::new(["rev-list", "--parents", "-n", "1", commit, "--"]))?;
        Ok(out.stdout.split_whitespace().skip(1).map(str::to_owned).collect())
    }

    /// Whether some remote-tracking branch has `commit`.
    fn on_a_remote(&self, commit: &str) -> Result<bool> {
        let out = self.run(&GitCommand::new([
            "for-each-ref",
            "--format=%(refname)",
            "--contains",
            commit,
            "refs/remotes",
        ]))?;
        Ok(out
            .stdout
            .lines()
            .any(|l| !l.trim().is_empty() && !l.ends_with("/HEAD")))
    }

    /// The full sha `rev` names, or `None`.
    pub(crate) fn rev(&self, rev: &str) -> Option<String> {
        let out = self
            .run(&GitCommand::new([
                "rev-parse",
                "--verify",
                "-q",
                &format!("{rev}^{{commit}}"),
            ]))
            .ok()?;
        Some(out.stdout.trim().to_owned()).filter(|s| !s.is_empty())
    }
}

/// A short hash of `value`, to compare one state with another.
fn fingerprint(value: impl Hash) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// A full sha reads better short, in comments and messages; commands take it whole.
fn short(commit: &str) -> String {
    commit[..commit.len().min(7)].to_owned()
}

/// Where the steps left of a fix that stopped on conflicts are written down.
const REST_FILE: &str = "oxbow/fix-rest.json";

/// The steps left of a fix whose cherry-pick or revert stopped on conflicts.
#[derive(Debug, Serialize, Deserialize)]
struct FixRest {
    /// `cherry-pick` or `revert`: the operation the steps wait for.
    kind: String,
    /// The branch the operation runs on.
    branch: Option<String>,
    /// The branch the fix started on, where Abort goes back to.
    start: Option<String>,
    /// The commands after the one that stopped.
    rest: Vec<SavedCommand>,
    /// The pop of the files the fix put aside, for Abort.
    pop: Option<SavedCommand>,
}

/// A command of a plan as it is written down, to run later.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedCommand {
    args: Vec<String>,
    comment: Option<String>,
    #[serde(default)]
    progress: bool,
    #[serde(default)]
    env: Vec<(String, String)>,
    #[serde(default)]
    only_if_moved: Option<(String, Option<String>)>,
}

impl From<&GitCommand> for SavedCommand {
    fn from(c: &GitCommand) -> Self {
        SavedCommand {
            args: c.args.clone(),
            comment: c.comment.clone(),
            progress: c.progress,
            env: c.env.clone(),
            only_if_moved: c.only_if_moved.clone(),
        }
    }
}

impl From<&SavedCommand> for GitCommand {
    fn from(c: &SavedCommand) -> Self {
        let mut command = GitCommand::new(c.args.clone());
        command.comment = c.comment.clone();
        command.progress = c.progress;
        command.env = c.env.clone();
        command.only_if_moved = c.only_if_moved.clone();
        command
    }
}

fn kind_word(kind: OperationKind) -> Option<&'static str> {
    match kind {
        OperationKind::CherryPick => Some("cherry-pick"),
        OperationKind::Revert => Some("revert"),
        _ => None,
    }
}

fn is_pop(command: &GitCommand) -> bool {
    command.args.len() >= 2 && command.args[0] == "stash" && command.args[1] == "pop"
}

impl Repo {
    /// After a command of a fix, or of a Continue of one, failed: when a cherry-pick or revert
    /// waits on conflicts now, note the commands after it, for Continue and Abort. `start` is the
    /// branch the fix started on, `commands` the plan that ran.
    pub(crate) fn note_fix_rest(&self, action: &Action, start: Option<String>, commands: &[GitCommand], failed: usize) {
        let Some(kind) = self.operation().ok().flatten().and_then(|op| kind_word(op.kind)) else {
            return;
        };
        let (start, pop) = match action {
            Action::Fix { .. } => (
                start,
                // The files were put aside when the first step was a stash, which ran by now.
                commands
                    .iter()
                    .find(|c| is_pop(c))
                    .filter(|_| {
                        commands
                            .first()
                            .is_some_and(|c| c.args.first().is_some_and(|a| a == "stash"))
                    })
                    .map(SavedCommand::from),
            ),
            // A Continue that stopped again keeps what the fix noted.
            _ => match self.read_fix_rest() {
                Some(rest) => (rest.start, rest.pop),
                None => return,
            },
        };
        let rest = FixRest {
            kind: kind.to_owned(),
            branch: self.head().ok().and_then(|h| h.branch),
            start,
            rest: commands[failed + 1..].iter().map(SavedCommand::from).collect(),
            pop,
        };
        if let Ok(file) = self.git_file(REST_FILE)
            && let Ok(json) = serde_json::to_string(&rest)
        {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(file, json);
        }
    }

    fn read_fix_rest(&self) -> Option<FixRest> {
        serde_json::from_str(&std::fs::read_to_string(self.git_file(REST_FILE).ok()?).ok()?).ok()
    }

    /// The steps of a fix left for after the operation in progress, if it is a fix's.
    fn fix_rest(&self, kind: OperationKind) -> Option<FixRest> {
        let rest = self.read_fix_rest()?;
        (Some(rest.kind.as_str()) == kind_word(kind) && rest.branch == self.head().ok()?.branch).then_some(rest)
    }

    /// Whether Continue of the operation in progress runs steps of a fix after it.
    pub(crate) fn has_fix_rest(&self) -> bool {
        self.read_fix_rest().is_some()
    }

    /// What goes after Continue of a cherry-pick or revert of a fix: the fix's steps left.
    pub(crate) fn fix_rest_after_continue(&self, kind: OperationKind) -> Vec<GitCommand> {
        self.fix_rest(kind)
            .map(|rest| rest.rest.iter().map(GitCommand::from).collect())
            .unwrap_or_default()
    }

    /// What goes after Skip of a cherry-pick of a fix: only the files put aside come back. The
    /// skipped commit is not copied, so the branch the commits came from keeps all of them.
    pub(crate) fn fix_rest_after_skip(&self, kind: OperationKind) -> Vec<GitCommand> {
        self.fix_rest(kind)
            .map(|rest| rest.rest.iter().map(GitCommand::from).filter(is_pop).collect())
            .unwrap_or_default()
    }

    /// What goes after Abort of a cherry-pick or revert of a fix: back to the branch the fix
    /// started on, and the files it put aside back. Commits steps before made stay.
    pub(crate) fn fix_rest_after_abort(&self, kind: OperationKind) -> Vec<GitCommand> {
        let Some(rest) = self.fix_rest(kind) else {
            return Vec::new();
        };
        let mut commands = Vec::new();
        if let Some(start) = rest.start.filter(|s| Some(s) != rest.branch.as_ref()) {
            commands
                .push(GitCommand::new(["switch", &start]).comment(format!("back to {start}, where the fix started")));
        }
        commands.extend(rest.pop.as_ref().map(GitCommand::from));
        commands
    }

    /// Forget the steps of a fix once no cherry-pick or revert waits for them.
    pub(crate) fn forget_fix_rest(&self) {
        if self.has_fix_rest()
            && !self
                .operation()
                .ok()
                .flatten()
                .is_some_and(|op| kind_word(op.kind).is_some())
            && let Ok(file) = self.git_file(REST_FILE)
        {
            let _ = std::fs::remove_file(file);
        }
    }
}
