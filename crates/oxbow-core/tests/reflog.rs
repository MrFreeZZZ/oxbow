mod support;

use oxbow_core::Repo;
use support::Fixture;

#[test]
fn a_hard_reset_in_a_terminal_leaves_the_commits_as_lost_work() {
    let mut fx = Fixture::new();
    let root = fx.commit("a.txt", "a\n", "Root");
    let first = fx.commit("b.txt", "b\n", "Add b");
    let second = fx.commit("c.txt", "c\n", "Add c");
    fx.git(&["reset", "-q", "--hard", &root]);
    let repo = Repo::open(fx.path()).unwrap();

    let entries = repo.reflog_entries().unwrap();
    let get = |id: &str| {
        entries
            .iter()
            .find(|e| e.id == id)
            .unwrap_or_else(|| panic!("{id} in {entries:#?}"))
    };

    let tip = get(&second);
    assert!(tip.lost);
    assert_eq!(tip.lost_count, 2);
    assert_eq!(tip.within, None);
    assert_eq!(tip.summary, "Add c");
    assert_eq!(tip.kind, "commit");
    assert_eq!(tip.parents, vec![first.clone()]);
    assert_eq!(tip.expires, Some(tip.time + 30 * 86_400));

    let inner = get(&first);
    assert!(inner.lost);
    assert_eq!(inner.within.as_deref(), Some(second.as_str()));

    // The reset itself moved HEAD and main to a commit main has.
    let now = get(&root);
    assert!(!now.lost);
    assert_eq!(now.kind, "reset");
    assert_eq!(now.expires, None);
    assert_eq!(entries[0].id, root, "newest first");
}

#[test]
fn a_deleted_branch_is_found_through_heads_reflog() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.git(&["switch", "-q", "-c", "topic"]);
    let work = fx.commit("t.txt", "t\n", "Topic work");
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["branch", "-q", "-D", "topic"]);
    let repo = Repo::open(fx.path()).unwrap();

    let entries = repo.reflog_entries().unwrap();
    let found = entries.iter().find(|e| e.id == work).expect("the topic commit");
    assert!(found.lost);
    assert_eq!(found.source, "HEAD");
    assert!(found.selector.starts_with("HEAD@{"), "{}", found.selector);
}

#[test]
fn an_amended_commit_keeps_the_old_version_and_names_the_branch() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    let old = fx.commit("b.txt", "b\n", "Add b");
    fx.write("b.txt", "better b\n");
    fx.git(&["commit", "-q", "-a", "--amend", "-m", "Add a better b"]);
    let amended = fx.git(&["rev-parse", "HEAD"]);
    let repo = Repo::open(fx.path()).unwrap();

    let entries = repo.reflog_entries().unwrap();
    let new = entries.iter().find(|e| e.id == amended).unwrap();
    assert_eq!(new.kind, "amend");
    assert_eq!(new.source, "main", "a branch's entry is preferred to HEAD's");
    assert!(new.selector.starts_with("main@{"));
    assert!(!new.lost);
    let before = entries.iter().find(|e| e.id == old).unwrap();
    assert!(before.lost);
    assert_eq!(before.lost_count, 1);
}

#[test]
fn a_branch_made_at_a_lost_commit_keeps_it() {
    let mut fx = Fixture::new();
    let root = fx.commit("a.txt", "a\n", "Root");
    let work = fx.commit("b.txt", "b\n", "Add b");
    fx.git(&["reset", "-q", "--hard", &root]);
    fx.git(&["branch", "rescue", &work]);
    let repo = Repo::open(fx.path()).unwrap();

    let entries = repo.reflog_entries().unwrap();
    assert!(entries.iter().all(|e| !e.lost), "{entries:#?}");
}

#[test]
fn the_expiry_follows_git_config() {
    let mut fx = Fixture::new();
    let root = fx.commit("a.txt", "a\n", "Root");
    let work = fx.commit("b.txt", "b\n", "Add b");
    fx.git(&["reset", "-q", "--hard", &root]);
    fx.git(&["config", "gc.reflogExpireUnreachable", "2.weeks"]);
    let repo = Repo::open(fx.path()).unwrap();
    let entries = repo.reflog_entries().unwrap();
    let lost = entries.iter().find(|e| e.id == work).unwrap();
    assert_eq!(lost.expires, Some(lost.time + 14 * 86_400));

    fx.git(&["config", "gc.reflogExpireUnreachable", "never"]);
    let entries = repo.reflog_entries().unwrap();
    assert_eq!(entries.iter().find(|e| e.id == work).unwrap().expires, None);
}

#[test]
fn a_new_repository_has_no_reflog() {
    let fx = Fixture::new();
    let repo = Repo::open(fx.path()).unwrap();
    assert!(repo.reflog_entries().unwrap().is_empty());
}
