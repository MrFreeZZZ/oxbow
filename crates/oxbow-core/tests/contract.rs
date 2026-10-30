//! The JSON the app reads, by its shape: for each type, the fields a value of it has, taken from
//! a real repository. They are kept in app/tests/contract.json, which app/tests/contract.test.ts
//! checks against lib/types.ts. After a change of one of these types, write them again:
//! `OXBOW_CONTRACT=write cargo test -p oxbow-core --test contract`.

mod support;

use std::collections::BTreeMap;

use oxbow_core::repofiles::RepoFileKind;
use oxbow_core::{Action, HistoryOptions, LocalHistory, Repo, Side};
use serde_json::Value;
use support::Fixture;

/// The fields of `value`, an object, sorted.
fn fields(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value.as_object().expect("an object").keys().cloned().collect();
    keys.sort();
    keys
}

fn json(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap()
}

/// `value` at `pointer` (`/rows/0`), which must be there.
fn at<'v>(value: &'v Value, pointer: &str) -> &'v Value {
    value.pointer(pointer).unwrap_or_else(|| panic!("nothing at {pointer}"))
}

fn shapes() -> BTreeMap<&'static str, Vec<String>> {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\ntwo\n", "First");
    fx.commit("a.txt", "one\ntwo\nthree\n", "Second");
    fx.git(&["tag", "v1"]);
    fx.write("a.txt", "one\n2\nthree\n");
    fx.git(&["stash", "-q"]);
    fx.write("a.txt", "one\ntwo\nthree\nfour\n");
    fx.write(".gitignore", "*.log\n./build\n");
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&Action::Stage {
        paths: vec![".gitignore".into()],
    })
    .unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    LocalHistory::open(&repo).keep(&repo, &["a.txt".into()], now.as_millis() as u64);

    let history = json(repo.history(&HistoryOptions::default()).unwrap());
    let tree = json(repo.working_tree().unwrap());
    let diff = json(repo.working_diff("a.txt", Side::Unstaged, false).unwrap());
    let head = repo.head().unwrap().commit.unwrap();
    let detail = json(repo.commit_detail(&head).unwrap());
    let files = json(repo.repo_files(false).unwrap());
    let check = json(
        repo.check_repo_file(RepoFileKind::Gitignore, false, "*.log\n./build\n", true)
            .unwrap(),
    );
    let file_history = json(repo.file_history("a.txt", None).unwrap());
    let blame = json(repo.blame("a.txt", "HEAD").unwrap());
    let operations = json(repo.operation_log().unwrap());
    let versions = json(repo.local_history_all().unwrap());

    let command = json(oxbow_core::user_commands::UserCommand {
        id: "tests".into(),
        name: "Tests".into(),
        script: "cargo test".into(),
        repository: String::new(),
    });
    BTreeMap::from([
        ("UserCommand", fields(&command)),
        (
            "RepoOverview",
            fields(&json(
                oxbow_core::overview::read(fx.path(), &std::sync::atomic::AtomicBool::new(false)).unwrap(),
            )),
        ),
        ("History", fields(&history)),
        ("HeadInfo", fields(at(&history, "/head"))),
        ("HistoryRow", fields(at(&history, "/rows/0"))),
        ("RefInfo", fields(at(&history, "/refs/0"))),
        ("StashInfo", fields(at(&history, "/stashes/0"))),
        ("WorkingTree", fields(&tree)),
        ("FileChange", fields(at(&tree, "/unstaged/0"))),
        ("FileDiff", fields(&diff)),
        ("Hunk", fields(at(&diff, "/hunks/0"))),
        ("DiffLine", fields(at(&diff, "/hunks/0/lines/0"))),
        ("CommitDetail", fields(&detail)),
        ("Person", fields(at(&detail, "/author"))),
        ("RepoFile", fields(at(&files, "/0"))),
        ("FileCheck", fields(&check)),
        ("FileProblem", fields(at(&check, "/problems/0"))),
        ("LineNote", fields(at(&check, "/notes/0"))),
        ("FileHistory", fields(&file_history)),
        ("FileCommit", fields(at(&file_history, "/commits/0"))),
        ("Blame", fields(&blame)),
        ("BlameCommit", fields(at(&blame, "/commits/0"))),
        ("OpEntry", fields(at(&operations, "/0"))),
        ("LocalVersion", fields(at(&versions, "/0"))),
    ])
}

#[test]
fn the_shapes_the_app_reads_are_the_ones_kept_for_it() {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/tests/contract.json");
    let now = serde_json::to_string_pretty(&shapes()).unwrap() + "\n";
    if std::env::var("OXBOW_CONTRACT").as_deref() == Ok("write") {
        std::fs::write(&file, &now).unwrap();
        return;
    }
    let kept = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(
        kept == now,
        "app/tests/contract.json is not what the types serialize now; check lib/types.ts, then \
         OXBOW_CONTRACT=write cargo test -p oxbow-core --test contract\n{now}"
    );
}
