mod support;

use oxbow_core::{Inner, Outer, PairKind, Repo, RewriteSource, SectionKind};
use support::Fixture;

/// feature had One, Two, Three on `base`. Then main got a commit, and feature was rebuilt on
/// it: One as it was, Two with a new message, Three with different code, and Four added.
fn rebased() -> (Fixture, String, String, String) {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\nb\nc\n", "Base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.commit("one.txt", "one\n", "One");
    fx.commit("two.txt", "two\n", "Two");
    fx.commit("three.txt", "1\n2\n3\n4\n5\nx\ny\nz\n6\n7\n8\n9\n", "Three");
    let old = fx.git(&["rev-parse", "HEAD"]);
    let (one, two, three) = (
        fx.git(&["rev-parse", "HEAD~2"]),
        fx.git(&["rev-parse", "HEAD~1"]),
        old.clone(),
    );
    fx.git(&["switch", "-q", "main"]);
    let main = fx.commit("main.txt", "main\n", "Main moves on");
    fx.git(&["switch", "-q", "-C", "feature"]);
    fx.git(&["cherry-pick", &one]);
    fx.git(&["cherry-pick", &two]);
    fx.git(&["commit", "-q", "--amend", "-m", "Two, better said"]);
    fx.git(&["cherry-pick", "--no-commit", &three]);
    fx.write("three.txt", "1\n2\n3\n4\n5\nx\nY\nz\n6\n7\n8\n9\n");
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "-q", "-m", "Three"]);
    fx.commit("four.txt", "four\n", "Four");
    let new = fx.git(&["rev-parse", "HEAD"]);
    (fx, old, new, main)
}

#[test]
fn a_rebase_pairs_each_commit_with_its_new_version() {
    let (fx, old, new, main) = rebased();
    let repo = Repo::open(fx.path()).unwrap();
    let s = repo.series(&old, &new, &["refs/heads/feature".to_owned()]).unwrap();
    // Main's commit is the new base, not an added commit.
    assert_eq!(s.new_base.as_deref(), Some(main.as_str()));
    assert_eq!(s.base_name.as_deref(), Some("main"));
    assert_eq!(s.base_commits, 1);
    assert_eq!((s.old_count, s.new_count), (3, 4));

    // Newest first.
    let shown: Vec<_> = s
        .pairs
        .iter()
        .map(|p| {
            let summary = p.new.as_ref().or(p.old.as_ref()).unwrap().summary.clone();
            (summary, p.kind, p.message, p.code)
        })
        .collect();
    assert_eq!(
        shown,
        [
            ("Four".to_owned(), PairKind::Added, false, false),
            ("Three".to_owned(), PairKind::Changed, false, true),
            ("Two, better said".to_owned(), PairKind::Changed, true, false),
            ("One".to_owned(), PairKind::Same, false, false),
        ]
    );
    let three = &s.pairs[1];
    let file = three
        .sections
        .iter()
        .find(|s| s.kind == SectionKind::File && s.changed)
        .unwrap();
    assert_eq!(file.title, "three.txt (new)");
    let changed: Vec<_> = file
        .lines
        .iter()
        .filter(|l| l.outer != Outer::Both)
        .map(|l| (l.outer, l.inner, l.text.as_str()))
        .collect();
    assert_eq!(
        changed,
        [(Outer::Old, Inner::Added, "y"), (Outer::New, Inner::Added, "Y")]
    );

    let two = &s.pairs[2];
    let message = two.sections.iter().find(|s| s.kind == SectionKind::Message).unwrap();
    assert!(message.lines.iter().any(|l| l.outer == Outer::Old && l.text == "Two"));
    assert!(
        message
            .lines
            .iter()
            .any(|l| l.outer == Outer::New && l.text == "Two, better said")
    );
    assert!(s.command.starts_with("git range-diff "));
}

#[test]
fn a_dropped_commit_stays_where_it_was() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.commit("one.txt", "one\n", "One");
    let two = fx.commit("two.txt", "two\n", "Two");
    fx.commit("three.txt", "three\n", "Three");
    let old = fx.git(&["rev-parse", "HEAD"]);
    // Drop Two: rebuild on One.
    fx.git(&["reset", "-q", "--hard", &format!("{two}~1")]);
    fx.git(&["cherry-pick", &old]);
    let new = fx.git(&["rev-parse", "HEAD"]);
    let repo = Repo::open(fx.path()).unwrap();
    let s = repo.series(&old, &new, &[]).unwrap();
    let kinds: Vec<_> = s.pairs.iter().map(|p| p.kind).collect();
    assert_eq!(kinds, [PairKind::Same, PairKind::Dropped]);
    assert_eq!(s.pairs[1].old.as_ref().unwrap().id, two);
    // One was not touched, so neither side has it, and the base stayed.
    assert_eq!(s.old_base, s.new_base);
    assert_eq!(s.base_name, None);
}

