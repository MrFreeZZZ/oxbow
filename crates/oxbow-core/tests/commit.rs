mod support;

use oxbow_core::{DiffContext, FileStatus, LineKind, Repo};
use support::Fixture;

#[test]
fn commit_detail_lists_changed_files_with_counts() {
    let mut fx = Fixture::new();
    fx.write("keep.txt", "same\n");
    fx.write("old_name.txt", "one\ntwo\nthree\nfour\nfive\n");
    fx.commit("src/app.rs", "fn main() {\n    let timeout = 10;\n}\n", "Root");
    fx.git(&["mv", "old_name.txt", "new_name.txt"]);
    fx.write("src/app.rs", "fn main() {\n    let timeout = 30;\n}\n");
    fx.write("logo.png", "\u{0}\u{1}PNG");
    let id = fx.commit("README.md", "# Oxbow\n", "Raise timeout\n\nLogin needs more time.");

    let repo = Repo::open(fx.path()).unwrap();
    let detail = repo.commit_detail(&id).unwrap();
    assert_eq!(detail.summary, "Raise timeout");
    assert_eq!(detail.body, "Login needs more time.");
    assert_eq!(detail.author.name, "Alexander");
    assert_eq!(detail.parents.len(), 1);

    let files: Vec<_> = detail
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.status, f.additions, f.deletions, f.binary))
        .collect();
    assert_eq!(
        files,
        [
            ("README.md", FileStatus::Added, 1, 0, false),
            ("logo.png", FileStatus::Added, 0, 0, true),
            ("new_name.txt", FileStatus::Renamed, 0, 0, false),
            ("src/app.rs", FileStatus::Modified, 1, 1, false),
        ]
    );
    assert_eq!(detail.files[2].old_path.as_deref(), Some("old_name.txt"));

    let diffs = repo.commit_diff(&id, Some("src/app.rs"), DiffContext::Compact).unwrap();
    assert_eq!(diffs.len(), 1);
    let lines = &diffs[0].hunks[0].lines;
    let kinds: Vec<_> = lines.iter().map(|l| l.kind).collect();
    assert_eq!(
        kinds,
        [LineKind::Context, LineKind::Removed, LineKind::Added, LineKind::Context]
    );
    let changed: Vec<_> = lines[2]
        .words
        .as_ref()
        .unwrap()
        .iter()
        .filter(|w| w.changed)
        .map(|w| w.text.as_str())
        .collect();
    assert_eq!(changed, ["30"]);

    let all = repo.commit_diff(&id, None, DiffContext::Compact).unwrap();
    assert_eq!(all.len(), 4);
}

#[test]
fn root_commit_diffs_against_nothing() {
    let mut fx = Fixture::new();
    let id = fx.commit("a.txt", "a\nb\n", "Root");
    let repo = Repo::open(fx.path()).unwrap();
    let detail = repo.commit_detail(&id).unwrap();
    assert!(detail.parents.is_empty());
    assert_eq!(detail.files[0].additions, 2);
}

#[test]
fn unknown_commit_is_an_error() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    let repo = Repo::open(fx.path()).unwrap();
    assert!(repo.commit_detail("0123456789012345678901234567890123456789").is_err());
    assert!(repo.commit_detail("not-a-sha").is_err());
}

#[test]
fn unusual_changes_are_described() {
    let mut fx = Fixture::new();
    // `git add -A` keeps the mode set with `update-index` instead of reading it from the disk.
    fx.git(&["config", "core.filemode", "false"]);
    let body: String = (1..=20).map(|i| format!("line {i}\n")).collect();
    fx.write("retry.rs", &body);
    fx.write("release.sh", "echo hi\n");
    fx.write("windows.rs", "a\r\nb\r\n");
    fx.write("font.woff2", "\u{0}old");
    fx.commit("big.json", "", "Root");
    fx.git(&["mv", "retry.rs", "backoff.rs"]);
    fx.write("backoff.rs", &body.replace("line 3\n", "line three\n"));
    fx.git(&["update-index", "--chmod=+x", "release.sh"]);
    fx.write("windows.rs", "a\nb\n");
    fx.write("font.woff2", "\u{0}newer");
    let big: String = (1..=30).map(|i| format!("{i}\n")).collect();
    let id = fx.commit("big.json", &big, "Unusual");

    let repo = Repo::open(fx.path()).unwrap();
    let detail = repo.commit_detail(&id).unwrap();
    let file = |p: &str| detail.files.iter().find(|f| f.path == p).unwrap().clone();

    let renamed = file("backoff.rs");
    assert_eq!(renamed.status, FileStatus::Renamed);
    assert_eq!(renamed.similarity, Some(95));

    let mode = file("release.sh").mode.unwrap();
    assert_eq!((mode.old.as_str(), mode.new.as_str()), ("100644", "100755"));

    let eol = file("windows.rs").eol.unwrap();
    assert_eq!((eol.from.as_str(), eol.to.as_str(), eol.lines), ("CRLF", "LF", 2));
    assert!(file("backoff.rs").eol.is_none());

    let font = file("font.woff2");
    assert!(font.binary);
    assert_eq!((font.old_size, font.new_size), (Some(4), Some(6)));

    let diffs = repo.commit_diff(&id, Some("font.woff2"), DiffContext::Compact).unwrap();
    let Some(oxbow_core::Source::Blob { id: blob }) = &diffs[0].new else {
        panic!("no new side")
    };
    assert_eq!(repo.read_source(&diffs[0].new.clone().unwrap()).unwrap(), b"\0newer");
    assert_eq!(blob.len(), 40);

    let mut limited = repo.clone();
    limited.set_diff_options(oxbow_core::DiffOptions {
        max_lines: 10,
        ..repo.diff_options()
    });
    let big = &limited
        .commit_diff(&id, Some("big.json"), DiffContext::Compact)
        .unwrap()[0];
    assert!(big.limited && big.hunks.is_empty());
    assert_eq!(big.file.additions, 30);
    let small = &limited
        .commit_diff(&id, Some("backoff.rs"), DiffContext::Compact)
        .unwrap()[0];
    assert!(!small.limited && !small.hunks.is_empty());
}

#[test]
fn working_copy_reports_mode_and_line_endings() {
    let mut fx = Fixture::new();
    fx.write("release.sh", "echo hi\n");
    fx.commit("windows.rs", "a\r\nb\r\n", "Root");
    fx.write("windows.rs", "a\nb\n");
    fx.git(&["update-index", "--chmod=+x", "release.sh"]);

    let repo = Repo::open(fx.path()).unwrap();
    let eol = repo
        .working_diff("windows.rs", oxbow_core::Side::Unstaged, false)
        .unwrap();
    assert_eq!(eol.file.eol.as_ref().unwrap().lines, 2);
    assert!(eol.hunks[0].lines[0].cr);
    assert!(matches!(eol.new, Some(oxbow_core::Source::Worktree { .. })));
    let mode = repo
        .working_diff("release.sh", oxbow_core::Side::Staged, false)
        .unwrap();
    assert_eq!(mode.file.mode.unwrap().new, "100755");
}
