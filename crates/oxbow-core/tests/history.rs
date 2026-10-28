mod support;

use oxbow_core::graph::{STASH_COLOR, TRUNK_COLOR, color_for_name};
use oxbow_core::{Error, History, HistoryOptions, HistoryRow, RefKind, Repo};
use support::Fixture;

#[test]
fn opens_from_a_subfolder_and_rejects_plain_folders() {
    let mut fx = Fixture::new();
    fx.commit("src/lib.rs", "fn a() {}\n", "Initial commit");
    let repo = Repo::open(fx.path().join("src")).expect("opens from a subfolder");
    assert_eq!(repo.name(), "repo");

    let plain = tempfile::tempdir().unwrap();
    assert!(matches!(Repo::open(plain.path()), Err(Error::NotARepository { .. })));
}

#[test]
fn a_sha256_repository_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let made = std::process::Command::new("git")
        .args(["init", "-q", "--object-format=sha256"])
        .current_dir(dir.path())
        .status()
        .unwrap();
    assert!(made.success());
    let err = Repo::open(dir.path()).err().unwrap();
    assert!(matches!(err, Error::Sha256 { .. }), "{err}");
    assert!(err.to_string().contains("SHA-256"));
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
    assert!(history.segments_at(0)[0].dashed);
    let pushed_labels: Vec<_> = history.rows[1].labels.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(pushed_labels, ["origin/main"]);
}

#[test]
fn unpushed_commits_of_branches_and_merges_are_the_ones_git_names() {
    use std::collections::HashSet;
    let mut fx = Fixture::new();
    for i in 0..60 {
        fx.commit("a.txt", &format!("{i}\n"), &format!("Main {i}"));
    }
    let remote = tempfile::tempdir().unwrap();
    let remote_path = remote.path().to_str().unwrap().to_owned();
    fx.git(&["init", "-q", "--bare", &remote_path]);
    fx.git(&["remote", "add", "origin", &remote_path]);
    fx.git(&["push", "-q", "-u", "origin", "main"]);
    // A branch from far back, pushed in part; main goes on and merges it.
    fx.git(&["checkout", "-q", "-b", "feature", "main~40"]);
    for i in 0..5 {
        fx.commit("b.txt", &format!("{i}\n"), &format!("Feature {i}"));
    }
    fx.git(&["push", "-q", "origin", "feature~2:refs/heads/feature"]);
    fx.git(&["checkout", "-q", "main"]);
    fx.commit("a.txt", "local\n", "Main local");
    fx.git(&["merge", "-q", "--no-ff", "-m", "Merge feature", "feature"]);
    fx.git(&["checkout", "-q", "-b", "spare", "main~1"]);
    fx.commit("c.txt", "1\n", "Spare");
    fx.git(&["checkout", "-q", "main"]);
    let exact: HashSet<String> = fx
        .git(&["rev-list", "--branches", "HEAD", "--not", "--remotes"])
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(exact.len(), 5);

    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions::default())
        .unwrap();
    assert!(!history.unpushed_pending);
    let marked: HashSet<String> = history
        .rows
        .iter()
        .filter(|r| r.unpushed)
        .map(|r| r.id.clone())
        .collect();
    assert_eq!(marked, exact);
}

#[test]
fn history_limit_truncates() {
    let mut fx = Fixture::new();
    for i in 0..5 {
        fx.commit("a.txt", &format!("{i}\n"), &format!("Commit {i}"));
    }
    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions {
            limit: 3,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(history.rows.len(), 3);
    assert!(history.truncated);
    // The oldest loaded commit's line keeps going down past the limit.
    assert_eq!(history.segments_at(2).len(), 1);
}

#[test]
fn every_commit_knows_its_branch() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "1\n", "Root");
    fx.git(&["checkout", "-q", "-b", "auth/1-models"]);
    fx.commit("m.txt", "1\n", "Models");
    fx.git(&["checkout", "-q", "-b", "auth/2-api"]);
    fx.commit("api.txt", "1\n", "Api");
    fx.git(&["checkout", "-q", "main"]);
    fx.commit("a.txt", "2\n", "Main work");

    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions::default())
        .unwrap();
    let branches: Vec<_> = history
        .rows
        .iter()
        .map(|r| (r.summary.as_str(), r.graph.branch.as_deref()))
        .collect();
    assert_eq!(
        branches,
        [
            ("Main work", Some("main")),
            ("Api", Some("auth/2-api")),
            ("Models", Some("auth/1-models")),
            ("Root", Some("main")),
        ]
    );
}

