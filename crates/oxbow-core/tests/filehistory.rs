mod support;

use oxbow_core::{FileStatus, Repo};
use support::Fixture;

#[test]
fn history_follows_renames_and_blame_names_each_line() {
    let mut fx = Fixture::new();
    let first = fx.commit("src/settings.rs", "one\ntwo\nthree\n", "Add settings");
    fx.git(&["mv", "src/settings.rs", "src/config.rs"]);
    let renamed = fx.commit("src/config.rs", "one\ntwo\nthree\n", "Rename settings to config");
    let edit = fx.commit("src/config.rs", "one\n2\nthree\nfour\n", "Edit config");
    fx.git(&["switch", "-q", "-c", "topic", &renamed]);
    let side = fx.commit("src/config.rs", "zero\none\ntwo\nthree\n", "Side change");
    fx.git(&["switch", "-q", "main"]);

    let repo = Repo::open(fx.path()).unwrap();
    let h = repo.file_history("src/config.rs", None).unwrap();
    let ids: Vec<_> = h.commits.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, [edit.as_str(), renamed.as_str(), first.as_str()]);
    assert_eq!((h.commits[0].additions, h.commits[0].deletions), (2, 1));
    assert_eq!(h.commits[1].status, FileStatus::Renamed);
    assert_eq!(h.commits[1].old_path.as_deref(), Some("src/settings.rs"));
    assert_eq!(h.commits[2].path, "src/settings.rs");
    assert_eq!(
        h.elsewhere.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        [side.as_str()]
    );

    let blame = repo.blame("src/config.rs", "HEAD").unwrap();
    let owners: Vec<_> = blame
        .lines
        .iter()
        .map(|l| (blame.commits[l.commit].id.as_str(), l.text.as_str()))
        .collect();
    assert_eq!(
        owners,
        [
            (first.as_str(), "one"),
            (edit.as_str(), "2"),
            (first.as_str(), "three"),
            (edit.as_str(), "four")
        ]
    );
    assert_eq!(
        blame.commits.iter().find(|c| c.id == first).unwrap().path,
        "src/settings.rs"
    );

    // Blame Before: at the parent of the edit, every line is older.
    let before = repo.blame("src/config.rs", &renamed).unwrap();
    assert!(before.lines.iter().all(|l| before.commits[l.commit].id == first));

    assert_eq!(
        repo.line_history("src/config.rs", 2, "HEAD").unwrap(),
        [edit.clone(), first.clone()]
    );

    // Quick Open lists HEAD's files.
    assert_eq!(repo.files().unwrap(), ["src/config.rs"]);

    // Find in file: the commits that added or removed a text, here and on other branches.
    assert_eq!(
        repo.file_pickaxe("src/config.rs", "TWO", false, "HEAD").unwrap(),
        [edit.clone(), first.clone()]
    );
    assert!(
        repo.file_pickaxe("src/config.rs", "TWO", true, "HEAD")
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        repo.file_pickaxe("src/config.rs", "zero", true, "HEAD").unwrap(),
        std::slice::from_ref(&side)
    );
}
