mod support;

use oxbow_core::{Action, ConflictSide, OperationKind, Repo};
use support::Fixture;

fn read(fx: &Fixture, file: &str) -> String {
    std::fs::read_to_string(fx.path().join(file)).unwrap()
}

/// `feature` adds `b.txt`, then changes `a.txt`; `main` changes `a.txt` the other way.
fn diverged() -> (Fixture, String, String) {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\ntwo\nthree\n", "Root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    let add_b = fx.commit("b.txt", "b\n", "Add b");
    let shout = fx.commit("a.txt", "one\nTWO on feature\nthree\n", "Shout two");
    fx.git(&["switch", "-q", "main"]);
    fx.commit("a.txt", "one\n2 on main\nthree\n", "Number two");
    (fx, add_b, shout)
}

/// The patches of `feature`'s own commits, a file each, in a folder of their own.
fn save_feature(repo: &Repo) -> (tempfile::TempDir, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let into = dir.path().join("patches").display().to_string();
    repo.perform(&Action::SavePatches {
        to: "feature".into(),
        since: Some("main".into()),
        count: 0,
        into: into.clone(),
        one_file: false,
    })
    .unwrap();
    let mut files: Vec<String> = std::fs::read_dir(&into)
        .unwrap()
        .map(|e| e.unwrap().path().display().to_string())
        .collect();
    files.sort();
    (dir, files)
}

#[test]
fn commits_are_saved_as_a_patch_file_each_or_all_in_one() {
    let (fx, add_b, shout) = diverged();
    let repo = Repo::open(fx.path()).unwrap();
    let (_dir, files) = save_feature(&repo);
    let names: Vec<&str> = files.iter().map(|f| f.rsplit('/').next().unwrap()).collect();
    assert_eq!(names, ["0001-Add-b.patch", "0002-Shout-two.patch"]);
    assert!(
        std::fs::read_to_string(&files[0])
            .unwrap()
            .starts_with(&format!("From {add_b} "))
    );

    let commits = repo.patch_range("feature", Some("main"), 0).unwrap();
    let summaries: Vec<&str> = commits.iter().map(|c| c.summary.as_str()).collect();
    assert_eq!(summaries, ["Add b", "Shout two"]);
    assert_eq!(repo.patch_range(&shout, None, 1).unwrap().len(), 1);

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("feature.patch").display().to_string();
    let action = Action::SavePatches {
        to: shout.clone(),
        since: None,
        count: 2,
        into: file.clone(),
        one_file: true,
    };
    assert_eq!(
        repo.plan(&action).unwrap().commands[0].display(),
        format!("git format-patch -2 {shout} --output={file}")
    );
    repo.perform(&action).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(text.matches("\nSubject: [PATCH ").count(), 2);
    // Saving changes nothing that Undo would take back.
    assert!(repo.operation_log().unwrap().is_empty());
}

#[test]
fn a_preview_lists_the_commits_and_files_and_whether_they_fit() {
    let (mut fx, add_b, _) = diverged();
    let repo = Repo::open(fx.path()).unwrap();
    let (_dir, files) = save_feature(&repo);

    // The first patch alone fits main as it is.
    let preview = repo.patch_preview(&files[..1]).unwrap();
    assert!(!preview.plain);
    assert_eq!(preview.commits.len(), 1);
    assert_eq!(preview.commits[0].summary, "Add b");
    assert_eq!(preview.commits[0].author_name, "Alexander");
    assert_eq!(preview.commits[0].id, add_b);
    assert_eq!(preview.files.len(), 1);
    assert_eq!(preview.files[0].path, "b.txt");
    assert_eq!(preview.files[0].added, Some(1));
    assert!(preview.not_clean.is_empty() && !preview.fails);
    assert!(preview.in_the_way.is_empty());

    // The second changes the line main changed too.
    let preview = repo.patch_preview(&files).unwrap();
    let paths: Vec<&str> = preview.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["b.txt", "a.txt"]);
    assert_eq!(preview.not_clean, ["a.txt"]);

    // git am won't start over staged changes, or over an edit to a file a patch changes.
    fx.write("c.txt", "staged\n");
    fx.git(&["add", "c.txt"]);
    fx.write("a.txt", "edited\n");
    fx.write("b.txt", "already here\n");
    let mut in_the_way = repo.patch_preview(&files).unwrap().in_the_way;
    in_the_way.sort();
    assert_eq!(in_the_way, ["a.txt", "b.txt", "c.txt"]);
}