#[test]
fn stashes_hang_off_the_commit_they_were_made_on() {
    let mut fx = Fixture::new();
    let root = fx.commit("a.txt", "1\n", "Root");
    fx.write("a.txt", "2\n");
    fx.write("new.txt", "untracked\n");
    fx.git(&["stash", "push", "-q", "--include-untracked", "-m", "First try"]);
    let base = fx.commit("b.txt", "1\n", "Second");
    fx.write("b.txt", "2\n");
    fx.git(&["stash", "push", "-q", "-m", "Second try"]);

    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions::default())
        .unwrap();
    let rows: Vec<_> = history.rows.iter().map(|r| r.summary.as_str()).collect();
    assert_eq!(rows, ["On main: Second try", "Second", "On main: First try", "Root"]);

    assert_eq!(history.stashes.len(), 2);
    assert_eq!(history.stashes[0].message, "On main: Second try");
    let newest = &history.rows[0];
    assert_eq!(newest.id, history.stashes[0].id);
    assert_eq!(newest.parents, [base]);
    assert_eq!(newest.graph.color, STASH_COLOR);
    assert!(newest.unpushed);
    assert_eq!(newest.labels[0].name, "stash@{0}");
    assert_eq!(newest.labels[0].kind, RefKind::Stash);

    // The index and untracked-files commits of a stash are not rows.
    let older = &history.rows[2];
    assert_eq!(older.labels[0].name, "stash@{1}");
    assert_eq!(older.parents, [root]);
    assert_eq!(history.rows.len(), 4);
}

#[test]
fn uncommitted_changes_sit_on_top_in_the_color_of_head() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "1\n", "Root");
    fx.git(&["checkout", "-q", "-b", "feature/x"]);
    fx.commit("b.txt", "1\n", "Feature work");
    fx.write("b.txt", "2\n");
    fx.write("c.txt", "new\n");
    fx.git(&["add", "c.txt"]);

    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions::default())
        .unwrap();
    let wip = &history.rows[0];
    assert_eq!(wip.id, oxbow_core::WORKTREE_ID);
    let summary = wip.worktree.as_ref().unwrap();
    assert_eq!((summary.files, summary.staged, summary.unstaged), (2, 1, 1));
    let head = &history.rows[1];
    assert_eq!(head.summary, "Feature work");
    assert_eq!(wip.parents, std::slice::from_ref(&head.id));
    // Same color as HEAD's line, which keeps its own color and branch.
    assert_eq!(wip.graph.color, color_for_name("feature/x"));
    assert_eq!(head.graph.color, color_for_name("feature/x"));
    assert_eq!(head.graph.branch.as_deref(), Some("feature/x"));
    assert!(history.segments_at(0)[0].dashed);
    assert_eq!(history.trunk_tip_row, Some(2));

    // A clean working copy has no such row.
    fx.git(&["stash", "-q"]);
    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions::default())
        .unwrap();
    assert!(history.rows.iter().all(|r| r.worktree.is_none()));
}

#[test]
fn the_chosen_production_branch_is_the_trunk() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "1\n", "Root");
    fx.git(&["checkout", "-q", "-b", "develop"]);
    fx.commit("b.txt", "1\n", "Develop work");
    fx.git(&["checkout", "-q", "main"]);
    fx.commit("a.txt", "2\n", "Main work");

    let repo = Repo::open(fx.path()).unwrap();
    repo.set_local_config(oxbow_core::history::PRODUCTION_KEY, Some("refs/heads/develop"))
        .unwrap();
    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions::default())
        .unwrap();
    assert_eq!(history.trunk.as_deref(), Some("develop"));
    let develop = history.rows.iter().find(|r| r.summary == "Develop work").unwrap();
    assert_eq!(develop.graph.color, TRUNK_COLOR);
    // main is checked out, so it takes column 0, but in its own color.
    let main = history.rows.iter().find(|r| r.summary == "Main work").unwrap();
    assert_eq!((main.graph.column, main.graph.color), (0, color_for_name("main")));

    // A chosen branch that is gone leaves no trunk rather than falling back.
    repo.set_local_config(oxbow_core::history::PRODUCTION_KEY, Some("refs/heads/gone"))
        .unwrap();
    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions::default())
        .unwrap();
    assert_eq!(history.trunk, None);
}

