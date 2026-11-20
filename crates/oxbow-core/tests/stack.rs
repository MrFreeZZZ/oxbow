mod support;

use oxbow_core::{
    Action, BranchPush, Error, OperationKind, Repo, Restack, RestackFailure, RestackStep, Stack, StackPlan, StackStep,
    StepAction,
};
use support::Fixture;

/// `main`, and three branches stacked on it: `auth/1` with one commit, `auth/2` with two on top
/// of it, `auth/3` with two more. `auth/2` is checked out.
fn stacked() -> Fixture {
    let mut fx = Fixture::new();
    fx.commit("readme.md", "hi\n", "Root");
    fx.git(&["switch", "-q", "-c", "auth/1"]);
    fx.commit("models.rs", "struct Session;\n", "Add Session model");
    fx.git(&["switch", "-q", "-c", "auth/2"]);
    fx.commit("api.rs", "fn sessions() {}\n", "Expose sessions endpoint");
    fx.commit("refresh.rs", "fn rotate() {}\n", "Validate refresh rotation");
    fx.git(&["switch", "-q", "-c", "auth/3"]);
    fx.commit("ui.rs", "banner\n", "Add expiry banner");
    fx.commit("client.rs", "refresh\n", "Wire token refresh");
    fx.git(&["switch", "-q", "auth/2"]);
    fx
}

/// The plan that keeps `stack` as it is, oldest first.
fn as_is(stack: &Stack) -> StackPlan {
    let mut steps = Vec::new();
    let mut branches = stack.branches.iter().rev().peekable();
    for commit in stack.commits.iter().rev() {
        steps.push(StackStep::Commit {
            id: commit.id.clone(),
            action: StepAction::Pick,
            message: None,
        });
        while let Some(b) = branches.next_if(|b| b.at == commit.id) {
            steps.push(StackStep::Branch { name: b.name.clone() });
        }
    }
    StackPlan {
        top: stack.top.clone(),
        base: stack.base.id.clone(),
        onto: stack.base.id.clone(),
        onto_name: None,
        head: stack.head.clone(),
        steps,
        splits: Vec::new(),
    }
}

fn subjects(fx: &mut Fixture, range: &str) -> Vec<String> {
    fx.git(&["log", "--format=%s", range])
        .lines()
        .map(str::to_owned)
        .collect()
}

fn set(plan: &mut StackPlan, id: &str, action: StepAction, message: Option<&str>) {
    for step in &mut plan.steps {
        if let StackStep::Commit {
            id: i,
            action: a,
            message: m,
        } = step
            && i == id
        {
            *a = action;
            *m = message.map(str::to_owned);
        }
    }
}

fn id_of(stack: &Stack, summary: &str) -> String {
    stack.commits.iter().find(|c| c.summary == summary).unwrap().id.clone()
}

#[test]
fn the_stack_reaches_from_the_checked_out_branch_up_to_the_last_one_built_on_it() {
    let fx = stacked();
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    assert_eq!(stack.top, "auth/3");
    assert_eq!(stack.head.as_deref(), Some("auth/2"));
    assert_eq!(stack.trunk, "main");
    assert_eq!(stack.base.summary, "Root");
    assert_eq!(stack.newer, 0);
    let names: Vec<_> = stack.branches.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["auth/3", "auth/2", "auth/1"]);
    let rows: Vec<_> = stack
        .commits
        .iter()
        .map(|c| (c.summary.as_str(), c.branch.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("Wire token refresh", "auth/3"),
            ("Add expiry banner", "auth/3"),
            ("Validate refresh rotation", "auth/2"),
            ("Expose sessions endpoint", "auth/2"),
            ("Add Session model", "auth/1"),
        ]
    );
    assert!(stack.left_behind.is_empty());

    // Nothing changes: no rebase needed.
    let preview = repo.stack_preview(&as_is(&stack)).unwrap();
    assert!(!preview.changed);
    assert_eq!(preview.rewritten, 0);
}

#[test]
fn editing_a_stack_ignores_tags_that_share_branch_names() {
    let mut fx = stacked();
    fx.git(&["tag", "auth/3", "main"]);
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    assert_eq!(stack.top, "auth/3");
    assert!(stack.left_behind.is_empty());

    fx.git(&["tag", "auth/2", "main"]);
    fx.git(&["tag", "main", "refs/heads/auth/3"]);
    let stack = repo.stack(None).unwrap();
    assert_eq!(stack.top, "auth/3");
    assert_eq!(stack.base.summary, "Root");
    assert_eq!(stack.commits.len(), 5);
    assert!(stack.left_behind.is_empty());
    let mut plan = as_is(&stack);
    set(
        &mut plan,
        &id_of(&stack, "Wire token refresh"),
        StepAction::Reword,
        Some("Rename the top commit"),
    );
    repo.perform(&Action::EditStack { plan }).unwrap();
    assert_eq!(repo.head().unwrap().branch.as_deref(), Some("auth/2"));
    assert_eq!(
        subjects(&mut fx, "refs/heads/auth/2..refs/heads/auth/3")
            .first()
            .map(String::as_str),
        Some("Rename the top commit")
    );
    assert_eq!(
        fx.git(&["rev-parse", "refs/tags/auth/3"]),
        fx.git(&["rev-parse", "refs/heads/main"])
    );
}

