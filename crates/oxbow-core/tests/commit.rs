mod support;

use std::sync::atomic::AtomicBool;

use oxbow_core::{DiffContext, Error, FileStatus, LineKind, Repo};
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

#[test]
fn working_diff_over_the_limit_reads_no_patch() {
    let mut fx = Fixture::new();
    fx.commit("data.txt", "start\n", "Root");
    let long: String = (0..30).map(|i| format!("line {i}\n")).collect();
    fx.write("data.txt", &long);

    let mut repo = Repo::open(fx.path()).unwrap();
    repo.set_diff_options(oxbow_core::DiffOptions {
        max_lines: 10,
        ..repo.diff_options()
    });
    let diff = repo
        .working_diff("data.txt", oxbow_core::Side::Unstaged, false)
        .unwrap();
    assert!(diff.limited && !diff.too_large && diff.hunks.is_empty());
    assert_eq!((diff.file.additions, diff.file.deletions), (30, 1));
    assert!(matches!(diff.old, Some(oxbow_core::Source::Blob { .. })));
    assert!(matches!(diff.new, Some(oxbow_core::Source::Worktree { .. })));

    // Staged, both sides are blobs, read from `git diff --raw`.
    fx.git(&["add", "data.txt"]);
    let staged = repo.working_diff("data.txt", oxbow_core::Side::Staged, false).unwrap();
    assert!(staged.limited);
    let (Some(oxbow_core::Source::Blob { id: old }), Some(oxbow_core::Source::Blob { id: new })) =
        (&staged.old, &staged.new)
    else {
        panic!("no blob sides");
    };
    assert_eq!(repo.read_source(&staged.new.clone().unwrap()).unwrap(), long.as_bytes());
    assert_ne!(old, new);
    assert_eq!(repo.source_size(&staged.new.clone().unwrap()), Some(long.len() as u64));
    // A preview reads a side only up to its size limit.
    let side = staged.new.clone().unwrap();
    assert!(repo.read_source_within(&side, long.len() as u64 - 1).is_err());
    assert_eq!(
        repo.read_source_within(&side, long.len() as u64).unwrap(),
        long.as_bytes()
    );
}

#[test]
fn a_blob_too_big_to_diff_still_shows_whether_it_is_binary() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "1\n", "Root");
    let big = 17 * 1024 * 1024;
    fx.write("large.bmp", &format!("BM\0{}", "x".repeat(big)));
    let id = fx.commit("big.txt", &"line\n".repeat(big / 5 + 1), "Big files");
    let repo = Repo::open(fx.path()).unwrap();

    let diffs = repo.commit_diff(&id, None, DiffContext::Compact).unwrap();
    let diff = |p: &str| diffs.iter().find(|d| d.file.path == p).unwrap();
    // A picture keeps its preview: binary, not "too large to diff".
    let picture = diff("large.bmp");
    assert!(picture.file.binary && !picture.too_large);
    assert_eq!(picture.file.new_size, Some(big as u64 + 3));
    assert!(picture.new.is_some());
    let text = diff("big.txt");
    assert!(!text.file.binary && text.too_large);
}

#[test]
fn where_a_shallow_clone_stops_the_commit_waits_for_its_parent() {
    let mut origin = Fixture::new();
    for n in 1..=4 {
        origin.commit("a.txt", &format!("{n}\n"), &format!("Commit {n}"));
    }
    let mut clone = Fixture::new();
    let url = format!("file://{}", origin.path().display());
    let dir = clone.path().join("shallow");
    let dir = dir.to_str().unwrap();
    clone.git(&["clone", "-q", "--depth=2", &url, dir]);
    let end = clone.git(&["-C", dir, "rev-parse", "HEAD~1"]);
    let repo = Repo::open(dir).unwrap();

    let detail = repo.commit_detail(&end).unwrap();
    assert!(detail.shallow_end);
    assert_eq!(detail.parents.len(), 1);
    assert!(detail.files.is_empty());
    assert!(repo.commit_diff(&end, None, DiffContext::Compact).unwrap().is_empty());

    clone.git(&["-C", dir, "fetch", "-q", "--deepen=1", "origin"]);
    let detail = repo.commit_detail(&end).unwrap();
    assert!(!detail.shallow_end);
    assert_eq!(detail.files.len(), 1);
    assert_eq!(repo.commit_diff(&end, None, DiffContext::Compact).unwrap().len(), 1);
}

#[test]
fn a_big_commit_lists_its_files_at_once_and_diffs_them_a_page_at_a_time() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    for n in 0..6 {
        fx.write(&format!("f{n}.txt"), &"line\n".repeat(10));
    }
    let id = fx.commit("a.txt", "b\n", "Many files");
    let repo = Repo::open(fx.path()).unwrap();
    let go = AtomicBool::new(false);

    // The list alone: paths and how each changed, no counts.
    let quick = repo.commit_detail_with(&id, None, &go).unwrap();
    assert!(!quick.counted);
    assert_eq!(quick.files.len(), 7);
    assert!(quick.files.iter().all(|f| f.additions == 0));
    let counted = repo.commit_detail(&id).unwrap();
    assert!(counted.counted);
    assert_eq!(counted.files[1].additions, 10);
    // Files whose diffs are shown already are not counted again.
    let rest = repo.commit_detail_with(&id, Some(3), &go).unwrap();
    assert_eq!(rest.counted_from, 3);
    assert_eq!(rest.files[1].additions, 0);
    assert_eq!(rest.files[4].additions, 10);

    // Pages of about 25 lines: the files come in order, each once.
    let mut paths = Vec::new();
    let mut from = Some(0);
    let mut pages = 0;
    while let Some(at) = from {
        let page = repo.commit_diff_page(&id, at, 25, DiffContext::Compact, &go).unwrap();
        assert_eq!(page.files, 7);
        paths.extend(page.diffs.iter().map(|d| d.file.path.clone()));
        from = page.next;
        pages += 1;
    }
    let all: Vec<_> = counted.files.iter().map(|f| f.path.clone()).collect();
    assert_eq!(paths, all);
    assert!(pages > 1);

    // A newer question stopped it.
    let stop = AtomicBool::new(true);
    assert!(matches!(
        repo.commit_diff_page(&id, 0, 25, DiffContext::Compact, &stop),
        Err(Error::Cancelled)
    ));
    assert!(matches!(
        repo.commit_detail_with(&id, Some(0), &stop),
        Err(Error::Cancelled)
    ));
    // So does a file's diff of its own.
    assert!(matches!(
        repo.commit_diff_with(&id, Some("f0.txt"), DiffContext::Compact, &stop),
        Err(Error::Cancelled)
    ));
    assert_eq!(
        repo.commit_diff_with(&id, Some("f0.txt"), DiffContext::Compact, &go)
            .unwrap()[0]
            .file
            .additions,
        10
    );
}