#[test]
fn without_main_or_master_the_checked_out_branch_is_not_the_trunk() {
    let mut fx = Fixture::new();
    fx.git(&["checkout", "-q", "-b", "trunk-less"]);
    fx.commit("a.txt", "1\n", "Root");
    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions::default())
        .unwrap();
    assert_eq!(history.trunk, None);
    assert_ne!(history.rows[0].graph.color, TRUNK_COLOR);
}

#[test]
fn a_line_without_a_name_keeps_its_color_as_history_grows() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "1\n", "Root");
    fx.git(&["checkout", "-q", "-b", "topic"]);
    fx.commit("b.txt", "1\n", "Topic work");
    fx.git(&["checkout", "-q", "main"]);
    fx.commit("a.txt", "2\n", "Main work");
    fx.git(&["merge", "-q", "--no-ff", "-m", "Merge work", "topic"]);
    fx.git(&["branch", "-q", "-D", "topic"]);
    let color = |fx: &Fixture| {
        let history = Repo::open(fx.path())
            .unwrap()
            .history(&HistoryOptions::default())
            .unwrap();
        history
            .rows
            .iter()
            .find(|r| r.summary == "Topic work")
            .unwrap()
            .graph
            .color
    };
    let before = color(&fx);
    fx.commit("a.txt", "3\n", "Later");
    fx.commit("a.txt", "4\n", "Even later");
    assert_eq!(color(&fx), before);
}

#[test]
fn until_loads_down_to_an_older_commit() {
    let mut fx = Fixture::new();
    let mut ids = Vec::new();
    for i in 0..8 {
        ids.push(fx.commit("a.txt", &format!("{i}\n"), &format!("Commit {i}")));
    }
    let repo = Repo::open(fx.path()).unwrap();
    // Commit 1 is the 7th newest: past a limit of 3, so the history goes on to it.
    let history = repo
        .history(&HistoryOptions {
            limit: 3,
            until: Some(ids[1].clone()),
            ..Default::default()
        })
        .unwrap();
    assert!(history.rows.iter().any(|r| r.id == ids[1]));
    // A whole page more was loaded, which here is the rest of the history.
    assert_eq!(history.rows.len(), 8);
    assert_eq!(history.loaded, 8);
    assert!(!history.truncated);

    // A commit no branch reaches: the history stays as asked for.
    fx.git(&["switch", "-q", "--detach", &ids[0]]);
    let lost = fx.commit("b.txt", "x\n", "Lost");
    fx.git(&["switch", "-q", "main"]);
    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions {
            limit: 3,
            until: Some(lost),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(history.rows.len(), 3);
    assert!(history.truncated);
}

#[test]
fn straight_lines_come_as_runs() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "1\n", "Root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    for i in 0..3 {
        fx.commit("b.txt", &format!("{i}\n"), &format!("Feature {i}"));
    }
    fx.git(&["switch", "-q", "main"]);
    for i in 0..3 {
        fx.commit("a.txt", &format!("main {i}\n"), &format!("Main {i}"));
    }
    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions::default())
        .unwrap();
    // The trunk runs straight down column 0 through all its rows: one run, not a segment per row.
    assert!(
        history
            .runs
            .iter()
            .any(|run| run.column == 0 && run.thick && run.last > run.first)
    );
    for row in &history.rows {
        assert!(row.graph.segments.iter().all(|s| s.from != s.to));
    }
    // Every row but the root still has its lines.
    let last = history.rows.len() - 1;
    for row in 0..last {
        assert!(!history.segments_at(row).is_empty(), "row {row}");
    }
    assert!(history.segments_at(last).is_empty());
}

#[test]
fn a_shallow_clone_says_so() {
    let mut fx = Fixture::new();
    for i in 0..5 {
        fx.commit("a.txt", &format!("{i}\n"), &format!("Commit {i}"));
    }
    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions::default())
        .unwrap();
    assert!(!history.shallow);

    let dir = tempfile::tempdir().unwrap();
    let url = format!("file://{}", fx.path().display());
    let clone = dir.path().join("clone");
    let status = std::process::Command::new("git")
        .args(["clone", "-q", "--depth", "2", &url])
        .arg(&clone)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .status()
        .unwrap();
    assert!(status.success());
    let repo = Repo::open(&clone).unwrap();
    let history = repo.history(&HistoryOptions::default()).unwrap();
    assert!(history.shallow);
    assert_eq!(history.rows.len(), 2);
    assert!(!history.truncated);

    repo.perform(&oxbow_core::Action::Unshallow {
        remote: "origin".into(),
    })
    .unwrap();
    let history = Repo::open(&clone).unwrap().history(&HistoryOptions::default()).unwrap();
    assert!(!history.shallow);
    assert_eq!(history.rows.len(), 5);
}

