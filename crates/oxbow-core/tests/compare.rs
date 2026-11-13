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

#[test]
fn pages_and_counts_add_up_to_the_whole_diff() {
    use std::sync::atomic::AtomicBool;
    let (fx, _, _, feature) = diverged();
    let repo = Repo::open(fx.path()).unwrap();
    let c = repo.compare("main", "feature", CompareMode::Tips).unwrap();
    // The list comes at once, its counts later.
    assert!(c.files.iter().all(|f| f.file.additions == 0 && f.file.deletions == 0));
    let never = AtomicBool::new(false);
    let counted = repo.tree_files_with(&c.from, &feature, Some(0), &never).unwrap();
    assert_eq!(counted.len(), c.files.len());
    assert!(counted.iter().any(|f| f.additions + f.deletions > 0));
    let skipped = repo.tree_files_with(&c.from, &feature, Some(1), &never).unwrap();
    assert_eq!((skipped[0].additions, skipped[0].deletions), (0, 0));
    assert_eq!(
        skipped[1..].iter().map(|f| f.additions).collect::<Vec<_>>(),
        counted[1..].iter().map(|f| f.additions).collect::<Vec<_>>()
    );

    let whole = repo
        .tree_diff(&c.from, &feature, None, oxbow_core::DiffContext::Compact)
        .unwrap();
    let mut paged = Vec::new();
    let mut at = Some(0);
    while let Some(from) = at {
        // A budget of one line still brings a file per page.
        let page = repo
            .tree_diff_page(&c.from, &feature, from, 1, oxbow_core::DiffContext::Compact, &never)
            .unwrap();
        assert_eq!(page.files, whole.len());
        assert_eq!(page.diffs.len(), 1);
        paged.extend(page.diffs);
        at = page.next;
    }
    assert_eq!(
        serde_json::to_value(&paged).unwrap(),
        serde_json::to_value(&whole).unwrap()
    );
    assert!(matches!(
        repo.tree_diff_page(
            &c.from,
            &feature,
            0,
            1,
            oxbow_core::DiffContext::Compact,
            &AtomicBool::new(true)
        ),
        Err(oxbow_core::Error::Cancelled)
    ));
}

#[test]
fn a_file_with_a_tab_or_a_quote_in_its_name_knows_its_last_commit() {
    let (mut fx, _, _, _) = diverged();
    fx.git(&["switch", "-q", "feature"]);
    let tab = fx.commit("tab\there \"q\".txt", "x\n", "Add an odd name");
    let repo = Repo::open(fx.path()).unwrap();
    let c = repo.compare("main", "feature", CompareMode::Split).unwrap();
    let file = c.files.iter().find(|f| f.file.path.starts_with("tab")).unwrap();
    assert_eq!(file.last.as_deref(), Some(tab.as_str()));
}

#[test]
fn a_file_changed_before_the_newest_500_commits_keeps_its_commit() {
    let mut fx = Fixture::new();
    fx.commit("shared.txt", "one\n", "Base");
    fx.git(&["switch", "-q", "-c", "feature"]);
    let old = fx.commit("shared.txt", "one\nfeature\n", "Extend shared");
    // 501 more commits on top, in one go.
    let mut stream = String::new();
    for i in 0..501 {
        let message = format!("Step {i}");
        stream += &format!(
            "commit refs/heads/feature\ncommitter Alexander <alexander@example.com> {} +0000\ndata {}\n{message}\n",
            1_800_000_000 + i,
            message.len()
        );
        if i == 0 {
            stream += &format!("from {old}\n");
        }
    }
    let mut import = std::process::Command::new("git")
        .args(["fast-import", "--quiet"])
        .current_dir(fx.path())
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut import.stdin.take().unwrap(), stream.as_bytes()).unwrap();
    assert!(import.wait().unwrap().success());
    fx.git(&["checkout", "-q", "-f", "feature"]);
    fx.git(&["switch", "-q", "main"]);
    fx.commit("shared.txt", "one\nmain\n", "Change shared on main");
    let repo = Repo::open(fx.path()).unwrap();
    for mode in [CompareMode::Split, CompareMode::Tips] {
        let c = repo.compare("main", "feature", mode).unwrap();
        assert_eq!(c.ahead_count, 502);
        let shared = c.files.iter().find(|f| f.file.path == "shared.txt").unwrap();
        assert_eq!(shared.last.as_deref(), Some(old.as_str()));
        assert!(!shared.only_base);
    }
}

