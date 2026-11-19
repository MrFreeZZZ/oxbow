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
    let action = Action::SavePatches {
        to: "refs/heads/feature".into(),
        since: Some("refs/heads/main".into()),
        count: 0,
        into: into.clone(),
        one_file: false,
    };
    let plan = repo.plan(&action).unwrap();
    assert!(
        plan.commands[0]
            .display()
            .contains("refs/heads/main..refs/heads/feature")
    );
    assert_eq!(
        plan.commands[0].comment.as_deref(),
        Some("the commits feature has that main doesn’t, a numbered .patch file each")
    );
    repo.perform(&action).unwrap();
    let mut files: Vec<String> = std::fs::read_dir(&into)
        .unwrap()
        .map(|e| e.unwrap().path().display().to_string())
        .collect();
    files.sort();
    (dir, files)
}

#[test]
fn commits_are_saved_as_a_patch_file_each_or_all_in_one() {
    let (mut fx, add_b, shout) = diverged();
    fx.git(&["tag", "main", &add_b]);
    fx.git(&["tag", "feature", "HEAD"]);
    let repo = Repo::open(fx.path()).unwrap();
    let (_dir, files) = save_feature(&repo);
    let names: Vec<&str> = files.iter().map(|f| f.rsplit('/').next().unwrap()).collect();
    assert_eq!(names, ["0001-Add-b.patch", "0002-Shout-two.patch"]);
    assert!(
        std::fs::read_to_string(&files[0])
            .unwrap()
            .starts_with(&format!("From {add_b} "))
    );

    let commits = repo
        .patch_range("refs/heads/feature", Some("refs/heads/main"), 0)
        .unwrap();
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
        stages: repo.conflict_file("a.txt").unwrap().stages,
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

#[test]
fn a_saved_file_patch_round_trips_only_the_selected_change() {
    let mut source = Fixture::new();
    const TEXT: &str = "text[1].txt";
    source.write("keep.txt", "before\n");
    source.write("text1.txt", "before\n");
    source.write("old.txt", "rename me\n");
    source.write("delete.txt", "delete me\n");
    source.write("was-file", "this was a file\n");
    std::fs::write(source.path().join(TEXT), b"before \xff\n\ncontext\n").unwrap();
    std::fs::write(source.path().join("binary.dat"), b"old\0\xff").unwrap();
    let root = source.commit("root.txt", "root change\n", "Root with several files");
    source.git(&["mv", "old.txt", "new.txt"]);
    source.git(&["rm", "delete.txt", "was-file"]);
    source.write("was-file/unrelated.txt", "directory now\n");
    source.write("keep.txt", "after\n");
    source.write("text1.txt", "after\n");
    std::fs::write(source.path().join(TEXT), b"after \xff\n\ncontext\n").unwrap();
    std::fs::write(source.path().join("binary.dat"), b"new\0\xfe").unwrap();
    let feature = source.commit("added.txt", "added\n", "Change several files");
    source.git(&["switch", "-qc", "first-parent", &root]);
    source.commit("parent-only.txt", "first parent\n", "Change on the first parent");
    source.git(&["merge", "--no-ff", "--no-edit", &feature]);
    let later = source.git(&["rev-parse", "HEAD"]);
    for (key, value) in [
        ("diff.noprefix", "true"),
        ("diff.mnemonicPrefix", "true"),
        ("diff.relative", "true"),
        ("diff.context", "0"),
        ("diff.external", "false"),
    ] {
        source.git(&["config", key, value]);
    }
    let repo = Repo::open(source.path()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("selected.patch");
    let paths = vec![output.to_str().unwrap().to_owned()];
    let mut empty = Fixture::new();
    empty.git(&["commit", "--allow-empty", "-qm", "Empty"]);
    let empty_repo = Repo::open(empty.path()).unwrap();
    let apply = |target: &Repo, expected: &str| {
        let preview = target.patch_preview(&paths).unwrap();
        assert!(preview.plain && !preview.fails && preview.not_clean.is_empty());
        assert_eq!(preview.files.len(), 1);
        assert_eq!(preview.files[0].path, expected);
        target.perform(&Action::ApplyPatches { paths: paths.clone() }).unwrap();
    };
    repo.save_file_patch(&root, "root.txt", None, &output).unwrap();
    apply(&empty_repo, "root.txt");
    assert_eq!(read(&empty, "root.txt"), "root change\n");
    assert!(!empty.path().join("keep.txt").exists());

    source.git(&["switch", "--detach", &root]);
    for (path, old_path) in [
        (TEXT, None),
        ("new.txt", Some("old.txt")),
        ("delete.txt", None),
        ("added.txt", None),
        ("binary.dat", None),
        ("was-file", None),
    ] {
        repo.save_file_patch(&later, path, old_path, &output).unwrap();
        let patch = std::fs::read(&output).unwrap();
        assert!(patch.starts_with(b"diff --git a/"));
        if old_path.is_some() {
            assert!(patch.ends_with(b"rename from old.txt\nrename to new.txt\n"));
        }
        if path == "binary.dat" {
            assert!(patch.windows(16).any(|bytes| bytes == b"GIT binary patch"));
        }
        apply(&repo, path);
    }
    assert_eq!(
        std::fs::read(source.path().join(TEXT)).unwrap(),
        b"after \xff\n\ncontext\n"
    );
    assert_eq!(std::fs::read(source.path().join("binary.dat")).unwrap(), b"new\0\xfe");
    assert_eq!(read(&source, "new.txt"), "rename me\n");
    assert_eq!(read(&source, "added.txt"), "added\n");
    assert_eq!(read(&source, "keep.txt"), "before\n");
    assert_eq!(read(&source, "text1.txt"), "before\n");
    for path in ["old.txt", "delete.txt", "was-file"] {
        assert!(!source.path().join(path).exists());
    }
    assert_eq!(source.git(&["rev-parse", "HEAD"]), root);
    assert!(source.git(&["diff", "--cached", "--name-only"]).is_empty());

    let saved = std::fs::read(&output).unwrap();
    assert!(repo.save_file_patch(&later, "missing.txt", None, &output).is_err());
    assert_eq!(std::fs::read(&output).unwrap(), saved);
}
