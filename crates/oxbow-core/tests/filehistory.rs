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

#[test]
fn a_huge_blame_stops_at_its_last_whole_line_and_reads_stop() {
    use std::sync::atomic::AtomicBool;
    let mut fx = Fixture::new();
    // About 12 MiB of porcelain: more than blame reads.
    let text: String = (0..150_000)
        .map(|i| format!("line {i:06} of a file far too long to blame\n"))
        .collect();
    fx.commit("big.txt", &text, "Big");
    let repo = Repo::open(fx.path()).unwrap();
    let blame = repo.blame("big.txt", "HEAD").unwrap();
    assert!(blame.cut);
    assert!(blame.lines.len() > 1000 && blame.lines.len() < 150_000);
    for (i, line) in blame.lines.iter().enumerate() {
        assert_eq!(line.text, format!("line {i:06} of a file far too long to blame"));
    }

    let stop = AtomicBool::new(true);
    assert!(matches!(
        repo.blame_with("big.txt", "HEAD", &stop),
        Err(oxbow_core::Error::Cancelled)
    ));
    assert!(matches!(
        repo.file_history_with("big.txt", None, &stop),
        Err(oxbow_core::Error::Cancelled)
    ));
    assert!(matches!(
        repo.line_history_with("big.txt", 1, "HEAD", &stop),
        Err(oxbow_core::Error::Cancelled)
    ));
    assert!(matches!(
        repo.file_pickaxe_with("big.txt", "line", true, "HEAD", &stop),
        Err(oxbow_core::Error::Cancelled)
    ));
}

#[cfg(unix)]
#[test]
fn names_git_quotes_come_back_as_they_are() {
    let mut fx = Fixture::new();
    let tab = "tab\tname \"x\" \\ é.txt";
    let first = fx.commit(tab, "one\n", "Add it");
    fx.git(&["mv", tab, "new\tname.txt"]);
    let renamed = fx.commit("new\tname.txt", "one\ntwo\n", "Rename it");

    let repo = Repo::open(fx.path()).unwrap();
    let h = repo.file_history("new\tname.txt", None).unwrap();
    let ids: Vec<_> = h.commits.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, [renamed.as_str(), first.as_str()]);
    assert_eq!(h.commits[0].path, "new\tname.txt");
    assert_eq!(h.commits[0].old_path.as_deref(), Some(tab));
    assert_eq!(h.commits[1].path, tab);

    let blame = repo.blame("new\tname.txt", "HEAD").unwrap();
    let paths: Vec<_> = blame
        .lines
        .iter()
        .map(|l| blame.commits[l.commit].path.as_str())
        .collect();
    assert_eq!(paths, [tab, "new\tname.txt"]);
}
