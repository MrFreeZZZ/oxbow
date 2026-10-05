mod support;

use oxbow_core::graph::{TRUNK_COLOR, color_for_name};
use oxbow_core::{HistoryOptions, RefKind, Repo};
use support::Fixture;

#[test]
fn opens_from_a_subfolder_and_rejects_plain_folders() {
    let mut fx = Fixture::new();
    fx.commit("src/lib.rs", "fn a() {}\n", "Initial commit");
    let repo = Repo::open(fx.path().join("src")).expect("opens from a subfolder");
    assert_eq!(repo.name(), "repo");

    let plain = tempfile::tempdir().unwrap();
    assert!(Repo::open(plain.path()).is_err());
}

#[test]
fn empty_repository_has_no_rows() {
    let fx = Fixture::new();
    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions::default())
        .unwrap();
    assert!(history.rows.is_empty());
    assert_eq!(history.head.branch.as_deref(), Some("main"));
    assert_eq!(history.head.commit, None);
}

#[test]
fn trunk_is_column_zero_and_branches_go_right() {
    let mut fx = Fixture::new();
    let root = fx.commit("a.txt", "1\n", "Root");
    fx.git(&["checkout", "-q", "-b", "feature/login"]);
    fx.commit("b.txt", "1\n", "Add login form");
    fx.commit("b.txt", "2\n", "Validate login form");
    fx.git(&["checkout", "-q", "main"]);
    fx.commit("a.txt", "2\n", "Bump version");
    fx.git(&["tag", "-a", "v1.0.0", "-m", "Release"]);

    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions::default())
        .unwrap();
    let summaries: Vec<_> = history.rows.iter().map(|r| r.summary.as_str()).collect();
    assert_eq!(
        summaries,
        ["Bump version", "Validate login form", "Add login form", "Root"]
    );
    assert_eq!(history.trunk.as_deref(), Some("main"));
    assert_eq!(history.trunk_tip_row, Some(0));

    let main_tip = &history.rows[0];
    assert_eq!((main_tip.graph.column, main_tip.graph.color), (0, TRUNK_COLOR));
    let labels: Vec<_> = main_tip
        .labels
        .iter()
        .map(|l| (l.name.as_str(), l.kind, l.head))
        .collect();
    assert_eq!(
        labels,
        [("main", RefKind::Local, true), ("v1.0.0", RefKind::Tag, false)]
    );

    let feature = &history.rows[1];
    assert_eq!(feature.graph.column, 1);
    assert_eq!(feature.graph.color, color_for_name("feature/login"));
    assert_eq!(feature.labels[0].name, "feature/login");

    // The feature line ends at the root, which gets a fork ring in the feature's color.
    let root_row = &history.rows[3];
    assert_eq!(root_row.id, root);
    assert_eq!(root_row.graph.column, 0);
    assert_eq!(root_row.graph.fork_colors, [color_for_name("feature/login")]);
    // Nothing has a remote, so nothing counts as unpushed.
    assert!(history.rows.iter().all(|r| !r.unpushed));
}

#[test]
fn merge_commits_carry_the_merged_branch_color() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "1\n", "Root");
    fx.git(&["checkout", "-q", "-b", "topic"]);
    fx.commit("t.txt", "1\n", "Topic work");
    fx.git(&["checkout", "-q", "main"]);
    fx.commit("a.txt", "2\n", "Main work");
    fx.git(&["merge", "-q", "--no-edit", "topic"]);
    fx.git(&["branch", "-q", "-D", "topic"]);

    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions::default())
        .unwrap();
    let merge = &history.rows[0];
    assert_eq!(merge.summary, "Merge branch 'topic'");
    assert_eq!(merge.parents.len(), 2);
    assert_eq!(merge.graph.merge_colors, [color_for_name("topic")]);
    let topic = history.rows.iter().find(|r| r.summary == "Topic work").unwrap();
    assert_eq!(topic.graph.color, color_for_name("topic"));
    assert_ne!(topic.graph.column, 0);
}

#[test]
fn commits_missing_from_the_remote_are_unpushed() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "1\n", "Pushed");
    let remote = tempfile::tempdir().unwrap();
    let remote_path = remote.path().to_str().unwrap().to_owned();
    fx.git(&["init", "-q", "--bare", &remote_path]);
    fx.git(&["remote", "add", "origin", &remote_path]);
    fx.git(&["push", "-q", "-u", "origin", "main"]);
    fx.commit("a.txt", "2\n", "Local only");

    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions::default())
        .unwrap();
    assert_eq!(history.remotes, ["origin"]);
    let flags: Vec<_> = history.rows.iter().map(|r| (r.summary.as_str(), r.unpushed)).collect();
    assert_eq!(flags, [("Local only", true), ("Pushed", false)]);
    assert!(history.rows[0].graph.segments[0].dashed);
    let pushed_labels: Vec<_> = history.rows[1].labels.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(pushed_labels, ["origin/main"]);
}

#[test]
fn history_limit_truncates() {
    let mut fx = Fixture::new();
    for i in 0..5 {
        fx.commit("a.txt", &format!("{i}\n"), &format!("Commit {i}"));
    }
    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions { limit: 3 })
        .unwrap();
    assert_eq!(history.rows.len(), 3);
    assert!(history.truncated);
    // The oldest loaded commit's line keeps going down past the limit.
    assert_eq!(history.rows[2].graph.segments.len(), 1);
}