#[cfg(unix)]
#[test]
fn cancelling_a_stack_preview_stops_its_running_git_command() {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    let mut fx = stacked();
    let repo = Repo::open(fx.path()).unwrap();
    let plan = as_is(&repo.stack(None).unwrap());
    let hook = fx.path().join(".git/preview-monitor");
    fx.write(
        ".git/preview-monitor",
        "#!/bin/sh\ntouch .git/preview-started\nexec sleep 15\n",
    );
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    fx.git(&["config", "core.fsmonitor", hook.to_str().unwrap()]);

    let cancel = Arc::new(AtomicBool::new(false));
    let stop = cancel.clone();
    let preview = std::thread::spawn(move || oxbow_core::cli::stoppable(&stop, || repo.stack_preview(&plan)));
    let started = fx.path().join(".git/preview-started");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !started.exists() && !preview.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let running = started.exists();
    let now = Instant::now();
    cancel.store(true, Ordering::Relaxed);
    let result = preview.join().unwrap();
    assert!(running, "Git did not reach the blocking fsmonitor hook: {result:?}");
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
    assert!(now.elapsed() < Duration::from_secs(3), "Git ran after Stop");
}

#[test]
fn a_tag_on_a_protected_branch_does_not_put_it_inside_edit_stack() {
    let mut fx = stacked();
    let remote = tempfile::tempdir().unwrap();
    let url = remote.path().to_str().unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q", "--bare", url])
            .status()
            .unwrap()
            .success()
    );
    fx.git(&["remote", "add", "origin", url]);
    fx.git(&["switch", "-q", "-c", "release/1", "main"]);
    let release = fx.commit("release.txt", "release\n", "Release");
    fx.git(&["push", "-q", "-u", "origin", "release/1"]);
    fx.git(&["config", "oxbow.protectedBranches", "main release/*"]);
    fx.git(&["switch", "-q", "auth/2"]);
    fx.git(&["tag", "auth/3", &release]);

    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    assert_eq!(stack.top, "auth/3");
    let mut plan = as_is(&stack);
    set(
        &mut plan,
        &id_of(&stack, "Wire token refresh"),
        StepAction::Reword,
        Some("Refresh tokens"),
    );
    repo.perform(&Action::EditStack { plan }).unwrap();
    assert_eq!(
        fx.git(&["log", "-1", "--format=%s", "refs/heads/auth/3"]),
        "Refresh tokens"
    );
    assert_eq!(fx.git(&["rev-parse", "refs/heads/release/1"]), release);
    assert_eq!(fx.git(&["rev-parse", "refs/tags/auth/3"]), release);
}

#[test]
fn a_fork_on_top_of_the_stack_is_left_behind() {
    let mut fx = stacked();
    fx.git(&["switch", "-q", "-c", "auth/3b", "auth/2"]);
    fx.commit("other.rs", "x\n", "Another idea");
    fx.git(&["switch", "-q", "auth/2"]);
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    // Two ways up from auth/2: the stack ends there, both stay where they are.
    assert_eq!(stack.top, "auth/2");
    assert_eq!(stack.left_behind, ["auth/3", "auth/3b"]);
}

#[test]
fn the_main_branch_has_no_stack() {
    let mut fx = stacked();
    fx.git(&["switch", "-q", "main"]);
    let repo = Repo::open(fx.path()).unwrap();
    let err = repo.stack(None).unwrap_err().to_string();
    assert!(err.contains("main branch"), "{err}");
}

#[test]
fn reword_and_reorder_keep_every_branch_on_its_commits() {
    let mut fx = stacked();
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    let old_1 = fx.git(&["rev-parse", "auth/1"]);
    let mut plan = as_is(&stack);
    // Swap the two commits of auth/3 and reword the lower one of auth/2.
    let banner = id_of(&stack, "Add expiry banner");
    let wire = id_of(&stack, "Wire token refresh");
    let n = plan.steps.len();
    let (a, b) = (
        plan.steps
            .iter()
            .position(|s| matches!(s, StackStep::Commit { id, .. } if *id == banner))
            .unwrap(),
        plan.steps
            .iter()
            .position(|s| matches!(s, StackStep::Commit { id, .. } if *id == wire))
            .unwrap(),
    );
    plan.steps.swap(a, b);
    assert_eq!(n, plan.steps.len());
    let expose = id_of(&stack, "Expose sessions endpoint");
    set(
        &mut plan,
        &expose,
        StepAction::Reword,
        Some("Expose /v2/sessions endpoint\n\nWith paging."),
    );

    let preview = repo.stack_preview(&plan).unwrap();
    assert!(preview.changed);
    assert_eq!(preview.reworded, 1);
    assert_eq!(preview.moved, 1);
    assert_eq!(preview.rewritten, 4);
    assert!(preview.conflict.is_none());

    let commands = repo.plan(&Action::EditStack { plan: plan.clone() }).unwrap().commands;
    assert_eq!(
        commands[0].display(),
        format!("git rebase --interactive {} auth/3", stack.base.id)
    );
    let todo = commands[0].todo.clone().unwrap();
    assert!(todo.contains("update-ref refs/heads/auth/1\n"), "{todo}");
    assert!(todo.contains("exec git commit --amend --only --no-verify --allow-empty -m 'Expose /v2/sessions endpoint' -m 'With paging.'"), "{todo}");
    assert_eq!(commands[1].display(), "git switch auth/2");

    repo.perform(&Action::EditStack { plan }).unwrap();
    assert_eq!(fx.git(&["rev-parse", "auth/1"]), old_1, "auth/1 did not need to change");
    assert_eq!(
        subjects(&mut fx, "main..auth/3"),
        [
            "Add expiry banner",
            "Wire token refresh",
            "Validate refresh rotation",
            "Expose /v2/sessions endpoint",
            "Add Session model",
        ]
    );
    assert_eq!(
        subjects(&mut fx, "auth/1..auth/2"),
        ["Validate refresh rotation", "Expose /v2/sessions endpoint"]
    );
    assert_eq!(fx.git(&["log", "-1", "--format=%b", "auth/2~1"]), "With paging.");
    assert_eq!(fx.git(&["branch", "--show-current"]), "auth/2");
    assert!(repo.operation().unwrap().is_none());
}

