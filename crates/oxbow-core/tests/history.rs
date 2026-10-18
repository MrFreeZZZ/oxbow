mod support;

use oxbow_core::graph::{STASH_COLOR, TRUNK_COLOR, color_for_name};
use oxbow_core::{History, HistoryOptions, HistoryRow, RefKind, Repo};
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
    assert!(history.segments_at(0)[0].dashed);
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
        .history(&HistoryOptions { limit: 3, until: None })
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
    let page = |limit| HistoryOptions { limit, until: None };

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
