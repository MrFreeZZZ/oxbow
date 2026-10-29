mod support;

use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use oxbow_core::localhistory::{GROUP, MAX_FILE};
use oxbow_core::{Action, DiffContext, LineKind, LocalHistory, Repo};
use support::Fixture;

const DAY: u64 = 24 * 60 * 60 * 1000;

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64
}

fn paths(list: &[&str]) -> Vec<String> {
    list.iter().map(|p| p.to_string()).collect()
}

/// The blobs Local History wrote itself.
fn blobs(fx: &Fixture) -> usize {
    walk(&fx.path().join(".git/oxbow/local-history/objects"))
}

fn walk(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| if e.path().is_dir() { walk(&e.path()) } else { 1 })
        .sum()
}

fn base() -> Fixture {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\ntwo\nthree\n", "First");
    fx
}

#[test]
fn each_save_is_a_version_with_what_it_changed_since_the_one_before() {
    let mut fx = base();
    let repo = Repo::open(fx.path()).unwrap();
    let mut history = LocalHistory::open(&repo);
    let t = now() - 10 * GROUP;
    fx.write("a.txt", "one\n2\nthree\n");
    assert_eq!(history.keep(&repo, &paths(&["a.txt"]), t), 1);
    fx.write("a.txt", "one\n2\nthree\nfour\nfive\n");
    assert_eq!(history.keep(&repo, &paths(&["a.txt"]), t + GROUP), 1);
    let versions = repo.local_history("a.txt").unwrap();
    assert_eq!(versions.len(), 2);
    let (newest, oldest) = (&versions[0], &versions[1]);
    assert_eq!(newest.at, t + GROUP);
    assert_eq!((newest.additions, newest.deletions), (Some(2), Some(0)));
    assert_eq!(newest.base.as_deref(), Some(oldest.id.as_str()));
    assert!(!newest.from_head);
    // The oldest changed the file as HEAD has it.
    assert!(oldest.from_head);
    assert_eq!(oldest.base, Some(fx.git(&["rev-parse", "HEAD:a.txt"])));
    assert_eq!((oldest.additions, oldest.deletions), (Some(1), Some(1)));
    assert!(newest.kept && !newest.gone && !newest.binary);
    assert_eq!(newest.size, 22);
    // Neither is in git, so both are Local History's own.
    assert_eq!(blobs(&fx), 2);
}

#[test]
fn saves_soon_after_each_other_are_one_version_and_the_same_bytes_none() {
    let fx = base();
    let repo = Repo::open(fx.path()).unwrap();
    let mut history = LocalHistory::open(&repo);
    let t = now() - 10 * GROUP;
    for (n, text) in ["1\n", "12\n", "123\n"].iter().enumerate() {
        fx.write("a.txt", text);
        history.keep(&repo, &paths(&["a.txt"]), t + n as u64 * 1000);
    }
    // Saved again unchanged, as on a touch.
    assert_eq!(history.keep(&repo, &paths(&["a.txt"]), t + 5000), 0);
    let versions = repo.local_history("a.txt").unwrap();
    assert_eq!(versions.len(), 1, "{versions:?}");
    assert_eq!(versions[0].at, t + 2000);
    // A group lasts from its first save: one a long while later is a version of its own.
    fx.write("a.txt", "1234\n");
    history.keep(&repo, &paths(&["a.txt"]), t + GROUP);
    assert_eq!(repo.local_history("a.txt").unwrap().len(), 2);
}

#[test]
fn what_git_stages_isnt_kept_and_what_git_has_elsewhere_is_kept_anyway() {
    let mut fx = base();
    let first = fx.git(&["rev-parse", "HEAD:a.txt"]);
    fx.commit("a.txt", "changed\n", "Second");
    let repo = Repo::open(fx.path()).unwrap();
    let mut history = LocalHistory::open(&repo);
    let t = now() - DAY;
    // A checkout writes what the index then stages.
    assert_eq!(history.keep(&repo, &paths(&["a.txt"]), t), 0);
    // The first commit's bytes, back by hand: git has the blob, and Local History a copy, as
    // git may drop it.
    fx.write("a.txt", "one\ntwo\nthree\n");
    assert_eq!(history.keep(&repo, &paths(&["a.txt"]), t + GROUP), 1);
    assert_eq!(repo.local_history("a.txt").unwrap()[0].id, first);
    assert_eq!(blobs(&fx), 1);
    // Taken back to what is staged, then saved as before: the second save is a version again.
    fx.write("a.txt", "changed\n");
    assert_eq!(history.keep(&repo, &paths(&["a.txt"]), t + 2 * GROUP), 0);
    fx.write("a.txt", "one\ntwo\nthree\n");
    assert_eq!(history.keep(&repo, &paths(&["a.txt"]), t + 3 * GROUP), 1);
    assert_eq!(repo.local_history("a.txt").unwrap().len(), 2);
}

