mod support;

use std::time::{SystemTime, UNIX_EPOCH};

use oxbow_core::Repo;
use support::Fixture;

fn bare(dir: &tempfile::TempDir, name: &str) -> String {
    let path = dir.path().join(name);
    let status = std::process::Command::new("git")
        .args(["init", "-q", "--bare", "-b", "main"])
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    path.to_str().unwrap().to_owned()
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

fn time_of(repo: &Repo, remote: &str) -> Option<i64> {
    repo.fetch_times()
        .into_iter()
        .find(|f| f.remote == remote)
        .unwrap_or_else(|| panic!("{remote} is listed"))
        .time
}

#[test]
fn a_push_is_not_a_fetch_but_a_fetch_with_nothing_new_is() {
    let dir = tempfile::tempdir().unwrap();
    let url = bare(&dir, "acme.git");
    let mut me = Fixture::new();
    me.commit("a.txt", "a\n", "Root");
    me.git(&["remote", "add", "origin", &url]);
    me.git(&["push", "-q", "-u", "origin", "main"]);
    let repo = Repo::open(me.path()).unwrap();
    // origin/main only moved by the push: nothing says what else origin has.
    assert_eq!(time_of(&repo, "origin"), None);

    let before = now();
    me.git(&["fetch", "-q", "origin"]);
    let fetched = time_of(&repo, "origin").expect("FETCH_HEAD names origin");
    assert!(fetched >= before - 1, "{fetched} vs {before}");
    // The history carries the same times.
    let history = repo.history(&Default::default()).unwrap();
    assert_eq!(history.fetched.len(), 1);
    assert_eq!(history.fetched[0].time, Some(fetched));
}

#[test]
fn a_clone_counts_as_the_first_fetch() {
    let dir = tempfile::tempdir().unwrap();
    let url = bare(&dir, "acme.git");
    let mut me = Fixture::new();
    me.commit("a.txt", "a\n", "Root");
    me.git(&["remote", "add", "origin", &url]);
    me.git(&["push", "-q", "origin", "main"]);

    let copy = dir.path().join("copy");
    let status = std::process::Command::new("git")
        .args(["clone", "-q", &url])
        .arg(&copy)
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
        .status()
        .unwrap();
    assert!(status.success());
    let repo = Repo::open(&copy).unwrap();
    assert_eq!(time_of(&repo, "origin"), Some(1_700_000_000));
}

#[test]
fn each_remote_keeps_its_own_time_after_another_is_fetched() {
    let dir = tempfile::tempdir().unwrap();
    let origin = bare(&dir, "origin.git");
    let mirror = bare(&dir, "mirror.git");
    let mut me = Fixture::new();
    me.commit("a.txt", "a\n", "Root");
    me.git(&["remote", "add", "origin", &format!("{origin}/")]);
    me.git(&["remote", "add", "mirror", &mirror]);
    me.git(&["push", "-q", "-u", "origin", "main"]);
    me.git(&["push", "-q", "mirror", "main"]);
    let repo = Repo::open(me.path()).unwrap();
    assert_eq!(time_of(&repo, "mirror"), None);

    // Nothing new on origin, so only FETCH_HEAD tells; Oxbow notes it when it reloads.
    me.git(&["fetch", "-q", "origin"]);
    let origin_time = time_of(&repo, "origin").expect("origin, despite the trailing slash");
    assert_eq!(time_of(&repo, "mirror"), None, "FETCH_HEAD names only origin");

    // Fetching the mirror rewrites FETCH_HEAD; origin's time stays.
    me.git(&["fetch", "-q", "mirror"]);
    assert!(time_of(&repo, "mirror").is_some());
    assert_eq!(time_of(&repo, "origin"), Some(origin_time));
}