#[test]
fn a_reload_with_nothing_moved_reuses_the_rows() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "1\n", "Root");
    fx.commit("a.txt", "2\n", "Second");
    let repo = Repo::open(fx.path()).unwrap();
    let options = HistoryOptions::default();
    let mut cache = oxbow_core::HistoryCache::default();

    let first = repo.history_with(&options, &mut cache, None).unwrap();
    assert!(!first.same);
    assert_eq!(first.rows.len(), 2);

    // Nothing moved: the caller's rows stand, only the key comes back.
    let again = repo.history_with(&options, &mut cache, Some(&first.key)).unwrap();
    assert_eq!(again.key, first.key);
    assert!(again.same && again.rows.is_empty() && again.runs.is_empty());

    // Edited files add the row of uncommitted changes: new rows. Its counts follow the files.
    fx.write("a.txt", "3\n");
    let dirty = repo.history_with(&options, &mut cache, Some(&first.key)).unwrap();
    assert!(!dirty.same);
    let dirty_rows = assemble(&first.rows, &dirty);
    assert_eq!(dirty_rows.len(), 3);
    assert_eq!(dirty.rows[0].worktree.as_ref().unwrap().unstaged, 1);
    fx.write("b.txt", "new\n");
    let more = repo.history_with(&options, &mut cache, Some(&dirty.key)).unwrap();
    assert!(more.same);
    assert_eq!(more.rows.len(), 1, "only the row of uncommitted changes");
    assert_eq!(more.rows[0].worktree.as_ref().unwrap().unstaged, 2);
    assert_eq!(more.changes.as_ref().unwrap().unstaged.len(), 2);

    // A commit moves the branch: the rows are built again.
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "-q", "-m", "Third"]);
    let moved = repo.history_with(&options, &mut cache, Some(&more.key)).unwrap();
    assert!(!moved.same);
    assert_eq!(moved.rows[0].summary, "Third");
    let moved_rows = assemble(&dirty_rows, &moved);
    assert_eq!(moved_rows, repo.history(&options).unwrap().rows);
    assert_eq!(moved_rows.len(), 3);
}

/// The rows of `answer`, put together from `known`, the rows of the key it was asked with.
fn assemble(known: &[HistoryRow], answer: &History) -> Vec<HistoryRow> {
    if answer.same {
        let mut rows = answer.rows.clone();
        rows.extend(known.iter().filter(|r| r.worktree.is_none()).cloned());
        return rows;
    }
    if answer.pieces.is_empty() {
        return answer.rows.clone();
    }
    let mut fresh = answer.rows.iter();
    let mut rows = Vec::new();
    for piece in &answer.pieces {
        match piece.known {
            Some(from) => rows.extend(known[from..from + piece.len].iter().cloned()),
            None => rows.extend(fresh.by_ref().take(piece.len).cloned()),
        }
    }
    assert!(fresh.next().is_none(), "every new row has its place");
    rows
}

#[test]
fn loading_more_commits_sends_only_the_new_rows() {
    let mut fx = Fixture::new();
    for n in 1..=12 {
        let file = if (5..=8).contains(&n) { "s.txt" } else { "a.txt" };
        fx.commit(file, &format!("{n}\n"), &format!("Commit {n}"));
        if n == 4 {
            fx.git(&["checkout", "-q", "-b", "side"]);
        }
        if n == 8 {
            fx.git(&["checkout", "-q", "main"]);
        }
    }
    fx.git(&["merge", "-q", "--no-edit", "side"]);
    let repo = Repo::open(fx.path()).unwrap();
    let mut cache = oxbow_core::HistoryCache::default();
    let page = |limit| HistoryOptions {
        limit,
        ..Default::default()
    };

    let first = repo.history_with(&page(5), &mut cache, None).unwrap();
    assert!(first.truncated && first.pieces.is_empty());
    assert_eq!(first.rows.len(), 5);

    let more = repo.history_with(&page(10), &mut cache, Some(&first.key)).unwrap();
    let rows = assemble(&first.rows, &more);
    assert_eq!(
        rows,
        repo.history(&page(10)).unwrap().rows,
        "the same as loaded at once"
    );
    assert!(
        more.rows.len() < rows.len(),
        "{} rows sent of {}",
        more.rows.len(),
        rows.len()
    );
    assert!(more.pieces.iter().any(|p| p.known == Some(0)));

    // A new commit on top: the rows below it come from the caller.
    fx.commit("b.txt", "1\n", "On top");
    let after = repo.history_with(&page(10), &mut cache, Some(&more.key)).unwrap();
    let next = assemble(&rows, &after);
    assert_eq!(next, repo.history(&page(10)).unwrap().rows);
    assert_eq!(next[0].summary, "On top");
    assert!(after.rows.len() < next.len());
}