#[test]
fn moving_a_commit_across_a_branch_line_moves_it_into_that_branch() {
    let mut fx = stacked();
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    let mut plan = as_is(&stack);
    // "Expose sessions endpoint" goes below the auth/1 line.
    let expose = id_of(&stack, "Expose sessions endpoint");
    let from = plan
        .steps
        .iter()
        .position(|s| matches!(s, StackStep::Commit { id, .. } if *id == expose))
        .unwrap();
    let step = plan.steps.remove(from);
    let line = plan
        .steps
        .iter()
        .position(|s| matches!(s, StackStep::Branch { name } if name == "auth/1"))
        .unwrap();
    plan.steps.insert(line, step);

    let preview = repo.stack_preview(&plan).unwrap();
    assert_eq!(preview.moved, 1);
    let auth1 = preview.branches.iter().find(|b| b.name == "auth/1").unwrap();
    assert_eq!(auth1.commits, 2);
    assert!(auth1.moves);

    repo.perform(&Action::EditStack { plan }).unwrap();
    assert_eq!(
        subjects(&mut fx, "main..auth/1"),
        ["Expose sessions endpoint", "Add Session model"]
    );
    assert_eq!(subjects(&mut fx, "auth/1..auth/2"), ["Validate refresh rotation"]);
}

#[test]
fn squash_fixup_and_drop() {
    let mut fx = stacked();
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    let mut plan = as_is(&stack);
    let validate = id_of(&stack, "Validate refresh rotation");
    let expose = id_of(&stack, "Expose sessions endpoint");
    let wire = id_of(&stack, "Wire token refresh");
    let banner = id_of(&stack, "Add expiry banner");
    set(&mut plan, &validate, StepAction::Squash, None);
    set(&mut plan, &wire, StepAction::Fixup, None);
    set(&mut plan, &banner, StepAction::Drop, None);
    let _ = expose;

    let preview = repo.stack_preview(&plan).unwrap();
    assert_eq!(preview.squashed, 2);
    assert_eq!(preview.dropped, 1);
    let commands = repo.plan(&Action::EditStack { plan: plan.clone() }).unwrap().commands;
    let todo = commands[0].todo.clone().unwrap();
    // The fixup has nothing left above it in auth/3, so it melts into auth/2's commit.
    assert!(todo.contains(&format!("fixup {}", &wire[..12])), "{todo}");

    repo.perform(&Action::EditStack { plan }).unwrap();
    assert_eq!(
        subjects(&mut fx, "main..auth/3"),
        ["Expose sessions endpoint", "Add Session model"]
    );
    assert_eq!(
        fx.git(&["log", "-1", "--format=%B", "auth/2"]),
        "Expose sessions endpoint\n\nValidate refresh rotation"
    );
    // auth/3 lost its own commits and now points where auth/2 does.
    assert_eq!(fx.git(&["rev-parse", "auth/3"]), fx.git(&["rev-parse", "auth/2"]));
    assert_eq!(fx.git(&["branch", "--show-current"]), "auth/2");
    assert!(fx.path().join("client.rs").exists(), "the fixup's file came along");
    assert!(!fx.path().join("ui.rs").exists(), "the dropped commit's file is gone");
}

