mod support;

use oxbow_core::{Action, BisectMark, HistoryOptions, Repo};
use support::Fixture;

/// Ten commits on `main`, `v1` tagged on the second; `retry.rs` breaks in the sixth.
fn ten_commits() -> (Fixture, Vec<String>) {
    let mut fx = Fixture::new();
    let mut ids = Vec::new();
    for i in 1..=10 {
        let state = if i >= 6 { "broken" } else { "fine" };
        ids.push(fx.commit("retry.rs", &format!("{state} {i}\n"), &format!("Commit {i}")));
        if i == 2 {
            fx.git(&["tag", "v1"]);
        }
    }
    (fx, ids)
}

fn mark(mark: BisectMark) -> Action {
    Action::BisectMark { mark, commit: None }
}

/// Answer the commit under test by looking at the file, as a person would after testing.
fn answer(repo: &Repo, fx: &Fixture) {
    let text = std::fs::read_to_string(fx.path().join("retry.rs")).unwrap();
    let good = text.starts_with("fine");
    repo.perform(&mark(if good { BisectMark::Good } else { BisectMark::Bad }))
        .unwrap();
}

#[test]
fn bisecting_finds_the_first_bad_commit_and_brings_the_stash_back() {
    let (mut fx, ids) = ten_commits();
    fx.write("notes.txt", "uncommitted\n");
    let repo = Repo::open(fx.path()).unwrap();

    let start = Action::BisectStart {
        good: "v1".into(),
        bad: "main".into(),
        stash: true,
        command: None,
    };
    let shown: Vec<String> = repo
        .plan(&start)
        .unwrap()
        .commands
        .iter()
        .map(|c| c.display())
        .collect();
    assert_eq!(
        shown,
        [
            "git stash push --include-untracked -m 'before bisect'",
            "git bisect start",
            "git bisect bad main",
            "git bisect good v1",
        ]
    );
    repo.perform(&start).unwrap();
    assert!(!fx.path().join("notes.txt").exists(), "the uncommitted file is stashed");

    let state = repo.bisect().unwrap().expect("a bisect is going on");
    assert_eq!(state.start, "main");
    assert!(state.start_is_branch);
    assert_eq!(state.first_good_label.as_deref(), Some("v1"));
    assert_eq!(state.first_bad_label.as_deref(), Some("main"));
    // Commits 3 to 10 may be the first bad one.
    assert_eq!(state.candidates.len(), 8);
    assert!(state.stashed);
    assert_eq!(state.steps, 0);
    assert!(state.found.is_none());
    assert_eq!(state.output[0], "$ git bisect good v1");
    assert!(state.output[1].starts_with("Bisecting: "), "{:?}", state.output);

    // The history carries it too, for the banner and the graph.
    let history = repo.history(&HistoryOptions::default()).unwrap();
    assert!(history.bisect.is_some());

    let mut rounds = 0;
    while repo.bisect().unwrap().unwrap().found.is_none() {
        answer(&repo, &fx);
        rounds += 1;
        assert!(rounds < 6, "bisect should narrow it down in a few steps");
    }
    let state = repo.bisect().unwrap().unwrap();
    assert_eq!(state.found.as_deref(), Some(ids[5].as_str()));
    assert_eq!(state.steps, rounds);
    assert!(
        state.output.iter().any(|l| l.ends_with("is the first bad commit")),
        "{:?}",
        state.output
    );
    // The journal has the two answers that set the range, then one row for each step.
    assert_eq!(state.journal.len(), rounds + 2);
    assert!(state.journal[0].range && state.journal[1].range);
    assert_eq!(state.journal[0].summary, "Commit 10");
    assert!(
        state.journal[2..]
            .iter()
            .all(|s| !s.range && s.summary.starts_with("Commit "))
    );
    let report = state.report.as_deref().expect("a report once found");
    assert!(
        report.starts_with(&format!("First bad commit: {}\n    Commit 6\n", ids[5])),
        "{report}"
    );
    assert!(
        report.contains(&format!("Between v1 (good) and main (bad), 8 commits: {rounds} step")),
        "{report}"
    );
    // Git 2.56 and newer put the term in quotes.
    assert!(state.log.contains("# first bad commit:") || state.log.contains("# first 'bad' commit:"));

    let finish = repo.plan(&Action::BisectReset).unwrap();
    let shown: Vec<String> = finish.commands.iter().map(|c| c.display()).collect();
    assert_eq!(shown, ["git bisect reset", "git stash pop stash@{0}"]);
    repo.perform(&Action::BisectReset).unwrap();
    assert!(repo.bisect().unwrap().is_none());
    assert_eq!(fx.git(&["branch", "--show-current"]), "main");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("notes.txt")).unwrap(),
        "uncommitted\n"
    );
    assert_eq!(fx.git(&["stash", "list"]), "");
}

#[test]
fn a_test_command_answers_every_step() {
    let (fx, ids) = ten_commits();
    let repo = Repo::open(fx.path()).unwrap();
    let start = Action::BisectStart {
        good: ids[1].clone(),
        bad: "main".into(),
        stash: false,
        command: Some("grep -q fine retry.rs".into()),
    };
    let plan = repo.plan(&start).unwrap();
    assert_eq!(
        plan.commands.last().unwrap().display(),
        "git bisect run sh -c 'grep -q fine retry.rs'"
    );
    repo.perform(&start).unwrap();
    let state = repo.bisect().unwrap().unwrap();
    assert_eq!(state.found.as_deref(), Some(ids[5].as_str()));
    assert!(!state.stashed);
    assert!(state.report.is_some_and(|r| r.contains("Commit 6")));
    assert!(state.journal.len() > 2);

    // Nothing was stashed, so finishing only goes back.
    let shown: Vec<String> = repo
        .plan(&Action::BisectReset)
        .unwrap()
        .commands
        .iter()
        .map(|c| c.display())
        .collect();
    assert_eq!(shown, ["git bisect reset"]);
}

#[test]
fn any_commit_can_be_answered_and_skipped_commits_are_listed() {
    let (fx, ids) = ten_commits();
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&Action::BisectStart {
        good: "v1".into(),
        bad: "main".into(),
        stash: false,
        command: None,
    })
    .unwrap();
    repo.perform(&mark(BisectMark::Skip)).unwrap();
    repo.perform(&Action::BisectMark {
        mark: BisectMark::Good,
        commit: Some(ids[3].clone()),
    })
    .unwrap();
    let state = repo.bisect().unwrap().unwrap();
    assert_eq!(state.skipped.len(), 1);
    assert!(state.good.contains(&ids[3]));
    assert_eq!(state.steps, 2);
    // Commits 5 to 10 are left.
    assert_eq!(state.candidates.len(), 6);

    repo.perform(&Action::BisectReset).unwrap();
    assert!(repo.bisect().unwrap().is_none());
    assert!(
        repo.operation_log().unwrap().is_empty(),
        "bisect steps stay out of the Operation Log"
    );
}