#[test]
fn rows_asked_with_an_old_key_come_whole() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "1\n", "Root");
    fx.commit("a.txt", "2\n", "Second");
    let repo = Repo::open(fx.path()).unwrap();
    let options = HistoryOptions::default();
    let mut cache = oxbow_core::HistoryCache::default();
    let first = repo.history_with(&options, &mut cache, None).unwrap();
    fx.commit("a.txt", "3\n", "Third");
    let _dropped = repo.history_with(&options, &mut cache, Some(&first.key)).unwrap();
    fx.commit("a.txt", "4\n", "Fourth");
    // The caller still has the first rows: the last ones built are not what it has.
    let next = repo.history_with(&options, &mut cache, Some(&first.key)).unwrap();
    assert!(next.pieces.is_empty());
    assert_eq!(next.rows.len(), 4);
}

#[test]
fn a_deeper_fetch_of_a_shallow_clone_rebuilds_the_rows() {
    let mut origin = Fixture::new();
    for n in 1..=6 {
        origin.commit("a.txt", &format!("{n}\n"), &format!("Commit {n}"));
    }
    let mut clone = Fixture::new();
    let url = format!("file://{}", origin.path().display());
    let dir = clone.path().join("shallow");
    clone.git(&["clone", "-q", "--depth=3", &url, dir.to_str().unwrap()]);
    let repo = Repo::open(&dir).unwrap();
    let options = HistoryOptions::default();
    let mut cache = oxbow_core::HistoryCache::default();

    let first = repo.history_with(&options, &mut cache, None).unwrap();
    assert_eq!(first.rows.len(), 3);

    // No ref moves, but the history now stops one commit further back.
    clone.git(&["-C", dir.to_str().unwrap(), "fetch", "-q", "--deepen=1", "origin"]);
    let deeper = repo.history_with(&options, &mut cache, Some(&first.key)).unwrap();
    assert!(!deeper.same);
    assert_eq!(assemble(&first.rows, &deeper).len(), 4);

    let again = repo.history_with(&options, &mut cache, Some(&deeper.key)).unwrap();
    assert!(again.same);
}

#[test]
fn history_lists_the_git_commands_it_ran() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "1\n", "Root");
    let repo = Repo::open(fx.path()).unwrap();
    let history = repo.history(&HistoryOptions::default()).unwrap();
    assert!(
        history.reads.iter().any(|c| c.starts_with("git status --porcelain=v2")),
        "{:?}",
        history.reads
    );
    assert!(history.reads.iter().any(|c| c.starts_with("git for-each-ref")));
    // Each read lists its own commands only.
    let again = repo.history(&HistoryOptions::default()).unwrap();
    assert_eq!(again.reads, history.reads);
}

#[test]
fn far_behind_remote_branches_still_mark_the_loaded_commits() {
    use std::io::Write;
    let mut fx = Fixture::new();
    fx.commit("a.txt", "root\n", "Root");
    let root = fx.git(&["rev-parse", "HEAD"]);
    // 12,000 commits on top of the root, more than the walk of unpushed ones takes.
    let mut script = String::new();
    for i in 1..=12_000 {
        script.push_str(&format!(
            "commit refs/heads/main\ncommitter A <a@x> {} +0000\ndata 3\nc{}\n{}M 644 inline a.txt\ndata 2\n{}\n\n",
            1_700_000_000 + i,
            i % 10,
            if i == 1 {
                format!("from {root}\n")
            } else {
                String::new()
            },
            i % 10
        ));
    }
    let mut child = std::process::Command::new("git")
        .args(["fast-import", "--quiet"])
        .current_dir(fx.path())
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(script.as_bytes()).unwrap();
    assert!(child.wait().unwrap().success());
    fx.git(&["reset", "-q", "--hard", "main"]);

    // The remote is still at the root: every loaded commit is unpushed.
    fx.git(&["update-ref", "refs/remotes/origin/main", &root]);
    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions::default())
        .unwrap();
    assert_eq!(history.rows.len(), 2000);
    assert!(history.rows.iter().all(|r| r.unpushed));

    // A thousand commits ahead: the exact walk.
    let pushed = fx.git(&["rev-parse", "main~1000"]);
    fx.git(&["update-ref", "refs/remotes/origin/main", &pushed]);
    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions::default())
        .unwrap();
    let ahead = history.rows.iter().take_while(|r| r.unpushed).count();
    assert_eq!(ahead, 1000);
    assert!(history.rows[1000..].iter().all(|r| !r.unpushed));
}