#[test]
fn moving_onto_the_newest_main_finds_the_conflict_first() {
    let mut fx = stacked();
    fx.git(&["switch", "-q", "main"]);
    fx.commit("api.rs", "fn sessions_v2() {}\n", "Sessions on main");
    fx.git(&["switch", "-q", "auth/2"]);
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    assert_eq!(stack.newer, 1);
    let mut plan = as_is(&stack);
    plan.onto = stack.trunk_tip.id.clone();
    plan.onto_name = Some("main".into());

    let preview = repo.stack_preview(&plan).unwrap();
    let conflict = preview.conflict.expect("api.rs conflicts");
    assert_eq!(conflict.summary, "Expose sessions endpoint");
    assert_eq!(conflict.files, ["api.rs"]);
    // The commits after the conflict are replayed too once it is resolved.
    assert_eq!(preview.rewritten, stack.commits.len());

    let commands = repo.plan(&Action::EditStack { plan: plan.clone() }).unwrap().commands;
    assert_eq!(
        commands[0].display(),
        format!(
            "git rebase --interactive --onto {} {} auth/3",
            stack.trunk_tip.id, stack.base.id
        )
    );
    repo.perform(&Action::EditStack { plan }).unwrap_err();
    let op = repo.operation().unwrap().expect("the rebase stopped");
    assert_eq!(op.kind, OperationKind::Rebase);
    repo.perform(&Action::Abort).unwrap();
    assert!(repo.operation().unwrap().is_none());
    assert_eq!(
        fx.git(&["branch", "--show-current"]),
        "auth/2",
        "back on the branch it started from"
    );
}

#[test]
fn the_stack_lands_on_the_commit_it_was_shown_even_if_the_branch_moves() {
    let mut fx = stacked();
    fx.git(&["switch", "-q", "main"]);
    fx.commit("docs.md", "how\n", "Docs on main");
    fx.git(&["switch", "-q", "auth/2"]);
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    let mut plan = as_is(&stack);
    plan.onto = stack.trunk_tip.id.clone();
    plan.onto_name = Some("main".into());
    let commands = repo.plan(&Action::EditStack { plan: plan.clone() }).unwrap().commands;
    assert!(commands[0].comment.as_deref().unwrap().starts_with("--onto main ("));

    // Someone else moves main back between the preview and the run.
    fx.git(&["branch", "-f", "main", "main~1"]);
    repo.perform(&Action::EditStack { plan }).unwrap();
    assert_eq!(fx.git(&["rev-parse", "auth/1~1"]), stack.trunk_tip.id);
    assert!(fx.path().join("docs.md").exists());
}

#[test]
fn after_a_conflict_continue_goes_back_to_the_branch_it_started_from() {
    let mut fx = stacked();
    fx.git(&["switch", "-q", "main"]);
    fx.commit("api.rs", "fn sessions_v2() {}\n", "Sessions on main");
    fx.git(&["switch", "-q", "auth/2"]);
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    let mut plan = as_is(&stack);
    plan.onto = stack.trunk_tip.id.clone();
    plan.onto_name = Some("main".into());
    repo.perform(&Action::EditStack { plan }).unwrap_err();
    assert!(repo.operation().unwrap().is_some());

    fx.write("api.rs", "fn sessions_v2() {}\nfn sessions() {}\n");
    fx.git(&["add", "api.rs"]);
    let commands = repo.plan(&Action::Continue { message: None }).unwrap().commands;
    assert_eq!(commands.last().unwrap().display(), "git switch auth/2");
    repo.perform(&Action::Continue { message: None }).unwrap();
    assert!(repo.operation().unwrap().is_none());
    assert_eq!(fx.git(&["branch", "--show-current"]), "auth/2");
    assert_eq!(subjects(&mut fx, "main..auth/3").len(), 5);
    assert_eq!(subjects(&mut fx, "main..auth/2").len(), 3);
    assert!(
        !fx.path().join(".git/oxbow/edit-stack-return").exists(),
        "the note is gone"
    );
}

#[test]
fn a_commit_main_already_has_is_left_out() {
    let mut fx = stacked();
    fx.git(&["switch", "-q", "main"]);
    fx.commit("models.rs", "struct Session;\n", "Session model on main");
    fx.git(&["switch", "-q", "auth/2"]);
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    let mut plan = as_is(&stack);
    plan.onto = stack.trunk_tip.id.clone();
    let preview = repo.stack_preview(&plan).unwrap();
    assert_eq!(preview.emptied, [id_of(&stack, "Add Session model")]);
    assert!(preview.conflict.is_none());
    let auth1 = preview.branches.iter().find(|b| b.name == "auth/1").unwrap();
    assert_eq!(auth1.commits, 0);
    // Empty, it ends up on the commit the stack is replayed on.
    assert_eq!(auth1.tip_from, stack.trunk_tip.id);

    repo.perform(&Action::EditStack { plan }).unwrap();
    assert_eq!(fx.git(&["rev-parse", "auth/1"]), fx.git(&["rev-parse", "main"]));
    assert_eq!(subjects(&mut fx, "main..auth/3").len(), 4);
}

#[test]
fn an_edit_stops_the_rebase_on_that_commit() {
    let mut fx = stacked();
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    let mut plan = as_is(&stack);
    let expose = id_of(&stack, "Expose sessions endpoint");
    set(&mut plan, &expose, StepAction::Edit, None);
    let preview = repo.stack_preview(&plan).unwrap();
    assert!(preview.changed, "an edit is enough to run the plan");
    assert_eq!(preview.edits, 1);
    assert_eq!(preview.rewritten, 0);
    assert!(preview.branches.iter().all(|branch| branch.same_sha && !branch.moves));
    let commands = repo.plan(&Action::EditStack { plan: plan.clone() }).unwrap().commands;
    assert_eq!(commands.len(), 1, "no switch back while the rebase waits");
    repo.perform(&Action::EditStack { plan }).unwrap();
    assert_eq!(repo.operation().unwrap().unwrap().kind, OperationKind::Rebase);
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]), "Expose sessions endpoint");
    repo.perform(&Action::Continue { message: None }).unwrap();
    assert!(repo.operation().unwrap().is_none());
    assert_eq!(subjects(&mut fx, "main..auth/3").len(), 5);
    assert_eq!(fx.git(&["branch", "--show-current"]), "auth/2");
}

