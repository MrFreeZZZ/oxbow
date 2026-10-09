mod support;

use oxbow_core::{Action, OperationKind, Repo, Stack, StackPlan, StackStep, StepAction};
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
        while let Some(b) = branches.next_if(|b| b.tip == commit.id) {
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
        format!("git rebase --interactive {} auth/3", &stack.base.id[..7])
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
            stack.trunk_tip.id,
            &stack.base.id[..7]
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

    repo.perform(&Action::AddToCommit { commit: expose }).unwrap();
    assert_eq!(subjects(&mut fx, "main..auth/3").len(), 5);
    assert_eq!(fx.git(&["show", "auth/2~1:api.rs"]), "fn sessions() { todo!() }");
    assert_eq!(subjects(&mut fx, "auth/1..auth/2").len(), 2, "auth/2 moved along");
    assert!(fx.path().join("notes.txt").exists());
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
