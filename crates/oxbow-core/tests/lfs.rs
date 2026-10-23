mod support;

use std::process::Command;
use std::sync::atomic::AtomicBool;

use oxbow_core::{Action, ActionEvent, Error, PartState, Repo, Side};
use support::Fixture;

/// Whether git-lfs is installed here; the tests that need it skip without it.
fn has_lfs() -> bool {
    Command::new("git")
        .args(["lfs", "version"])
        .output()
        .is_ok_and(|out| out.status.success())
}

/// Bytes that git sees as binary.
fn binary(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

#[test]
fn rules_are_read_without_git_lfs() {
    let mut fx = Fixture::new();
    fx.write(".gitattributes", "*.psd filter=lfs diff=lfs merge=lfs -text\n");
    // A pointer, as a clone without git-lfs has it.
    fx.write(
        "art/cover.psd",
        "version https://git-lfs.github.com/spec/v1\noid sha256:4d7a\nsize 2048\n",
    );
    fx.commit("a.txt", "a\n", "Root");
    fx.write(
        ".gitattributes",
        "*.psd filter=lfs diff=lfs merge=lfs -text\nfixtures/*.bin filter=lfs diff=lfs merge=lfs -text\n",
    );
    let repo = Repo::open(fx.path()).unwrap();
    let status = repo.lfs_status().unwrap();
    assert_eq!(status.patterns.len(), 2);
    let psd = &status.patterns[0];
    assert_eq!(
        (psd.pattern.as_str(), psd.files, psd.size, psd.missing, psd.new),
        ("*.psd", 1, 2048, 1, false)
    );
    let bin = &status.patterns[1];
    assert_eq!((bin.files, bin.new), (0, true));
}

#[test]
fn tracking_turns_a_staged_file_into_a_pointer() {
    if !has_lfs() {
        return;
    }
    let mut fx = Fixture::new();
    fx.git(&["lfs", "install", "--local"]);
    fx.commit("a.txt", "a\n", "Root");
    std::fs::create_dir_all(fx.path().join("assets")).unwrap();
    std::fs::write(fx.path().join("assets/intro.mov"), binary(300_000)).unwrap();
    let repo = Repo::open(fx.path()).unwrap();

    let tree = repo.working_tree().unwrap();
    let file = tree.unstaged.iter().find(|f| f.path == "assets/intro.mov").unwrap();
    assert!(file.binary && file.lfs.is_none());
    assert_eq!(file.new_size, Some(300_000));

    repo.perform(&Action::LfsTrack {
        pattern: "*.mov".into(),
        paths: vec!["assets/intro.mov".into()],
        stage: true,
    })
    .unwrap();
    assert!(
        std::fs::read_to_string(fx.path().join(".gitattributes"))
            .unwrap()
            .contains("*.mov filter=lfs")
    );
    let blob = fx.git(&["cat-file", "-p", ":assets/intro.mov"]);
    assert!(blob.starts_with("version https://git-lfs.github.com/spec/v1"), "{blob}");

    let tree = repo.working_tree().unwrap();
    let file = tree.staged.iter().find(|f| f.path == "assets/intro.mov").unwrap();
    assert!(file.lfs.is_some());
    let diff = repo.working_diff("assets/intro.mov", Side::Staged, false).unwrap();
    let lfs = diff.file.lfs.unwrap();
    assert_eq!(lfs.new.unwrap().size, 300_000);
    assert!(lfs.old.is_none());

    let status = repo.lfs_status().unwrap();
    let mov = &status.patterns[0];
    assert_eq!((mov.files, mov.size, mov.missing, mov.new), (1, 300_000, 0, true));

    fx.git(&["commit", "-q", "-m", "Add intro"]);
    let head = fx.git(&["rev-parse", "HEAD"]);
    let detail = repo.commit_detail(&head).unwrap();
    let file = detail.files.iter().find(|f| f.path == "assets/intro.mov").unwrap();
    assert_eq!(file.new_size, Some(300_000));
    assert!(file.lfs.is_some());

    repo.perform(&Action::LfsUntrack {
        pattern: "*.mov".into(),
    })
    .unwrap();
    assert!(repo.lfs_status().unwrap().patterns.is_empty());
}

/// A repository can commit `.gitattributes` as a symlink to a file that never ends; it is not
/// read through, as git itself doesn't. Nor is a symlink shown as a picture.
#[cfg(unix)]
#[test]
fn symlinks_to_endless_files_are_not_read() {
    let mut fx = Fixture::new();
    std::os::unix::fs::symlink("/dev/zero", fx.path().join(".gitattributes")).unwrap();
    std::os::unix::fs::symlink("/dev/zero", fx.path().join("logo.svg")).unwrap();
    fx.commit("a.txt", "a\n", "Root");
    let repo = Repo::open(fx.path()).unwrap();
    assert!(repo.lfs_status().unwrap().patterns.is_empty());
    let shown = repo
        .read_source(&oxbow_core::Source::Worktree {
            path: "logo.svg".into(),
        })
        .unwrap();
    assert_eq!(shown, b"/dev/zero");
    // Untracked, it is counted as a link too.
    std::os::unix::fs::symlink("/dev/zero", fx.path().join("new.txt")).unwrap();
    assert!(
        repo.working_tree()
            .unwrap()
            .unstaged
            .iter()
            .any(|f| f.path == "new.txt")
    );
}

/// Run git in `dir` the way the fixture does, for a second repository.
fn git_in(dir: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn a_download_brings_each_file_and_names_the_one_that_failed() {
    if !has_lfs() {
        return;
    }
    let mut fx = Fixture::new();
    fx.git(&["lfs", "install", "--local"]);
    fx.git(&["lfs", "track", "*.bin"]);
    for (i, name) in ["a.bin", "b.bin", "dir/c d.bin"].iter().enumerate() {
        let path = fx.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, binary(2000 + i)).unwrap();
    }
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "-q", "-m", "Assets"]);
    let top = fx.path().parent().unwrap().to_owned();
    git_in(&top, &["init", "-q", "--bare", "remote.git"]);
    let remote = format!("file://{}", top.join("remote.git").display());
    fx.git(&["remote", "add", "origin", &remote]);
    fx.git(&["push", "-q", "origin", "main"]);
    // A clone that got only the pointers, and a server that lost b.bin.
    git_in(&top, &["clone", "-q", "-b", "main", "remote.git", "clone"]);
    let clone = top.join("clone");
    git_in(&clone, &["lfs", "install", "--local"]);
    let lost = git_in(&clone, &["lfs", "ls-files", "-l", "-I", "b.bin"]);
    let lost = lost.split_whitespace().next().unwrap().to_owned();
    let store = top
        .join("remote.git/lfs/objects")
        .join(&lost[..2])
        .join(&lost[2..4])
        .join(&lost);
    let kept = top.join("b.object");
    std::fs::rename(&store, &kept).unwrap();

    let repo = Repo::open(&clone).unwrap();
    let missing: Vec<(String, u64)> = repo
        .lfs_status()
        .unwrap()
        .missing_files
        .into_iter()
        .map(|f| (f.path, f.size))
        .collect();
    assert_eq!(
        missing,
        [
            ("a.bin".to_owned(), 2000),
            ("b.bin".to_owned(), 2001),
            ("dir/c d.bin".to_owned(), 2002)
        ]
    );

    let mut events = Vec::new();
    let pull = Action::LfsPull {
        include: None,
        paths: Vec::new(),
    };
    let result = repo.perform_with(&pull, &mut |e| events.push(e), &AtomicBool::new(false));
    match result {
        Err(Error::PartlyDone { of: 3, failed }) => {
            assert_eq!(failed.len(), 1);
            assert_eq!(failed[0].name, "b.bin");
        }
        other => panic!("one file should fail: {other:?}"),
    }
    assert!(events.contains(&ActionEvent::Parts {
        names: vec!["a.bin".into(), "b.bin".into(), "dir/c d.bin".into()]
    }));
    let ended: Vec<(String, PartState)> = events
        .iter()
        .filter_map(|e| match e {
            ActionEvent::Part { name, state, .. } if *state != PartState::Started => Some((name.clone(), *state)),
            _ => None,
        })
        .collect();
    assert_eq!(
        ended,
        [
            ("a.bin".to_owned(), PartState::Done),
            ("b.bin".to_owned(), PartState::Failed),
            ("dir/c d.bin".to_owned(), PartState::Done),
        ]
    );
    assert_eq!(std::fs::read(clone.join("dir/c d.bin")).unwrap(), binary(2002));

    // Once the server has it again, trying that one file again brings it.
    std::fs::rename(&kept, &store).unwrap();
    let again = Action::LfsPull {
        include: None,
        paths: vec!["b.bin".into()],
    };
    let shown = repo.plan(&again).unwrap().commands[0].display();
    assert_eq!(shown, "git lfs pull --include=/b.bin");
    repo.perform(&again).unwrap();
    assert_eq!(std::fs::read(clone.join("b.bin")).unwrap(), binary(2001));
    assert!(repo.lfs_status().unwrap().missing_files.is_empty());
}
