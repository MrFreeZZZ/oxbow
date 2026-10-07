mod support;

use oxbow_core::{CompareMode, Repo};
use support::Fixture;

/// main and feature split at `base`; each has its own commits after that.
fn diverged() -> (Fixture, String, String, String) {
    let mut fx = Fixture::new();
    let base = fx.commit("shared.txt", "one\n", "Base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.commit("feature.txt", "new\n", "Add feature");
    let feature = fx.commit("shared.txt", "one\nfeature\n", "Extend shared");
    fx.git(&["switch", "-q", "main"]);
    let main = fx.commit("main.txt", "fix\n", "Fix on main");
    (fx, base, main, feature)
}

#[test]
fn since_split_shows_only_what_the_branch_changed() {
    let (fx, base, main, feature) = diverged();
    let repo = Repo::open(fx.path()).unwrap();
    let c = repo.compare("main", "feature", CompareMode::Split).unwrap();
    assert_eq!(c.merge_base.as_ref().map(|m| m.id.as_str()), Some(base.as_str()));
    assert_eq!((c.ahead_count, c.behind_count), (2, 1));
    assert_eq!(c.ahead[0].id, feature);
    assert_eq!(c.ahead[0].summary, "Extend shared");
    assert_eq!(c.behind[0].id, main);
    assert_eq!(c.from, base);
    let files: Vec<_> = c.files.iter().map(|f| f.file.path.as_str()).collect();
    assert_eq!(files, ["feature.txt", "shared.txt"]);
    assert_eq!(c.files[1].last.as_deref(), Some(feature.as_str()));
    assert_eq!(c.command, "git diff main...feature");
}

#[test]
fn tip_to_tip_also_shows_the_base_side_undone() {
    let (fx, _, main, feature) = diverged();
    let repo = Repo::open(fx.path()).unwrap();
    let c = repo.compare("main", "feature", CompareMode::Tips).unwrap();
    assert_eq!(c.from, main);
    let files: Vec<_> = c.files.iter().map(|f| (f.file.path.as_str(), f.only_base)).collect();
    assert_eq!(
        files,
        [("feature.txt", false), ("main.txt", true), ("shared.txt", false)]
    );
    assert_eq!(c.command, "git diff main..feature");
    let diffs = repo
        .tree_diff(&c.from, &feature, Some("main.txt"), oxbow_core::DiffContext::Compact)
        .unwrap();
    assert_eq!(diffs[0].file.deletions, 1);
}

#[test]
fn unknown_names_are_errors() {
    let (fx, ..) = diverged();
    let repo = Repo::open(fx.path()).unwrap();
    assert!(repo.compare("main", "--output=x", CompareMode::Split).is_err());
    assert!(repo.compare("main", "nope", CompareMode::Split).is_err());
}