#[test]
fn folding_into_an_edit_honors_the_combined_message_and_manual_amend() {
    let original = "Expose sessions endpoint";
    let combined = "Expose sessions endpoint\n\nValidate refresh rotation";
    let typed = "Handle sessions\n\n- expose the endpoint\n- user's refresh";
    let manual = "Rework the sessions endpoint\n\nKeep this manual edit.";
    let manual_combined = "Rework the sessions endpoint\n\nKeep this manual edit.\n\nValidate refresh rotation";
    for (fold, message, amend, expected) in [
        (StepAction::Squash, None, None, combined),
        (StepAction::Squash, Some(combined), Some(manual), manual_combined),
        (StepAction::Squash, None, Some(manual), manual_combined),
        (StepAction::Squash, Some(typed), None, typed),
        (StepAction::Squash, Some(typed), Some(original), typed),
        (StepAction::Squash, Some(typed), Some(manual), manual),
        (StepAction::Fixup, None, None, original),
        (StepAction::Fixup, None, Some(manual), manual),
    ] {
        let mut fx = stacked();
        let repo = Repo::open(fx.path()).unwrap();
        let stack = repo.stack(None).unwrap();
        let mut plan = as_is(&stack);
        let target = id_of(&stack, original);
        let folded = id_of(&stack, "Validate refresh rotation");
        set(&mut plan, &target, StepAction::Edit, message);
        set(&mut plan, &folded, fold, None);
        set(&mut plan, &id_of(&stack, "Add expiry banner"), StepAction::Fixup, None);

        repo.perform(&Action::EditStack { plan }).unwrap();
        let op = repo.operation().unwrap().unwrap();
        assert_eq!(op.kind, OperationKind::Rebase);
        assert_eq!(op.commit.unwrap().id, target);
        assert_eq!(fx.git(&["log", "-1", "--format=%B"]), original);
        assert!(!fx.path().join("refresh.rs").exists());
        if let Some(message) = amend {
            fx.write("api.rs", "// amended\nfn sessions() {}\n");
            fx.git(&["add", "api.rs"]);
            fx.git(&["commit", "--amend", "-m", message]);
        }

        repo.perform(&Action::Continue { message: None }).unwrap();
        assert!(repo.operation().unwrap().is_none());
        assert_eq!(fx.git(&["branch", "--show-current"]), "auth/2");
        assert_eq!(subjects(&mut fx, "main..auth/2").len(), 2);
        assert_eq!(fx.git(&["log", "-1", "--format=%B", "auth/2"]), expected);
        assert_eq!(fx.git(&["show", "auth/2:refresh.rs"]), "fn rotate() {}");
        assert_eq!(fx.git(&["show", "auth/2:ui.rs"]), "banner");
        let api = fx.git(&["show", "auth/2:api.rs"]);
        assert_eq!(api.contains("// amended"), amend.is_some());
        assert!(api.ends_with("fn sessions() {}"));
    }
}

#[test]
fn a_squash_conflict_does_not_strip_the_message_amended_during_edit() {
    let manual = "New subject\n\n# Heading in the message";
    let squashed = "Squash this\n\n# Keep the user's heading\nKeep $(literal) and \\c";
    let combined = format!("{manual}\n\n{squashed}");
    for (message, expected) in [(None, combined.as_str()), (Some("Custom combination"), manual)] {
        let mut fx = Fixture::new();
        fx.commit("file.txt", "base\n", "Root");
        fx.commit("HEAD", "a file named HEAD\n", "Add a file named HEAD");
        fx.git(&["switch", "-c", "feature"]);
        let target = fx.commit("file.txt", "edit\n", "Edit target");
        let folded = fx.commit("file.txt", "folded\n", squashed);
        let repo = Repo::open(fx.path()).unwrap();
        let mut plan = as_is(&repo.stack(None).unwrap());
        set(&mut plan, &target, StepAction::Edit, message);
        set(&mut plan, &folded, StepAction::Squash, None);

        repo.perform(&Action::EditStack { plan }).unwrap();
        fx.write("file.txt", "amended\n");
        fx.git(&["commit", "--amend", "-a", "-m", manual]);
        let next = Action::Continue { message: None };
        assert!(repo.perform(&next).is_err());
        assert_eq!(repo.operation().unwrap().unwrap().conflicted, 1);

        fx.write("file.txt", "amended and folded\n");
        fx.git(&["add", "file.txt"]);
        repo.perform(&next).unwrap();
        assert!(repo.operation().unwrap().is_none());
        assert_eq!(fx.git(&["branch", "--show-current"]), "feature");
        assert_eq!(fx.git(&["log", "-1", "--format=%B"]), expected);
        assert_eq!(fx.git(&["show", "HEAD:file.txt"]), "amended and folded");
        assert!(fx.git(&["for-each-ref", "refs/rewritten/"]).is_empty());
    }
}