#[test]
fn big_files_folders_links_scratch_files_and_submodules_are_left_out() {
    let mut fx = base();
    fx.write("big.bin", &"x".repeat(MAX_FILE as usize + 1));
    fx.write("folder/inside.txt", "in a folder\n");
    fx.write(".a.txt.swp", "vim\n");
    fx.write("a.txt~", "backup\n");
    // A submodule's folder: a gitlink in the index, its files another repository's.
    let sub = fx.git(&["rev-parse", "HEAD"]);
    fx.git(&["update-index", "--add", "--cacheinfo", &format!("160000,{sub},lib")]);
    fx.write("lib/inside.txt", "the submodule's\n");
    #[cfg(unix)]
    std::os::unix::fs::symlink("a.txt", fx.path().join("link")).unwrap();
    let repo = Repo::open(fx.path()).unwrap();
    let mut history = LocalHistory::open(&repo);
    let told = paths(&[
        "big.bin",
        "folder",
        ".a.txt.swp",
        "a.txt~",
        "lib/inside.txt",
        "link",
        "gone.txt",
    ]);
    assert_eq!(history.keep(&repo, &told, now()), 0);
    assert_eq!(history.keep(&repo, &paths(&["folder/inside.txt"]), now()), 1);
    assert_eq!(repo.local_history_all().unwrap().len(), 1);
}

#[test]
fn the_list_of_every_file_is_newest_first_and_tells_a_file_gone_from_disk() {
    let mut fx = base();
    let repo = Repo::open(fx.path()).unwrap();
    let mut history = LocalHistory::open(&repo);
    let t = now() - DAY;
    fx.write("new.txt", "draft\n");
    history.keep(&repo, &paths(&["new.txt"]), t);
    fx.write("a.txt", "edited\n");
    history.keep(&repo, &paths(&["a.txt"]), t + GROUP);
    std::fs::remove_file(fx.path().join("new.txt")).unwrap();
    let all = repo.local_history_all().unwrap();
    let names: Vec<(&str, bool)> = all.iter().map(|v| (v.path.as_str(), v.gone)).collect();
    assert_eq!(names, [("a.txt", false), ("new.txt", true)]);
    // What each changed from, without counting lines.
    assert_eq!(all[0].base, Some(fx.git(&["rev-parse", "HEAD:a.txt"])));
    assert!(all[0].from_head && all[0].additions.is_none());
    assert_eq!(all[1].base, None);
    // A file that never was in git: its oldest version changed nothing before it.
    let new = repo.local_history("new.txt").unwrap();
    assert_eq!(new[0].base, None);
    assert!(!new[0].from_head && new[0].gone);
    assert_eq!(new[0].additions, Some(1));
}