#[test]
fn unpushed_marks_match_git_however_far_the_walk_would_go() {
    use std::collections::HashSet;
    use std::io::Write;
    use std::sync::atomic::AtomicBool;
    let mut fx = Fixture::new();
    fx.commit("a.txt", "root\n", "Root");
    let root = fx.git(&["rev-parse", "HEAD"]);
    // main and side each 12,000 commits on the root, their dates interleaved. origin/main is a
    // commit of its own on main~50, dated before all of them: the loaded commits it reaches
    // are pushed, though it is not loaded itself.
    let mut script = String::new();
    let mut add = |branch: &str, mark: usize, from: Option<String>, time: i64| {
        script.push_str(&format!(
            "commit refs/heads/{branch}\nmark :{mark}\ncommitter A <a@x> {time} +0000\ndata 2\nc\n{}M 644 inline a.txt\ndata 2\n{}\n\n",
            from.map_or_else(String::new, |f| format!("from {f}\n")),
            mark % 10
        ));
    };
    for i in 1..=12_000 {
        let first = (i == 1).then(|| root.clone());
        add("main", i, first.clone(), 1_700_000_000 + 2 * i as i64);
        add("side", 20_000 + i, first, 1_700_000_001 + 2 * i as i64);
    }
    add("pushed", 40_000, Some(":11950".into()), 1_600_000_000);
    let mut child = std::process::Command::new("git")
        .args(["fast-import", "--quiet"])
        .current_dir(fx.path())
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(script.as_bytes()).unwrap();
    assert!(child.wait().unwrap().success());
    fx.git(&["update-ref", "refs/remotes/origin/main", "pushed"]);
    fx.git(&["update-ref", "-d", "refs/heads/pushed"]);
    fx.git(&["reset", "-q", "--hard", "main"]);
    let exact: HashSet<String> = fx
        .git(&["rev-list", "--branches", "--not", "--remotes"])
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(exact.len(), 12_050);

    let repo = Repo::open(fx.path()).unwrap();
    let history = repo.history(&HistoryOptions::default()).unwrap();
    assert_eq!(history.rows.len(), 2000);
    // Past the walk it takes, the commits it met are marked and the others wait.
    assert!(history.unpushed_pending);
    assert!(
        history
            .rows
            .iter()
            .filter(|r| r.unpushed)
            .all(|r| exact.contains(&r.id))
    );
    let unknown: Vec<String> = history
        .rows
        .iter()
        .filter(|r| !r.unpushed)
        .map(|r| r.id.clone())
        .collect();
    assert!(repo.settle_unpushed(&unknown, &AtomicBool::new(false)).unwrap());

    // Opened afresh, as each reload does, it marks them as git does.
    for history in [
        repo.history(&HistoryOptions::default()).unwrap(),
        repo.reopen().unwrap().history(&HistoryOptions::default()).unwrap(),
    ] {
        assert!(!history.unpushed_pending);
        let wrong: Vec<&str> = history
            .rows
            .iter()
            .filter(|r| r.unpushed != exact.contains(&r.id))
            .map(|r| r.summary.as_str())
            .collect();
        assert!(wrong.is_empty(), "{} rows marked wrong", wrong.len());
        assert!(history.rows.iter().any(|r| !r.unpushed));
    }
    // A stopped settling keeps nothing.
    assert!(matches!(
        repo.reopen()
            .unwrap()
            .settle_unpushed(&["0".repeat(40)], &AtomicBool::new(true)),
        Err(oxbow_core::Error::Cancelled)
    ));
}