#[test]
fn a_message_with_line_breaks_inside_a_paragraph_keeps_them() {
    let mut fx = stacked();
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    let mut plan = as_is(&stack);
    let model = id_of(&stack, "Add Session model");
    let message = "Add Session model\n\n- id\n- user's token";
    set(&mut plan, &model, StepAction::Reword, Some(message));
    repo.perform(&Action::EditStack { plan }).unwrap();
    assert_eq!(fx.git(&["log", "-1", "--format=%B", "auth/1"]), message);
}

#[test]
fn staged_changes_go_into_an_older_commit() {
    let mut fx = stacked();
    fx.git(&["switch", "-q", "auth/3"]);
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    let expose = id_of(&stack, "Expose sessions endpoint");
    fx.write("api.rs", "fn sessions() { todo!() }\n");
    fx.git(&["add", "api.rs"]);
    fx.write("notes.txt", "keep me\n");

    repo.perform(&Action::AddToCommit {
        commit: expose,
        allow_secrets: false,
        skip_checks: false,
    })
    .unwrap();
    assert_eq!(subjects(&mut fx, "main..auth/3").len(), 5);
    assert_eq!(fx.git(&["show", "auth/2~1:api.rs"]), "fn sessions() { todo!() }");
    assert_eq!(subjects(&mut fx, "auth/1..auth/2").len(), 2, "auth/2 moved along");
    assert!(fx.path().join("notes.txt").exists());
}

#[test]
fn a_secret_in_the_staged_changes_stops_adding_them_to_an_older_commit() {
    let mut fx = stacked();
    fx.git(&["switch", "-q", "auth/3"]);
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    let expose = id_of(&stack, "Expose sessions endpoint");
    // A fake token, split so that secret scanners don't take this file for a leak.
    fx.write(
        "deploy.env",
        concat!("TOKEN=ghp_", "R8mZ2kQx7Lp4Wn9Vb3Tc6Yd1Hf5Gt0Js8Ka2\n"),
    );
    fx.git(&["add", "deploy.env"]);
    let top = fx.git(&["rev-parse", "HEAD"]);

    let add = |allow_secrets| Action::AddToCommit {
        commit: expose.clone(),
        allow_secrets,
        skip_checks: false,
    };
    let err = repo.perform(&add(false)).unwrap_err();
    assert!(matches!(err, Error::Secrets(ref found) if found.len() == 1), "{err}");
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), top, "nothing was committed");
    repo.perform(&add(true)).unwrap();
    assert_ne!(fx.git(&["rev-parse", "HEAD"]), top);
}

#[test]
fn a_stack_that_changed_since_the_plan_is_refused() {
    let mut fx = stacked();
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    let plan = as_is(&stack);
    fx.git(&["switch", "-q", "auth/3"]);
    fx.commit("late.rs", "x\n", "Late commit");
    let err = repo.stack_preview(&plan).unwrap_err().to_string();
    assert!(err.contains("changed since"), "{err}");
}

#[test]
fn a_pushed_branch_that_is_rewritten_needs_a_force_push() {
    let mut fx = stacked();
    let remote = tempfile::tempdir().unwrap();
    let url = remote.path().join("acme.git");
    fx.git(&["init", "-q", "--bare", url.to_str().unwrap()]);
    fx.git(&["remote", "add", "origin", url.to_str().unwrap()]);
    fx.git(&["push", "-q", "-u", "origin", "main", "auth/1", "auth/2"]);
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    assert!(
        stack
            .commits
            .iter()
            .find(|c| c.summary == "Add Session model")
            .unwrap()
            .pushed
    );
    assert!(
        !stack
            .commits
            .iter()
            .find(|c| c.summary == "Wire token refresh")
            .unwrap()
            .pushed
    );
    assert_eq!(
        stack.branches.iter().find(|b| b.name == "auth/3").unwrap().upstream,
        None
    );

    let mut plan = as_is(&stack);
    let expose = id_of(&stack, "Expose sessions endpoint");
    set(&mut plan, &expose, StepAction::Reword, Some("Expose /v2/sessions"));
    let preview = repo.stack_preview(&plan).unwrap();
    let force: Vec<_> = preview
        .branches
        .iter()
        .filter(|b| b.force_push.is_some())
        .map(|b| b.name.as_str())
        .collect();
    assert_eq!(force, ["auth/2"], "auth/1 keeps its commit, auth/3 was never pushed");
}