#[test]
fn old_versions_go_after_the_days_kept_and_their_blobs_with_them() {
    let fx = base();
    let repo = Repo::open(fx.path()).unwrap();
    let mut history = LocalHistory::open(&repo);
    let t = now();
    fx.write("a.txt", "eight days ago\n");
    history.keep(&repo, &paths(&["a.txt"]), t - 8 * DAY);
    fx.write("a.txt", "yesterday\n");
    history.keep(&repo, &paths(&["a.txt"]), t - DAY);
    // Too old to list even before it goes.
    assert_eq!(repo.local_history("a.txt").unwrap().len(), 1);
    assert_eq!(blobs(&fx), 2);
    history.prune(t).unwrap();
    assert_eq!(blobs(&fx), 1);
    let lists = std::fs::read_dir(fx.path().join(".git/oxbow/local-history"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .count();
    assert_eq!(lists, 1, "the old day's list is gone");
    assert_eq!(repo.local_history("a.txt").unwrap()[0].id, fx_id(&fx, "yesterday\n"));
}

fn fx_id(fx: &Fixture, text: &str) -> String {
    let file = fx.path().join("scratch-for-id");
    std::fs::write(&file, text).unwrap();
    let id = std::process::Command::new("git")
        .args(["hash-object", "scratch-for-id"])
        .current_dir(fx.path())
        .output()
        .unwrap();
    std::fs::remove_file(file).unwrap();
    String::from_utf8_lossy(&id.stdout).trim().to_owned()
}

#[test]
fn past_the_space_kept_the_oldest_versions_go_first() {
    let fx = base();
    let repo = Repo::open(fx.path()).unwrap();
    let mut history = LocalHistory::open(&repo);
    let t = now() - DAY;
    for n in 0..4u64 {
        // Bytes that don't compress, so each blob takes about as much as the file.
        let text: String = (0..2000u64)
            .map(|i| format!("{:x}", (i * 7919 + n * 104729) % 65521))
            .collect();
        fx.write("a.txt", &text);
        history.keep(&repo, &paths(&["a.txt"]), t + n * GROUP);
    }
    let all = history.prune_within(now(), u64::MAX).unwrap();
    assert_eq!(blobs(&fx), 4);
    let kept = history.prune_within(now(), all / 2 + 1).unwrap();
    assert!(kept <= all / 2 + 1, "{kept} of {all}");
    let versions = repo.local_history("a.txt").unwrap();
    assert_eq!(versions.len(), blobs(&fx));
    assert!(versions.len() < 4);
    // The newest stays.
    assert_eq!(versions[0].at, t + 3 * GROUP);
}

#[test]
fn a_version_shows_what_it_changed_and_how_the_file_differs_from_it_now() {
    let fx = base();
    let repo = Repo::open(fx.path()).unwrap();
    let mut history = LocalHistory::open(&repo);
    let t = now() - DAY;
    fx.write("a.txt", "one\nTWO\nthree\n");
    history.keep(&repo, &paths(&["a.txt"]), t);
    fx.write("a.txt", "one\nTWO\nthree\nfour\n");
    let version = &repo.local_history("a.txt").unwrap()[0];
    let cancel = AtomicBool::new(false);
    let diff = repo
        .local_version_diff_with(
            "a.txt",
            &version.id,
            version.base.as_deref(),
            false,
            DiffContext::Compact,
            &cancel,
        )
        .unwrap();
    assert_eq!(diff.len(), 1);
    assert_eq!(diff[0].file.path, "a.txt");
    assert_eq!((diff[0].file.additions, diff[0].file.deletions), (1, 1));
    let added: Vec<&str> = diff[0].hunks[0]
        .lines
        .iter()
        .filter(|l| l.kind == LineKind::Added)
        .map(|l| l.text.as_str())
        .collect();
    assert_eq!(added, ["TWO"]);
    let now_diff = repo
        .local_version_diff_with("a.txt", &version.id, None, true, DiffContext::Compact, &cancel)
        .unwrap();
    assert_eq!((now_diff[0].file.additions, now_diff[0].file.deletions), (1, 0));
    // With the file gone, the version is all there is.
    std::fs::remove_file(fx.path().join("a.txt")).unwrap();
    let gone = repo
        .local_version_diff_with("a.txt", &version.id, None, true, DiffContext::Compact, &cancel)
        .unwrap();
    assert_eq!(gone[0].file.deletions, 3);
}

#[test]
fn restore_puts_the_version_on_disk_stages_nothing_and_undo_takes_it_back() {
    let mut fx = base();
    let repo = Repo::open(fx.path()).unwrap();
    let mut history = LocalHistory::open(&repo);
    fx.write("a.txt", "the good version\n");
    history.keep(&repo, &paths(&["a.txt"]), now() - DAY);
    fx.write("a.txt", "broken\n");
    let id = repo.local_history("a.txt").unwrap()[0].id.clone();
    let action = Action::RestoreLocalVersion {
        path: "a.txt".into(),
        id: id.clone(),
    };
    let shown = repo.plan(&action).unwrap().commands[0].display();
    assert!(shown.starts_with("git restore --source="), "{shown}");
    assert!(shown.contains(" --worktree -- ") && shown.ends_with("a.txt"), "{shown}");
    repo.perform(&action).unwrap();
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "the good version\n"
    );
    assert_eq!(fx.git(&["diff", "--cached", "--name-only"]), "");
    assert_eq!(
        repo.operation_log().unwrap()[0].title,
        "Restore a.txt from Local History"
    );
    let log = repo.operation_log().unwrap();
    repo.perform(&Action::Restore {
        id: log[0].id.clone(),
        after: false,
    })
    .unwrap();
    assert_eq!(std::fs::read_to_string(fx.path().join("a.txt")).unwrap(), "broken\n");
    // A deleted file comes back too.
    std::fs::remove_file(fx.path().join("a.txt")).unwrap();
    repo.perform(&action).unwrap();
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "the good version\n"
    );
}

