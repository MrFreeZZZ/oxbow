mod support;

use oxbow_core::{
    Action, FileStatus, LineKind, Repo, SplitFile, SplitLines, Stack, StackPlan, StackSplit, StackStep, StepAction,
    split_part,
};
use support::Fixture;

/// `main`, then `feature/a` with one commit and `feature/b` on it with a commit that changes
/// two files and a commit above it. `feature/b` is checked out.
fn stacked() -> Fixture {
    let mut fx = Fixture::new();
    fx.write("app.rs", "fn main() {\n    run();\n}\n\nfn run() {}\n");
    fx.commit("readme.md", "hi\n", "Root");
    fx.git(&["switch", "-q", "-c", "feature/a"]);
    fx.commit("models.rs", "struct Session;\n", "Add Session model");
    fx.git(&["switch", "-q", "-c", "feature/b"]);
    fx.write(
        "app.rs",
        "fn main() {\n    setup();\n    run();\n}\n\nfn run() {}\n\nfn setup() {}\n",
    );
    fx.commit("api.rs", "fn sessions() {}\n", "Expose sessions and set up");
    fx.commit("docs.md", "how\n", "Write docs");
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

fn id_of(stack: &Stack, summary: &str) -> String {
    stack.commits.iter().find(|c| c.summary == summary).unwrap().id.clone()
}

/// `plan` with `split` made of the commit `id`: its new commit goes right after it.
fn with_split(mut plan: StackPlan, id: &str, files: Vec<SplitFile>, message: &str) -> StackPlan {
    let at = plan
        .steps
        .iter()
        .position(|s| matches!(s, StackStep::Commit { id: i, .. } if i == id))
        .unwrap();
    plan.steps.insert(
        at + 1,
        StackStep::Commit {
            id: split_part(id),
            action: StepAction::Pick,
            message: None,
        },
    );
    plan.splits.push(StackSplit {
        id: id.to_owned(),
        files,
        message: message.to_owned(),
    });
    plan
}

fn whole(path: &str) -> SplitFile {
    SplitFile {
        path: path.to_owned(),
        lines: None,
    }
}

fn files_of(fx: &mut Fixture, rev: &str) -> Vec<String> {
    fx.git(&["show", "--format=", "--name-only", rev])
        .lines()
        .map(str::to_owned)
        .collect()
}

fn subjects(fx: &mut Fixture, range: &str) -> Vec<String> {
    fx.git(&["log", "--format=%s", range])
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_picked_file_becomes_a_commit_right_after_its_commit() {
    let mut fx = stacked();
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    let id = id_of(&stack, "Expose sessions and set up");
    let tree_before = fx.git(&["rev-parse", "feature/b^{tree}"]);
    let a_before = fx.git(&["rev-parse", "feature/a"]);
    let plan = with_split(as_is(&stack), &id, vec![whole("app.rs")], "Set up before running");

    let preview = repo.stack_preview(&plan).unwrap();
    assert!(preview.changed);
    assert_eq!(preview.split, 1);
    assert!(preview.conflict.is_none());
    assert!(preview.emptied.is_empty());
    // The rest, the new commit and the one above them get new SHAs.
    assert_eq!(preview.rewritten, 3);
    assert_eq!(preview.moved, 0);
    let b = preview.branches.iter().find(|b| b.name == "feature/b").unwrap();
    assert!(b.moves);
    assert_eq!(b.commits, 3);
    let a = preview.branches.iter().find(|b| b.name == "feature/a").unwrap();
    assert!(!a.moves);

    let commands = repo.plan(&Action::EditStack { plan: plan.clone() }).unwrap().commands;
    assert!(
        commands[0]
            .comment
            .as_deref()
            .unwrap()
            .contains("is split first, with git commit-tree")
    );
    let todo = commands[0].todo.clone().unwrap();
    // Both parts are committed again by the rebase, with their messages.
    assert!(todo.contains("-m 'Expose sessions and set up'"), "{todo}");
    assert!(todo.contains("-m 'Set up before running'"), "{todo}");

    repo.perform(&Action::EditStack { plan }).unwrap();
    assert_eq!(
        subjects(&mut fx, "feature/a..feature/b"),
        ["Write docs", "Set up before running", "Expose sessions and set up"]
    );
    assert_eq!(files_of(&mut fx, "feature/b~2"), ["api.rs"]);
    assert_eq!(files_of(&mut fx, "feature/b~1"), ["app.rs"]);
    // Nothing else changed: the branch ends on the same files, feature/a stays.
    assert_eq!(fx.git(&["rev-parse", "feature/b^{tree}"]), tree_before);
    assert_eq!(fx.git(&["rev-parse", "feature/a"]), a_before);
    assert_eq!(fx.git(&["status", "--porcelain"]), "");
    // Both parts keep the commit's author and date; the committer is now.
    let author = fx.git(&["log", "-1", "--format=%an %ad", "--date=raw", &id]);
    assert_eq!(
        fx.git(&["log", "-1", "--format=%an %ad", "--date=raw", "feature/b~2"]),
        author
    );
    assert_eq!(
        fx.git(&["log", "-1", "--format=%an %ad", "--date=raw", "feature/b~1"]),
        author
    );
    let committed =
        |fx: &mut Fixture, rev: &str| -> i64 { fx.git(&["log", "-1", "--format=%ct", rev]).parse().unwrap() };
    assert!(committed(&mut fx, "feature/b~2") > committed(&mut fx, &id));
    // The Operation Log names the split, and Undo takes it back.
    let log = repo.operation_log().unwrap();
    assert_eq!(log[0].title, format!("Edit stack feature/b: split {}", &id[..7]));
    let tip = fx.git(&["rev-parse", "feature/b"]);
    repo.perform(&Action::Restore {
        id: log[0].id.clone(),
        after: false,
    })
    .unwrap();
    assert_ne!(fx.git(&["rev-parse", "feature/b"]), tip);
    assert_eq!(fx.git(&["rev-parse", "feature/b~1"]), id);
}

#[test]
fn picked_lines_leave_the_others_in_the_commit() {
    let mut fx = stacked();
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    let id = id_of(&stack, "Expose sessions and set up");

    // The sheet's diff of app.rs: two added lines, `setup();` and `fn setup() {}`.
    let files = repo.split_files(&id).unwrap();
    let paths: Vec<_> = files.iter().map(|f| (f.file.path.as_str(), f.file.status)).collect();
    assert_eq!(paths, [("api.rs", FileStatus::Added), ("app.rs", FileStatus::Modified)]);
    let app = &files[1];
    assert_eq!((app.file.additions, app.file.deletions), (3, 0));
    let added: Vec<_> = app
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .filter(|l| l.kind == LineKind::Added)
        .map(|l| (l.new_line.unwrap(), l.text.as_str()))
        .collect();
    assert_eq!(added, [(2, "    setup();"), (7, ""), (8, "fn setup() {}")]);

    // Only the call moves out; the function stays with the commit.
    let lines = SplitLines {
        removed: vec![],
        added: vec![2],
    };
    let plan = with_split(
        as_is(&stack),
        &id,
        vec![SplitFile {
            path: "app.rs".into(),
            lines: Some(lines),
        }],
        "Call setup",
    );
    repo.perform(&Action::EditStack { plan }).unwrap();
    assert_eq!(
        fx.git(&["show", "feature/b~2:app.rs"]),
        "fn main() {\n    run();\n}\n\nfn run() {}\n\nfn setup() {}"
    );
    assert_eq!(files_of(&mut fx, "feature/b~2"), ["api.rs", "app.rs"]);
    assert_eq!(
        fx.git(&["diff", "feature/b~2", "feature/b~1", "--", "app.rs"])
            .lines()
            .filter(|l| l.starts_with(['+', '-']) && !l.starts_with("+++") && !l.starts_with("---"))
            .collect::<Vec<_>>(),
        ["+    setup();"]
    );
}

#[test]
fn the_new_commit_can_go_into_the_branch_below() {
    let mut fx = stacked();
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    let id = id_of(&stack, "Expose sessions and set up");
    let mut plan = as_is(&stack);
    // The new commit goes on top of feature/a, under its branch line.
    let line = plan
        .steps
        .iter()
        .position(|s| matches!(s, StackStep::Branch { name } if name == "feature/a"))
        .unwrap();
    plan.steps.insert(
        line,
        StackStep::Commit {
            id: split_part(&id),
            action: StepAction::Pick,
            message: None,
        },
    );
    plan.splits.push(StackSplit {
        id: id.clone(),
        files: vec![whole("api.rs")],
        message: "Expose sessions".into(),
    });

    let preview = repo.stack_preview(&plan).unwrap();
    assert!(preview.conflict.is_none());
    assert_eq!(preview.moved, 1);
    let a = preview.branches.iter().find(|b| b.name == "feature/a").unwrap();
    assert!(a.moves);
    assert_eq!(a.commits, 2);
    // feature/a ends on the new commit, which the screen knows by its step.
    assert_eq!(a.tip_from, split_part(&id));

    repo.perform(&Action::EditStack { plan }).unwrap();
    assert_eq!(
        subjects(&mut fx, "main..feature/a"),
        ["Expose sessions", "Add Session model"]
    );
    assert_eq!(
        subjects(&mut fx, "feature/a..feature/b"),
        ["Write docs", "Expose sessions and set up"]
    );
    assert_eq!(files_of(&mut fx, "feature/a"), ["api.rs"]);
    assert_eq!(files_of(&mut fx, "feature/b~1"), ["app.rs"]);
    assert_eq!(fx.git(&["branch", "--show-current"]), "feature/b");
}

#[test]
fn a_split_that_moves_all_or_nothing_is_refused() {
    let fx = stacked();
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    let id = id_of(&stack, "Expose sessions and set up");
    let refused = |files: Vec<SplitFile>, message: &str| {
        let plan = with_split(as_is(&stack), &id, files, message);
        repo.stack_preview(&plan).unwrap_err().to_string()
    };
    assert!(refused(vec![whole("api.rs"), whole("app.rs")], "All").contains("leave some"));
    assert!(refused(vec![], "None").contains("pick the changes"));
    assert!(refused(vec![whole("api.rs")], "  ").contains("type a message"));
    assert!(refused(vec![whole("other.rs")], "Other").contains("not a file this commit changes"));
    let not_a_change = SplitFile {
        path: "app.rs".into(),
        lines: Some(SplitLines {
            removed: vec![1],
            added: vec![],
        }),
    };
    assert!(refused(vec![not_a_change], "Line").contains("not a change"));
    // Every changed line of a file is the whole file.
    let all_lines = SplitFile {
        path: "app.rs".into(),
        lines: Some(SplitLines {
            removed: vec![],
            added: vec![2, 7, 8],
        }),
    };
    assert!(refused(vec![whole("api.rs"), all_lines], "All").contains("leave some"));
}

#[test]
fn a_plan_whose_split_has_no_step_is_refused() {
    let fx = stacked();
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    let id = id_of(&stack, "Expose sessions and set up");
    let mut plan = as_is(&stack);
    plan.splits.push(StackSplit {
        id,
        files: vec![whole("api.rs")],
        message: "Expose".into(),
    });
    assert!(
        repo.stack_preview(&plan)
            .unwrap_err()
            .to_string()
            .contains("changed since")
    );
}

#[test]
fn binary_files_and_deleted_files_split_whole() {
    let mut fx = stacked();
    std::fs::write(fx.path().join("logo.png"), [0u8, 1, 2, 3, 0]).unwrap();
    std::fs::remove_file(fx.path().join("docs.md")).unwrap();
    fx.write("models.rs", "struct Session;\nstruct Token;\n");
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "-q", "-m", "Logo, tokens, no docs"]);
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    let id = id_of(&stack, "Logo, tokens, no docs");
    let files = repo.split_files(&id).unwrap();
    let logo = files.iter().find(|f| f.file.path == "logo.png").unwrap();
    assert!(logo.file.binary);
    assert!(logo.hunks.is_empty());
    let docs = files.iter().find(|f| f.file.path == "docs.md").unwrap();
    assert_eq!(docs.file.status, FileStatus::Deleted);

    let lines = SplitFile {
        path: "logo.png".into(),
        lines: Some(SplitLines {
            removed: vec![],
            added: vec![1],
        }),
    };
    let plan = with_split(as_is(&stack), &id, vec![lines], "Logo");
    assert!(
        repo.stack_preview(&plan)
            .unwrap_err()
            .to_string()
            .contains("only be split whole")
    );

    // The deletion and the logo move out; the tokens stay.
    let plan = with_split(
        as_is(&stack),
        &id,
        vec![whole("logo.png"), whole("docs.md")],
        "Logo, no docs",
    );
    repo.perform(&Action::EditStack { plan }).unwrap();
    assert_eq!(files_of(&mut fx, "feature/b~1"), ["models.rs"]);
    assert_eq!(files_of(&mut fx, "feature/b"), ["docs.md", "logo.png"]);
    assert_eq!(std::fs::read(fx.path().join("logo.png")).unwrap(), [0u8, 1, 2, 3, 0]);
}

#[test]
fn a_split_of_the_newest_commit_with_changes_on_disk_keeps_them() {
    let mut fx = stacked();
    fx.write("docs.md", "how, and why\n");
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    let id = id_of(&stack, "Expose sessions and set up");
    // Reword the rest as well: the message typed for it wins over the original.
    let mut plan = with_split(as_is(&stack), &id, vec![whole("api.rs")], "Expose sessions");
    for step in &mut plan.steps {
        if let StackStep::Commit { id: i, action, message } = step
            && *i == id
        {
            *action = StepAction::Reword;
            *message = Some("Set up before running".into());
        }
    }
    repo.perform(&Action::EditStack { plan }).unwrap();
    assert_eq!(
        subjects(&mut fx, "feature/a..feature/b"),
        ["Write docs", "Expose sessions", "Set up before running"]
    );
    assert_eq!(
        std::fs::read_to_string(fx.path().join("docs.md")).unwrap(),
        "how, and why\n"
    );
}
