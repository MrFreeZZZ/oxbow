//! Browse Files at This Commit: a commit's folders and files, a file's text there, Save As.

mod support;

use oxbow_core::Repo;
use oxbow_core::browse::{MAX_TEXT, TreeKind};
use support::Fixture;

/// `README.md`, `src/lib.rs` and `src/bin/tool.sh`, then a commit that changes `src/lib.rs`.
fn project() -> (Fixture, String) {
    let mut fx = Fixture::new();
    fx.write("README.md", "# Demo\n");
    fx.write("src/bin/tool.sh", "#!/bin/sh\necho hi\n");
    let first = fx.commit("src/lib.rs", "pub fn one() {}\n", "First");
    fx.commit("src/lib.rs", "pub fn two() {}\n", "Second");
    (fx, first)
}

#[test]
fn a_folder_lists_its_folders_first_then_its_files() {
    let (fx, first) = project();
    let repo = Repo::open(fx.path()).unwrap();
    let top = repo.tree_at(&first, "").unwrap();
    let shown: Vec<(&str, TreeKind)> = top.iter().map(|i| (i.path.as_str(), i.kind)).collect();
    assert_eq!(shown, [("src", TreeKind::Folder), ("README.md", TreeKind::File)]);
    assert_eq!(top[1].size, 7);
    let src = repo.tree_at(&first, "src").unwrap();
    let names: Vec<&str> = src.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(names, ["src/bin", "src/lib.rs"]);
    assert!(
        repo.tree_at(&first, "nothere")
            .unwrap_err()
            .to_string()
            .contains("no folder nothere")
    );
    assert!(repo.tree_at("nothere", "").is_err());
}

#[test]
fn a_file_reads_as_the_commit_has_it() {
    let (fx, first) = project();
    let repo = Repo::open(fx.path()).unwrap();
    let file = repo.file_at(&first, "src/lib.rs").unwrap();
    assert_eq!(file.text.as_deref(), Some("pub fn one() {}\n"));
    assert!(!file.binary && !file.lfs);
    assert_eq!(
        repo.file_at("HEAD", "src/lib.rs").unwrap().text.as_deref(),
        Some("pub fn two() {}\n")
    );
    // A branch or a tag names a commit too.
    assert_eq!(
        repo.file_at("main", "README.md").unwrap().text.as_deref(),
        Some("# Demo\n")
    );
    assert!(
        repo.file_at(&first, "src")
            .unwrap_err()
            .to_string()
            .contains("has no file src")
    );
    assert_eq!(
        repo.files_at(&first).unwrap(),
        ["README.md", "src/bin/tool.sh", "src/lib.rs"]
    );
}

#[test]
fn binary_big_and_lfs_files_show_no_text_or_say_so() {
    let mut fx = Fixture::new();
    std::fs::write(fx.path().join("logo.png"), b"\x89PNG\0\0data").unwrap();
    fx.write(
        "video.mp4",
        "version https://git-lfs.github.com/spec/v1\noid sha256:abc\nsize 12345\n",
    );
    fx.write("big.txt", &"x".repeat(MAX_TEXT as usize + 1));
    let head = fx.commit("a.txt", "a\n", "Files");
    let repo = Repo::open(fx.path()).unwrap();
    let logo = repo.file_at(&head, "logo.png").unwrap();
    assert!(logo.binary && logo.text.is_none());
    assert!(repo.file_at(&head, "video.mp4").unwrap().lfs);
    let big = repo.file_at(&head, "big.txt").unwrap();
    assert_eq!((big.text.is_none(), big.size), (true, MAX_TEXT + 1));
}

#[test]
fn save_as_writes_the_file_of_the_commit() {
    let (fx, first) = project();
    let repo = Repo::open(fx.path()).unwrap();
    let to = fx.path().join("old-lib.rs");
    repo.save_file_at(&first, "src/lib.rs", &to).unwrap();
    assert_eq!(std::fs::read_to_string(&to).unwrap(), "pub fn one() {}\n");
    // The file on disk stays as it is.
    assert_eq!(
        std::fs::read_to_string(fx.path().join("src/lib.rs")).unwrap(),
        "pub fn two() {}\n"
    );
}