#[test]
fn preview_checks_the_upstream_mapped_by_the_fetch_refspec() {
    let mut fx = stacked();
    fx.git(&["remote", "add", "origin", "."]);
    fx.git(&["config", "remote.origin.fetch", "+refs/heads/*:refs/remotes/custom/*"]);
    fx.git(&["config", "branch.auth/2.remote", "origin"]);
    fx.git(&["config", "branch.auth/2.merge", "refs/heads/auth/2"]);
    fx.git(&["tag", "auth/2", "main"]);
    let upstream = "refs/remotes/custom/auth/2";
    fx.git(&["update-ref", upstream, "refs/heads/auth/2"]);
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    let mut plan = as_is(&stack);
    let expose = id_of(&stack, "Expose sessions endpoint");
    set(&mut plan, &expose, StepAction::Reword, Some("Expose /v2/sessions"));

    let force_push = || {
        repo.stack_preview(&plan)
            .unwrap()
            .branches
            .into_iter()
            .find(|b| b.name == "auth/2")
            .unwrap()
            .force_push
    };
    let push = force_push().unwrap();
    assert_eq!((push.remote.as_str(), push.branch.as_str()), ("origin", "auth/2"));

    fx.git(&["update-ref", upstream, "refs/heads/auth/1"]);
    assert!(force_push().is_none());
    fx.git(&["update-ref", "-d", upstream]);
    assert!(force_push().is_none());
}

#[test]
fn a_branch_forking_off_the_middle_of_the_stack_is_left_behind() {
    let mut fx = stacked();
    fx.git(&["switch", "-q", "-c", "spike", "auth/1"]);
    fx.commit("spike.rs", "x\n", "Spike");
    fx.git(&["switch", "-q", "auth/3"]);
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    assert_eq!(stack.top, "auth/3");
    assert_eq!(stack.left_behind, ["spike"]);
}

#[test]
fn each_branch_knows_the_commit_it_ends_up_on() {
    let fx = stacked();
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    let mut plan = as_is(&stack);
    // The newest commit of auth/3 goes; auth/2's commits stay as they are.
    let wire = id_of(&stack, "Wire token refresh");
    set(&mut plan, &wire, StepAction::Drop, None);
    let preview = repo.stack_preview(&plan).unwrap();
    let after = |name: &str| preview.branches.iter().find(|b| b.name == name).unwrap().clone();
    assert!(after("auth/3").moves);
    assert_eq!(after("auth/3").tip_from, id_of(&stack, "Add expiry banner"));
    assert!(after("auth/3").same_sha);
    assert!(!after("auth/2").moves);
    assert_eq!(after("auth/2").tip_from, id_of(&stack, "Validate refresh rotation"));
}

#[test]
fn a_merge_of_main_into_the_stack_is_left_out() {
    let mut fx = stacked();
    fx.git(&["switch", "-q", "main"]);
    fx.commit("docs.md", "rate limits\n", "Mention rate limits");
    fx.git(&["switch", "-q", "auth/2"]);
    fx.git(&["merge", "-q", "--no-edit", "main"]);
    fx.git(&["switch", "-q", "auth/3"]);
    fx.git(&["reset", "-q", "--hard", "auth/2"]);
    fx.commit("ui.rs", "banner\n", "Add expiry banner");
    fx.git(&["switch", "-q", "auth/2"]);
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    assert_eq!(stack.top, "auth/3");
    assert_eq!(stack.base.summary, "Mention rate limits");
    assert_eq!(stack.merges.len(), 1);
    assert_eq!(stack.merges[0].branch, "auth/2");
    let names: Vec<_> = stack.branches.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["auth/3", "auth/2", "auth/1"]);
    // auth/2's line sits on its own newest commit, under the merge.
    let auth2 = stack.branches.iter().find(|b| b.name == "auth/2").unwrap();
    assert_eq!(auth2.at, id_of(&stack, "Validate refresh rotation"));
    assert_eq!(stack.commits.len(), 4);

    // As it is, the plan replays the stack straight onto main, without the merge.
    let plan = as_is(&stack);
    let preview = repo.stack_preview(&plan).unwrap();
    assert!(preview.changed);
    assert!(preview.conflict.is_none());
    repo.perform(&Action::EditStack { plan }).unwrap();
    assert_eq!(fx.git(&["rev-list", "--min-parents=2", "--count", "main..auth/3"]), "0");
    assert_eq!(
        subjects(&mut fx, "main..auth/3"),
        [
            "Add expiry banner",
            "Validate refresh rotation",
            "Expose sessions endpoint",
            "Add Session model"
        ]
    );
    assert_eq!(fx.git(&["show", "auth/2:docs.md"]), "rate limits");
}

#[test]
fn a_merge_of_another_branch_is_explained() {
    let mut fx = stacked();
    fx.git(&["switch", "-q", "-c", "spike", "main"]);
    fx.commit("spike.rs", "x\n", "Try a spike");
    fx.git(&["switch", "-q", "auth/3"]);
    fx.git(&["merge", "-q", "--no-edit", "spike"]);
    let repo = Repo::open(fx.path()).unwrap();
    let err = repo.stack(Some("auth/3")).unwrap_err().to_string();
    assert!(err.contains("brings in commits main doesn't have"), "{err}");
    // Below the merge the stack still opens, ending where the merge starts.
    let stack = repo.stack(Some("auth/1")).unwrap();
    assert_eq!(stack.top, "auth/2");
}

