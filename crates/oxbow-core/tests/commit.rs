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