#[cfg(unix)]
#[test]
fn restore_keeps_a_script_runnable() {
    use std::os::unix::fs::PermissionsExt;
    let fx = base();
    fx.write("run.sh", "echo good\n");
    let script = fx.path().join("run.sh");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let repo = Repo::open(fx.path()).unwrap();
    let mut history = LocalHistory::open(&repo);
    history.keep(&repo, &paths(&["run.sh"]), now() - DAY);
    fx.write("run.sh", "echo bad\n");
    let id = repo.local_history("run.sh").unwrap()[0].id.clone();
    repo.perform(&Action::RestoreLocalVersion {
        path: "run.sh".into(),
        id,
    })
    .unwrap();
    assert_eq!(std::fs::read_to_string(&script).unwrap(), "echo good\n");
    assert_ne!(std::fs::metadata(&script).unwrap().permissions().mode() & 0o111, 0);
}

#[test]
fn save_as_writes_the_version_where_asked() {
    let fx = base();
    let repo = Repo::open(fx.path()).unwrap();
    let mut history = LocalHistory::open(&repo);
    fx.write("a.txt", "as it was\n");
    history.keep(&repo, &paths(&["a.txt"]), now() - DAY);
    let id = repo.local_history("a.txt").unwrap()[0].id.clone();
    let to = fx.path().join("copy.txt");
    repo.save_local_version(&id, &to).unwrap();
    assert_eq!(std::fs::read_to_string(to).unwrap(), "as it was\n");
}

#[test]
fn a_version_no_longer_kept_cant_be_restored() {
    let fx = base();
    let repo = Repo::open(fx.path()).unwrap();
    let action = Action::RestoreLocalVersion {
        path: "a.txt".into(),
        id: "0123456789abcdef0123456789abcdef01234567".into(),
    };
    let err = repo.plan(&action).unwrap_err().to_string();
    assert!(err.contains("no longer has this version"), "{err}");
}