#[test]
fn a_stack_compares_whole_or_by_branch() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Base");
    fx.git(&["switch", "-q", "-c", "lower"]);
    fx.commit("l.txt", "l\n", "Lower");
    fx.git(&["switch", "-q", "-c", "upper"]);
    fx.commit("u.txt", "u\n", "Upper");
    let (old_lower, old_upper) = (fx.git(&["rev-parse", "lower"]), fx.git(&["rev-parse", "upper"]));
    // Reword Lower; both branches move.
    fx.git(&["switch", "-q", "lower"]);
    fx.git(&["commit", "-q", "--amend", "-m", "Lower, reworded"]);
    fx.git(&["switch", "-q", "upper"]);
    fx.git(&["rebase", "-q", "--onto", "lower", &old_lower, "upper"]);
    let (new_lower, new_upper) = (fx.git(&["rev-parse", "lower"]), fx.git(&["rev-parse", "upper"]));
    let moved = ["refs/heads/lower".to_owned(), "refs/heads/upper".to_owned()];
    let repo = Repo::open(fx.path()).unwrap();

    let whole = repo.series(&old_upper, &new_upper, &moved).unwrap();
    assert_eq!(whole.pairs.len(), 2);
    assert!(
        whole
            .pairs
            .iter()
            .all(|p| p.kind != PairKind::Added && p.kind != PairKind::Dropped)
    );

    let lower = repo.series(&old_lower, &new_lower, &moved).unwrap();
    assert_eq!(lower.pairs.len(), 1);
    assert!(lower.pairs[0].message);

    // Without knowing lower moved too, its new version is upper's base, and its old one looks
    // dropped.
    let alone = repo.series(&old_upper, &new_upper, &[]).unwrap();
    let kinds: Vec<_> = alone.pairs.iter().map(|p| p.kind).collect();
    assert_eq!(kinds, [PairKind::Same, PairKind::Dropped]);
    assert_eq!(alone.base_name.as_deref(), Some("lower"));
}

#[test]
fn the_same_commits_are_not_a_rewrite() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Base");
    let tip = fx.commit("b.txt", "b\n", "Next");
    let repo = Repo::open(fx.path()).unwrap();
    assert!(repo.series(&tip, &tip, &[]).is_err());
    assert!(repo.series("--output=x", &tip, &[]).is_err());
}

#[test]
fn the_latest_rewrite_comes_from_the_remote_branch() {
    let mut fx = Fixture::new();
    let remote = tempfile::tempdir().unwrap();
    let bare = remote.path().join("origin.git");
    fx.git(&["init", "-q", "--bare", bare.to_str().unwrap()]);
    fx.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
    fx.commit("a.txt", "a\n", "Base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    let first = fx.commit("b.txt", "b\n", "Feature");
    fx.git(&["push", "-q", "-u", "origin", "feature"]);
    let repo = Repo::open(fx.path()).unwrap();
    assert_eq!(repo.last_rewrite("feature").unwrap(), None);

    fx.git(&["commit", "-q", "--amend", "-m", "Feature, reworded"]);
    let second = fx.git(&["rev-parse", "HEAD"]);
    let r = repo.last_rewrite("feature").unwrap().unwrap();
    assert_eq!((r.old.as_str(), r.new.as_str()), (first.as_str(), second.as_str()));
    assert_eq!(r.source, RewriteSource::NotPushed);
    assert_eq!(r.rewritten, ["refs/heads/feature", "refs/remotes/origin/feature"]);

    fx.git(&["push", "-q", "--force", "origin", "feature"]);
    let r = repo.last_rewrite("feature").unwrap().unwrap();
    assert_eq!(r.source, RewriteSource::Pushed);
    assert_eq!((r.old.as_str(), r.new.as_str()), (first.as_str(), second.as_str()));
    assert!(r.time.is_some());
    let s = repo.series(&r.old, &r.new, &r.rewritten).unwrap();
    assert!(s.pairs[0].message);

    // A later push forward keeps the rewrite before it.
    fx.commit("c.txt", "c\n", "More");
    fx.git(&["push", "-q", "origin", "feature"]);
    assert_eq!(repo.last_rewrite("feature").unwrap().unwrap().new, second);
    assert_eq!(repo.last_rewrite("main").unwrap(), None);
}

#[test]
fn a_tag_inside_the_old_series_does_not_cut_it_short() {
    for annotated in [false, true] {
        let (mut fx, old, new, _) = rebased();
        let one = fx.git(&["rev-parse", &format!("{old}~2")]);
        let repo = Repo::open(fx.path()).unwrap();
        let without = repo.series(&old, &new, &["refs/heads/feature".to_owned()]).unwrap();
        if annotated {
            fx.git(&["tag", "-a", "-m", "Checkpoint", "checkpoint", &one]);
        } else {
            fx.git(&["tag", "checkpoint", &one]);
        }
        let with = repo.series(&old, &new, &["refs/heads/feature".to_owned()]).unwrap();
        assert_eq!((with.old_count, with.new_count), (3, 4), "annotated: {annotated}");
        let kinds = |s: &oxbow_core::Series| s.pairs.iter().map(|p| p.kind).collect::<Vec<_>>();
        assert_eq!(kinds(&with), kinds(&without));
    }
}