#[test]
fn current_branch_only_shows_the_branch_its_upstream_and_the_trunk() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "1\n", "Root");
    fx.git(&["checkout", "-q", "-b", "topic"]);
    fx.commit("t.txt", "1\n", "Topic work");
    fx.git(&["checkout", "-q", "main"]);
    fx.commit("a.txt", "2\n", "Main work");
    fx.git(&["merge", "-q", "--no-edit", "topic"]);
    fx.git(&["checkout", "-q", "-b", "other", "main~1"]);
    fx.commit("o.txt", "1\n", "Other work");
    fx.git(&["checkout", "-q", "-b", "feature", "main"]);
    fx.commit("f.txt", "1\n", "Feature work");
    fx.write("f.txt", "2\n");

    let repo = Repo::open(fx.path()).unwrap();
    assert!(repo.history(&HistoryOptions::default()).unwrap().focus.is_none());
    let history = repo
        .history(&HistoryOptions {
            focus: true,
            ..Default::default()
        })
        .unwrap();
    let focus = history.focus.as_ref().unwrap();
    let shown: Vec<_> = focus.rows.iter().map(|&r| history.rows[r].summary.as_str()).collect();
    assert_eq!(
        shown,
        [
            "Uncommitted changes",
            "Feature work",
            "Merge branch 'topic'",
            "Main work",
            "Root"
        ]
    );
    assert_eq!(focus.graph.len(), focus.rows.len());
    // The feature line sits in column 0, the trunk beside it until the fork point.
    assert_eq!(focus.graph[1].column, 0);
    // The merged topic line is left out: it stops short instead of running on.
    let merge = &focus.graph[2];
    assert_eq!(merge.cuts.len(), 1);
    assert_eq!(merge.cuts[0].color, color_for_name("topic"));
    assert_ne!(merge.cuts[0].to, merge.column);
    assert!(merge.width >= merge.cuts[0].to);
    assert_eq!(merge.merge_colors, [color_for_name("topic")]);
    // The whole history keeps every commit.
    assert_eq!(history.rows.len(), 7);
    assert!(history.rows.iter().all(|r| r.graph.cuts.is_empty()));
}

#[test]
fn current_branch_only_follows_a_new_upstream_without_a_ref_moving() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "1\n", "Root");
    fx.git(&["checkout", "-q", "-b", "old"]);
    fx.commit("o.txt", "1\n", "Old upstream work");
    fx.git(&["checkout", "-q", "-b", "new", "main"]);
    fx.commit("n.txt", "1\n", "New upstream work");
    fx.git(&["checkout", "-q", "-b", "feature", "main"]);
    fx.commit("f.txt", "1\n", "Feature work");
    fx.git(&["remote", "add", "origin", "https://example.com/r.git"]);
    fx.git(&["update-ref", "refs/remotes/origin/old", "old"]);
    fx.git(&["update-ref", "refs/remotes/origin/new", "new"]);
    fx.git(&["branch", "-q", "-D", "old", "new"]);
    fx.git(&["branch", "-q", "--set-upstream-to=origin/old", "feature"]);

    let repo = Repo::open(fx.path()).unwrap();
    let options = HistoryOptions {
        focus: true,
        ..Default::default()
    };
    let mut cache = oxbow_core::HistoryCache::default();
    let shown = |h: &oxbow_core::History, rows: &[oxbow_core::HistoryRow]| -> Vec<String> {
        let focus = h.focus.as_ref().unwrap();
        focus.rows.iter().map(|&r| rows[r].summary.clone()).collect()
    };
    let first = repo.history_with(&options, &mut cache, None).unwrap();
    let rows = first.rows.clone();
    assert!(shown(&first, &rows).contains(&"Old upstream work".to_owned()));

    fx.git(&["branch", "-q", "--set-upstream-to=origin/new", "feature"]);
    let again = repo.history_with(&options, &mut cache, Some(&first.key)).unwrap();
    assert!(!again.same, "a new upstream makes a new focused graph");
    let now = shown(&again, &assemble(&rows, &again));
    assert!(now.contains(&"New upstream work".to_owned()), "{now:?}");
    assert!(!now.contains(&"Old upstream work".to_owned()), "{now:?}");
}

#[test]
fn a_failing_status_keeps_the_row_of_changes_as_unknown() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\n", "First");
    // A damaged index: `git status` can't read it, the commits still can be.
    std::fs::write(fx.path().join(".git/index"), "not an index").unwrap();
    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions::default())
        .unwrap();
    assert!(history.changes.is_none());
    assert!(
        history
            .problems
            .iter()
            .any(|p| p.part == oxbow_core::ReadPart::Changes && !p.error.is_empty())
    );
    // Without a .gitmodules there are no submodules, read or not: that part didn't fail.
    assert!(
        !history
            .problems
            .iter()
            .any(|p| p.part == oxbow_core::ReadPart::Submodules),
        "{:?}",
        history.problems
    );
    let row = &history.rows[0];
    assert!(row.worktree.as_ref().is_some_and(|w| w.unknown));
    assert_eq!(history.rows[1].summary, "First");
}