#[test]
fn qualified_refs_compare_the_selected_branch_remote_or_tag() {
    let (mut fx, base, main, feature) = diverged();
    fx.git(&["branch", "origin/feature", &main]);
    fx.git(&["update-ref", "refs/remotes/origin/feature", &feature]);
    fx.git(&["tag", "-a", "-m", "Tag with the same name", "origin/feature", &base]);
    let repo = Repo::open(fx.path()).unwrap();

    let local = "refs/heads/origin/feature";
    let remote = "refs/remotes/origin/feature";
    let tag = "refs/tags/origin/feature";
    let forward = repo.compare(local, tag, CompareMode::Tips).unwrap();
    assert_eq!((&forward.base_id, &forward.target_id), (&main, &base));
    assert_eq!((forward.ahead_count, forward.behind_count), (0, 1));
    assert_eq!(forward.command, format!("git diff {local}..{tag}"));

    let swapped = repo.compare(tag, local, CompareMode::Split).unwrap();
    assert_eq!((&swapped.base_id, &swapped.target_id), (&base, &main));
    assert_eq!((swapped.ahead_count, swapped.behind_count), (1, 0));
    assert_eq!(swapped.command, format!("git diff {tag}...{local}"));

    let tracked = repo.compare(&main[..7], remote, CompareMode::Tips).unwrap();
    assert_eq!((&tracked.base_id, &tracked.target_id), (&main, &feature));

    // A deleted branch must not fall back to a tag whose name looks like its full ref.
    fx.git(&["tag", local, &base]);
    fx.git(&["branch", "-D", "origin/feature"]);
    assert!(repo.compare(local, tag, CompareMode::Tips).is_err());
}

#[test]
fn bare_hex_compares_only_commit_ids_even_when_refs_match() {
    let (mut fx, base, main, feature) = diverged();
    let prefix = &feature[..9];
    let missing = "cafedecafbadc0de";
    for name in [prefix, missing] {
        fx.git(&["branch", name, &main]);
        fx.git(&["tag", name, &base]);
    }
    let blob = fx.git(&["rev-parse", "HEAD:main.txt"]);
    let repo = Repo::open(fx.path()).unwrap();

    for sha in [prefix, &prefix.to_ascii_uppercase(), &feature] {
        let c = repo.compare(&main, sha, CompareMode::Tips).unwrap();
        assert_eq!(c.target_id, feature);
        assert_eq!(c.command, format!("git diff {main}..{feature}"));
    }
    for (reference, expected) in [("refs/heads", &main), ("refs/tags", &base)] {
        let c = repo
            .compare("HEAD", &format!("{reference}/{missing}"), CompareMode::Tips)
            .unwrap();
        assert_eq!(&c.target_id, expected);
    }
    assert!(repo.compare("HEAD", missing, CompareMode::Tips).is_err());
    assert!(repo.compare("HEAD", &blob, CompareMode::Tips).is_err());
}

#[test]
fn annotated_tag_object_id_compares_its_commit() {
    let (mut fx, _, main, feature) = diverged();
    fx.git(&["tag", "-a", "-m", "Release", "release", &feature]);
    let tag_id = fx.git(&["rev-parse", "refs/tags/release"]);
    assert_ne!(tag_id, feature);
    let repo = Repo::open(fx.path()).unwrap();

    let c = repo.compare(&main, &tag_id, CompareMode::Tips).unwrap();
    assert_eq!(c.target_id, feature);
    assert_eq!(c.command, format!("git diff {main}..{feature}"));
}

#[test]
fn abbreviated_commit_id_ignores_a_colliding_blob() {
    let (mut fx, _, main, feature) = diverged();
    let prefix = &feature[..4];
    // This blob shares the first four hex digits of the fixture's feature tip.
    fx.write("collision.txt", "compare collision 71025\n");
    let blob = fx.git(&["hash-object", "-w", "collision.txt"]);
    assert!(blob.starts_with(prefix));
    let repo = Repo::open(fx.path()).unwrap();

    let c = repo.compare(&main, prefix, CompareMode::Tips).unwrap();
    assert_eq!(c.target_id, feature);
}