#[test]
fn a_merge_of_main_with_changes_of_its_own_is_explained() {
    let mut fx = stacked();
    fx.git(&["switch", "-q", "main"]);
    fx.commit("docs.md", "rate limits\n", "Mention rate limits");
    fx.git(&["switch", "-q", "auth/3"]);
    fx.git(&["merge", "-q", "--no-ff", "--no-commit", "main"]);
    fx.write("api.rs", "fn sessions_v2() {}\n");
    fx.git(&["commit", "-q", "-a", "--no-edit"]);
    let repo = Repo::open(fx.path()).unwrap();
    let err = repo.stack(Some("auth/3")).unwrap_err().to_string();
    assert!(err.contains("changes files of its own"), "{err}");
    assert!(
        err.contains("still opens on auth/1 and auth/2, below the merge"),
        "{err}"
    );
}

#[test]
fn a_restack_is_kept_between_its_steps() {
    let mut fx = stacked();
    let remote = tempfile::tempdir().unwrap();
    let url = remote.path().join("acme.git");
    fx.git(&["init", "-q", "--bare", url.to_str().unwrap()]);
    fx.git(&["remote", "add", "origin", url.to_str().unwrap()]);
    fx.git(&["push", "-q", "-u", "origin", "main", "auth/1", "auth/2", "auth/3"]);
    // auth/1 merged on GitHub as a new commit on main.
    fx.git(&["switch", "-q", "main"]);
    fx.commit("models.rs", "struct Session;\n", "Add Session model (#418)");
    fx.git(&["switch", "-q", "auth/2"]);
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(Some("auth/3")).unwrap();
    let merged = id_of(&stack, "Add Session model");
    let restack = Restack {
        merged: 418,
        base: "main".into(),
        branches: vec!["auth/3".into(), "auth/2".into()],
        drop: vec![merged.clone()],
        remote: Some("origin".into()),
        push: ["auth/3", "auth/2"]
            .map(|b| BranchPush {
                branch: b.into(),
                upstream: b.into(),
            })
            .to_vec(),
        retarget: vec![419],
        failed: None,
    };

    // Kept before its rebase ran, it is forgotten: nothing moved.
    repo.save_restack(Some(&restack)).unwrap();
    assert!(repo.restack().is_none());
    assert!(!fx.path().join(".git/oxbow/restack.json").exists());

    let mut plan = as_is(&stack);
    set(&mut plan, &merged, StepAction::Drop, None);
    plan.steps
        .retain(|s| !matches!(s, StackStep::Branch { name } if name == "auth/1"));
    plan.onto = fx.git(&["rev-parse", "main"]);
    repo.perform(&Action::EditStack { plan }).unwrap();
    let failed = Restack {
        failed: Some(RestackFailure {
            step: RestackStep::Push,
            message: "Could not resolve host: github.com".into(),
        }),
        ..restack.clone()
    };
    repo.save_restack(Some(&failed)).unwrap();
    let state = repo.restack().unwrap();
    assert_eq!(state.restack, failed);
    assert!(!state.rebasing);
    assert_eq!(state.unpushed, ["auth/3", "auth/2"]);

    fx.git(&["push", "-q", "--force-with-lease", "origin", "auth/3", "auth/2"]);
    assert!(repo.restack().unwrap().unpushed.is_empty());
    repo.save_restack(None).unwrap();
    assert!(repo.restack().is_none());
    repo.save_restack(None).unwrap();
}

#[test]
fn a_restack_stopped_on_a_conflict_is_kept() {
    let mut fx = stacked();
    fx.git(&["switch", "-q", "main"]);
    fx.commit("models.rs", "struct Session;\n", "Add Session model (#418)");
    fx.commit("api.rs", "fn sessions_v2() {}\n", "Sessions on main");
    fx.git(&["switch", "-q", "auth/2"]);
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(Some("auth/3")).unwrap();
    let merged = id_of(&stack, "Add Session model");
    let mut plan = as_is(&stack);
    set(&mut plan, &merged, StepAction::Drop, None);
    plan.steps
        .retain(|s| !matches!(s, StackStep::Branch { name } if name == "auth/1"));
    plan.onto = fx.git(&["rev-parse", "main"]);
    plan.onto_name = Some("main".into());
    repo.perform(&Action::EditStack { plan }).unwrap_err();
    let restack = Restack {
        merged: 418,
        base: "main".into(),
        branches: vec!["auth/3".into(), "auth/2".into()],
        drop: vec![merged],
        remote: None,
        push: Vec::new(),
        retarget: Vec::new(),
        failed: None,
    };
    repo.save_restack(Some(&restack)).unwrap();
    assert!(repo.restack().unwrap().rebasing);

    // Aborted, the branches are where they were: nothing to go on with.
    repo.perform(&Action::Abort).unwrap();
    assert!(repo.restack().is_none());
}

#[test]
fn separators_in_a_message_never_become_a_git_option() {
    let mut fx = stacked();
    let out = fx.path().parent().unwrap().join("written-by-git");
    let message = format!("Innocent\n\n\u{1e}--output={}\u{1f}\u{1f}\u{1f}rest", out.display());
    fx.git(&["switch", "-q", "auth/3"]);
    fx.write("ui.rs", "banner 2\n");
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "-q", "-m", &message]);
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(Some("auth/1")).unwrap();
    assert!(!out.exists(), "git wrote {}", out.display());
    assert!(stack.commits.iter().all(|c| c.id.len() == 40));
    assert!(stack.commits.iter().any(|c| c.summary == "Innocent"));
}