#[test]
fn a_clean_read_has_no_problems() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\n", "First");
    let history = Repo::open(fx.path())
        .unwrap()
        .history(&HistoryOptions::default())
        .unwrap();
    assert!(history.problems.is_empty());
    assert!(history.rows.iter().all(|r| r.worktree.is_none()));
}

#[test]
fn a_quiet_refresh_with_nothing_changed_runs_no_git() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "1\n", "Root");
    fx.git(&["checkout", "-q", "-b", "feature"]);
    fx.commit("b.txt", "1\n", "On feature");
    fx.write("a.txt", "edited\n");
    let repo = Repo::open(fx.path()).unwrap();
    let mut cache = oxbow_core::HistoryCache::default();
    // `seen`: what the window's watch had noticed when each read began.
    let read = |cache: &mut oxbow_core::HistoryCache, seen: u64, quiet: bool, known: Option<&str>| {
        let options = HistoryOptions {
            seen: Some(seen),
            quiet,
            ..Default::default()
        };
        repo.history_with(&options, cache, known).unwrap()
    };
    let asked = |h: &History, what: &str| h.reads.iter().any(|r| r.contains(what));

    let first = read(&mut cache, 1, false, None);
    assert!(asked(&first, "status") && asked(&first, "for-each-ref") && asked(&first, "worktree list"));
    assert!(!first.unchanged);

    // The window came back to the front; the watch noticed nothing.
    let quiet = read(&mut cache, 1, true, Some(&first.key));
    assert_eq!(quiet.reads, Vec::<String>::new());
    assert!(quiet.unchanged && quiet.same);
    assert_eq!(format!("{:?}", quiet.changes), format!("{:?}", first.changes));
    assert_eq!(quiet.worktrees, first.worktrees);

    // A reload after something done here asks for the changes again, not for what the refs
    // and settings still give.
    let after = read(&mut cache, 1, false, Some(&first.key));
    assert!(asked(&after, "status") && !asked(&after, "for-each-ref") && !asked(&after, "worktree list"));
    assert!(!after.unchanged);

    // The watch noticed an edit.
    fx.write("b.txt", "edited\n");
    let edited = read(&mut cache, 2, true, Some(&after.key));
    assert!(asked(&edited, "status") && !asked(&edited, "for-each-ref"));
    assert_eq!(edited.changes.as_ref().unwrap().unstaged.len(), 2);
    assert!(!edited.unchanged);

    // Staged outside, before the watch counted it: what is staged says so.
    fx.git(&["add", "b.txt"]);
    let staged = read(&mut cache, 2, true, Some(&edited.key));
    assert!(asked(&staged, "status"));
    assert_eq!(staged.changes.as_ref().unwrap().staged.len(), 1);

    // A new branch moves no file: the changes are taken again, the upstreams asked for.
    fx.git(&["branch", "other"]);
    let branched = read(&mut cache, 2, true, Some(&staged.key));
    assert!(!asked(&branched, "status") && asked(&branched, "for-each-ref") && asked(&branched, "worktree list"));
    assert!(!branched.unchanged);
    assert!(read(&mut cache, 2, true, Some(&branched.key)).unchanged);

    // Another worktree, and later its folder gone.
    let other = fx.path().parent().unwrap().join("other-folder");
    fx.git(&["worktree", "add", "-q", other.to_str().unwrap(), "other"]);
    let added = read(&mut cache, 2, true, Some(&branched.key));
    assert!(asked(&added, "worktree list"));
    assert_eq!(added.worktrees.len(), 2);
    std::fs::remove_dir_all(&other).unwrap();
    let gone = read(&mut cache, 2, true, Some(&added.key));
    assert!(asked(&gone, "worktree list"));
    assert!(gone.worktrees[1].missing);

    // Without a watch of the files, the changes are always asked for.
    let unwatched = HistoryOptions {
        quiet: true,
        ..Default::default()
    };
    let blind = repo.history_with(&unwatched, &mut cache, Some(&gone.key)).unwrap();
    assert!(asked(&blind, "status") && !blind.unchanged);
}