#[test]
fn patches_that_conflict_stop_and_are_resolved_and_continued() {
    let (mut fx, _, shout) = diverged();
    let repo = Repo::open(fx.path()).unwrap();
    let (_dir, files) = save_feature(&repo);
    let action = Action::ApplyPatches { paths: files.clone() };
    assert!(
        repo.plan(&action).unwrap().commands[0]
            .display()
            .starts_with("git am --3way -- ")
    );
    repo.perform(&action).unwrap_err();

    let op = repo.operation().unwrap().expect("git am in progress");
    assert_eq!(op.kind, OperationKind::Am);
    assert_eq!(op.branch.as_deref(), Some("main"));
    assert_eq!(op.step, Some((2, 2)));
    assert_eq!(op.conflicted, 1);
    let commit = op.commit.unwrap();
    assert_eq!(commit.summary, "Shout two");
    assert_eq!(commit.id, shout);
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]), "Add b");

    repo.perform(&Action::TakeFile {
        path: "a.txt".into(),
        side: ConflictSide::Theirs,
    })
    .unwrap();
    repo.perform(&Action::Continue { message: None }).unwrap();
    assert!(repo.operation().unwrap().is_none());
    assert_eq!(read(&fx, "a.txt"), "one\nTWO on feature\nthree\n");
    assert_eq!(fx.git(&["log", "-1", "--format=%s %an"]), "Shout two Alexander");
}

#[test]
fn a_patch_is_skipped_or_all_of_them_aborted() {
    let (mut fx, _, _) = diverged();
    let repo = Repo::open(fx.path()).unwrap();
    let (_dir, files) = save_feature(&repo);
    let main = fx.git(&["rev-parse", "main"]);

    repo.perform(&Action::ApplyPatches { paths: files.clone() })
        .unwrap_err();
    repo.perform(&Action::Skip).unwrap();
    assert!(repo.operation().unwrap().is_none());
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]), "Add b");
    assert_eq!(read(&fx, "a.txt"), "one\n2 on main\nthree\n");

    fx.git(&["reset", "-q", "--hard", &main]);
    repo.perform(&Action::ApplyPatches { paths: files }).unwrap_err();
    repo.perform(&Action::Abort).unwrap();
    assert!(repo.operation().unwrap().is_none());
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), main);
}

#[test]
fn a_plain_diff_goes_into_the_files_uncommitted() {
    let (mut fx, _, _) = diverged();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("change.diff");
    fx.write("a.txt", "one\n2 on main\nthree\nfour\n");
    std::fs::write(&file, fx.git(&["diff"]) + "\n").unwrap();
    fx.git(&["checkout", "--", "a.txt"]);
    let repo = Repo::open(fx.path()).unwrap();
    let paths = vec![file.display().to_string()];

    let preview = repo.patch_preview(&paths).unwrap();
    assert!(preview.plain);
    assert!(preview.commits.is_empty());
    assert_eq!(preview.files[0].path, "a.txt");
    assert!(preview.not_clean.is_empty());

    let head = fx.git(&["rev-parse", "HEAD"]);
    repo.perform(&Action::ApplyPatches { paths: paths.clone() }).unwrap();
    assert_eq!(read(&fx, "a.txt"), "one\n2 on main\nthree\nfour\n");
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), head);

    // Once it is in, the same diff no longer fits the files.
    assert_eq!(repo.patch_preview(&paths).unwrap().not_clean, ["a.txt"]);
}