#[test]
fn a_file_saved_while_watched_is_kept_once_for_two_windows() {
    let mut fx = base();
    fx.write(".gitignore", "*.log\n");
    fx.git(&["add", ".gitignore"]);
    fx.git(&["commit", "-q", "-m", "Ignore logs"]);
    let repo = Repo::open(fx.path()).unwrap();
    let first = repo.watch(|_| {}).unwrap();
    let second = repo.watch(|_| {}).unwrap();
    assert!(first.files && second.files);
    fx.write("a.txt", "saved in an editor\n");
    fx.write("build.log", "ignored\n");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut versions = Vec::new();
    while Instant::now() < deadline {
        versions = repo.local_history_all().unwrap();
        if !versions.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // Whatever else the system tells late.
    std::thread::sleep(Duration::from_millis(1500));
    let versions = repo.local_history_all().unwrap_or(versions);
    let names: Vec<&str> = versions.iter().map(|v| v.path.as_str()).collect();
    assert_eq!(names, ["a.txt"]);
}

#[test]
fn a_file_in_folders_is_compared_and_restored_at_its_path() {
    let mut fx = base();
    fx.commit("src/deep/b.rs", "fn b() {}\n", "Add b");
    let repo = Repo::open(fx.path()).unwrap();
    let mut history = LocalHistory::open(&repo);
    fx.write("src/deep/b.rs", "fn b() { 1 }\n");
    history.keep(&repo, &paths(&["src/deep/b.rs"]), now() - DAY);
    let version = repo.local_history("src/deep/b.rs").unwrap()[0].clone();
    let diff = repo
        .local_version_diff_with(
            "src/deep/b.rs",
            &version.id,
            version.base.as_deref(),
            false,
            DiffContext::Compact,
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(diff[0].file.path, "src/deep/b.rs");
    assert_eq!((diff[0].file.additions, diff[0].file.deletions), (1, 1));
    std::fs::remove_dir_all(fx.path().join("src")).unwrap();
    repo.perform(&Action::RestoreLocalVersion {
        path: "src/deep/b.rs".into(),
        id: version.id,
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(fx.path().join("src/deep/b.rs")).unwrap(),
        "fn b() { 1 }\n"
    );
}

/// Audit F04: a blob git has but nothing reaches (staged, then unstaged) goes with
/// `git prune`; the version stays.
#[test]
fn a_version_git_has_a_blob_of_stays_after_git_prunes_it() {
    let mut fx = base();
    fx.write("a.txt", "unique draft\n");
    fx.git(&["add", "a.txt"]);
    fx.git(&["reset", "-q", "HEAD", "--", "a.txt"]);
    let repo = Repo::open(fx.path()).unwrap();
    let mut history = LocalHistory::open(&repo);
    assert_eq!(history.keep(&repo, &paths(&["a.txt"]), now()), 1);
    fx.git(&["prune", "--expire=now"]);
    let repo = Repo::open(fx.path()).unwrap();
    let version = &repo.local_history("a.txt").unwrap()[0];
    assert!(version.kept);
    let to = fx.path().join("saved.txt");
    repo.save_local_version(&version.id, &to).unwrap();
    assert_eq!(std::fs::read_to_string(to).unwrap(), "unique draft\n");
}

/// Audit F05: a version whose line can't be written isn't counted, says why, and is written
/// once it can be, also when the file isn't saved again.
#[test]
fn a_version_that_cant_be_written_waits_and_is_written_when_it_can() {
    let fx = base();
    fx.write("a.txt", "draft\n");
    let repo = Repo::open(fx.path()).unwrap();
    let t = now();
    let list = fx.path().join(format!(".git/oxbow/local-history/{}.jsonl", t / DAY));
    // A folder where the day's list goes: it can't be appended to.
    std::fs::create_dir_all(&list).unwrap();
    let mut history = LocalHistory::open(&repo);
    assert_eq!(history.keep(&repo, &paths(&["a.txt"]), t), 0);
    assert!(repo.local_history("a.txt").unwrap().is_empty());
    let problem = history.problem().unwrap().clone();
    assert_eq!((problem.since, problem.waiting), (t, 1));
    assert!(
        problem.error.starts_with("Couldn’t write the list of versions: "),
        "{}",
        problem.error
    );
    // The same bytes saved again still wait for the one version.
    assert_eq!(history.keep(&repo, &paths(&["a.txt"]), t + 1000), 0);
    assert_eq!(history.problem().unwrap().waiting, 1);

    std::fs::remove_dir(&list).unwrap();
    // Tried again with no save: written as it was then.
    assert_eq!(history.keep(&repo, &[], t + 2000), 1);
    assert!(history.problem().is_none());
    let versions = repo.local_history("a.txt").unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].at, t);
    assert!(versions[0].kept);
    // Written once: the same bytes again are no new version.
    assert_eq!(history.keep(&repo, &paths(&["a.txt"]), t + 3000), 0);
    assert_eq!(repo.local_history("a.txt").unwrap().len(), 1);
}

/// A blob that can't be written keeps nothing of the save: the file is read again when tried
/// again.
#[cfg(unix)]
#[test]
fn a_blob_that_cant_be_written_is_read_again_later() {
    use std::os::unix::fs::PermissionsExt;
    let fx = base();
    fx.write("a.txt", "draft\n");
    let repo = Repo::open(fx.path()).unwrap();
    let objects = fx.path().join(".git/oxbow/local-history/objects");
    std::fs::create_dir_all(&objects).unwrap();
    std::fs::set_permissions(&objects, std::fs::Permissions::from_mode(0o500)).unwrap();
    let mut history = LocalHistory::open(&repo);
    let t = now();
    let kept = history.keep(&repo, &paths(&["a.txt"]), t);
    let problem = history.problem().cloned();
    std::fs::set_permissions(&objects, std::fs::Permissions::from_mode(0o700)).unwrap();
    // Root writes anyway; the rest of the test needs a folder that refuses.
    if kept == 1 {
        return;
    }
    assert_eq!(kept, 0);
    let problem = problem.unwrap();
    assert!(
        problem.error.contains("Couldn’t keep a version of a.txt"),
        "{}",
        problem.error
    );
    assert_eq!(history.keep(&repo, &[], t + 1000), 1);
    assert!(history.problem().is_none());
    assert_eq!(repo.local_history("a.txt").unwrap().len(), 1);
}
