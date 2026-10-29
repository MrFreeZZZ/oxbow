//! Property tests of the parsers. Whatever is typed into a Repository File, its check answers
//! without a panic and points only into the text; a git-config file (.lfsconfig, .gitmodules) is
//! unreadable for the check exactly when it is for git; well-formed lines are no error; and a
//! remote's error is sorted by its words wherever they stand in git's output.

mod support;

use std::process::Command;

use oxbow_core::repofiles::{FileCheck, Fix, Level, RepoFileKind};
use oxbow_core::{Error, FailureKind, Repo, classify_failure};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use support::Fixture;

/// A repository with a few files and folders, each kind's working copy and history read once.
fn repo() -> (Fixture, Repo) {
    let mut fx = Fixture::new();
    fx.commit("src/main.rs", "fn main() {}\n", "First");
    fx.commit("docs/guide.md", "# Guide\n", "Docs");
    fx.commit("a.txt", "a\n", "A");
    let repo = Repo::open(fx.path()).unwrap();
    for kind in RepoFileKind::ALL {
        repo.check_repo_file(kind, false, "", true).unwrap();
    }
    (fx, repo)
}

/// Bits of every format the files have, and of none: comments, globs, sections, owners, hashes,
/// quotes, escapes, tabs, a carriage return, letters outside ASCII; between `|`.
const BITS: &str = "#|!|*|**|/|\\|[|]|{|}|,|=|\"| |\t|\r|@|@octo|@org/team|a@b.co|<a@b.co>|é|😀|..|?|-|;|\
    [core]|[section \"x\"]|[*]|[*.{rs,md}]|{1..3}|src/|a.txt|*.rs|text|eol=lf|-diff|binary|indent_size|\
    lfs.url|url|path|submodule|[submodule \"m\"]|root = true|0123456789abcdef0123456789abcdef01234567";

/// One of the bits, a few letters, or any character.
fn bit() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::sample::select(BITS.split('|').collect::<Vec<_>>()).prop_map(String::from),
        "[a-z]{1,6}",
        any::<char>().prop_map(String::from),
    ]
}

/// A text of up to 12 lines made of those bits.
fn text() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::collection::vec(bit(), 0..8).prop_map(|bits| bits.concat()), 0..12)
        .prop_map(|lines| lines.join("\n"))
}

/// The check points only into the text: problems, their fixes and notes name a line it has, and
/// an underline stays within its line (UTF-16 units, as the editor counts).
fn sound(check: &FileCheck, text: &str) -> std::result::Result<(), TestCaseError> {
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    for p in &check.problems {
        prop_assert!(
            p.line <= lines.len(),
            "line {} of {}: {}",
            p.line,
            lines.len(),
            p.message
        );
        prop_assert!(p.from <= p.to, "{}..{}: {}", p.from, p.to, p.message);
        if p.line > 0 {
            let units = lines[p.line - 1].encode_utf16().count();
            prop_assert!(p.to <= units, "{}..{} past {units}: {}", p.from, p.to, p.message);
        }
        if let Some(Fix::Replace { line, .. }) = &p.fix {
            prop_assert!(
                (1..=lines.len()).contains(line),
                "fix of line {line} of {}",
                lines.len()
            );
        }
    }
    for n in &check.notes {
        prop_assert!(
            (1..=lines.len()).contains(&n.line),
            "note on line {} of {}",
            n.line,
            lines.len()
        );
    }
    Ok(())
}

fn errors(check: &FileCheck) -> Vec<String> {
    check
        .problems
        .iter()
        .filter(|p| p.level == Level::Error)
        .map(|p| format!("{}: {}", p.line, p.message))
        .collect()
}

#[test]
fn every_check_answers_any_text_and_points_only_into_it() {
    let (_fx, repo) = repo();
    // A runner for each run: one runner counts its cases across runs and stops early.
    let runner = || TestRunner::new(Config::with_cases(150));
    for kind in RepoFileKind::ALL {
        for gitlab in [false, true] {
            runner()
                .run(&prop_oneof![text(), "\\PC{0,200}"], |text| {
                    let check = repo.check_repo_file(kind, gitlab, &text, false).unwrap();
                    sound(&check, &text)?;
                    let now = repo.check_repo_file_now(kind, gitlab, &text, true);
                    sound(&now, &text)
                })
                .unwrap_or_else(|err| panic!("{kind:?} (GitLab {gitlab}): {err}"));
        }
    }
}

/// Lines of a git-config file, as typed: sections, keys, values, comments, and the mistakes.
fn config_text() -> impl Strategy<Value = String> {
    let line = prop_oneof![
        prop::sample::select(vec![
            "[lfs]",
            "[core]",
            "[remote \"origin\"]",
            "[submodule \"lib\"]",
            "[a.b]",
            "[lfs",
            "[ lfs ]",
            "[sub \"x",
            "[x \"a\\\"b\"]",
            "url = https://example.com/x.git",
            "path = lib",
            "\turl=x",
            "key",
            "key = \"quoted\"",
            "key = \"open",
            "key = a\\",
            "key = a \\n b",
            "= value",
            "1key = v",
            "k-ey = v",
            "k_ey = v",
            "# comment",
            "; comment",
            "",
            "   ",
            "key = v # c",
            "key = \"a;b\"",
            "fetchrecursesubmodules = true",
            "concurrenttransfers = 3",
            "é = v",
            "key = é",
            "\"key\" = v",
        ])
        .prop_map(String::from),
        "[a-z]{1,5} = [a-z0-9 ]{0,6}",
    ];
    prop::collection::vec(line, 0..8).prop_map(|lines| lines.join("\n"))
}

/// Whether git reads `text` as a config file.
fn git_reads(dir: &std::path::Path, text: &str) -> bool {
    let file = dir.join("config-under-test");
    std::fs::write(&file, text).unwrap();
    Command::new("git")
        .args(["config", "--file"])
        .arg(&file)
        .arg("--list")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap()
        .status
        .success()
}

#[test]
fn a_config_file_is_unreadable_for_the_check_exactly_when_git_cant_read_it() {
    let (fx, repo) = repo();
    let dir = fx.path().parent().unwrap().to_path_buf();
    let runner = || TestRunner::new(Config::with_cases(200));
    for kind in [RepoFileKind::Lfsconfig, RepoFileKind::Gitmodules] {
        runner()
            .run(&config_text(), |text| {
                let check = repo.check_repo_file(kind, false, &text, false).unwrap();
                let unreadable = check
                    .problems
                    .iter()
                    .any(|p| p.message.starts_with("Git can’t read this line"));
                prop_assert_eq!(unreadable, !git_reads(&dir, &text), "{:?}", errors(&check));
                Ok(())
            })
            .unwrap_or_else(|err| panic!("{kind:?}: {err}"));
    }
}

#[test]
fn well_formed_lines_are_no_error() {
    let (_fx, repo) = repo();
    let runner = || TestRunner::new(Config::with_cases(200));
    let pattern = || prop::sample::select(vec!["*", "*.rs", "/src/", "src/**", "docs/*.md", "a.txt", "/a.txt"]);

    let owners = prop::collection::vec(
        prop::sample::select(vec![
            "@octo",
            "@octo-cat",
            "@org/team",
            "@org/web-team",
            "dev@example.com",
        ]),
        1..4,
    );
    runner()
        .run(&prop::collection::vec((pattern(), owners), 0..8), |lines| {
            let text: String = lines.iter().map(|(p, o)| format!("{p} {}\n", o.join(" "))).collect();
            for gitlab in [false, true] {
                let check = repo
                    .check_repo_file(RepoFileKind::Codeowners, gitlab, &text, false)
                    .unwrap();
                prop_assert!(errors(&check).is_empty(), "{text:?}: {:?}", errors(&check));
            }
            Ok(())
        })
        .unwrap();

    let key = prop::sample::select(vec![
        "indent_style = space",
        "indent_style = tab",
        "indent_size = 4",
        "tab_width = 8",
        "end_of_line = lf",
        "charset = utf-8",
        "trim_trailing_whitespace = true",
        "insert_final_newline = false",
        "max_line_length = 100",
    ]);
    let glob = prop::sample::select(vec![
        "*",
        "*.rs",
        "*.{rs,md}",
        "src/**.rs",
        "[Mm]akefile",
        "a.txt",
        "**/docs/*",
    ]);
    runner()
        .run(
            &prop::collection::vec((glob, prop::collection::vec(key, 1..4)), 0..5),
            |sections| {
                let mut text = String::from("root = true\n");
                for (glob, keys) in &sections {
                    text.push_str(&format!("\n[{glob}]\n{}\n", keys.join("\n")));
                }
                let check = repo
                    .check_repo_file(RepoFileKind::Editorconfig, false, &text, false)
                    .unwrap();
                prop_assert!(errors(&check).is_empty(), "{text:?}: {:?}", errors(&check));
                Ok(())
            },
        )
        .unwrap();

    let attributes = prop::collection::vec(
        (
            pattern(),
            prop::collection::vec(
                prop::sample::select(vec!["text", "-text", "eol=lf", "binary", "-diff", "linguist-generated"]),
                1..3,
            ),
        ),
        0..8,
    );
    runner()
        .run(&attributes, |lines| {
            let text: String = lines.iter().map(|(p, a)| format!("{p} {}\n", a.join(" "))).collect();
            let check = repo
                .check_repo_file(RepoFileKind::Gitattributes, false, &text, false)
                .unwrap();
            prop_assert!(errors(&check).is_empty(), "{text:?}: {:?}", errors(&check));
            Ok(())
        })
        .unwrap();
}

fn failed(output: String) -> Error {
    Error::Command {
        command: "git push".into(),
        code: Some(1),
        output,
    }
}

/// Lines git and a server print around the words that matter.
fn noise() -> impl Strategy<Value = String> {
    prop::collection::vec("(remote: )?[a-z0-9 .,:/'-]{0,40}", 0..4).prop_map(|lines| lines.join("\n"))
}

#[test]
fn a_remotes_error_is_sorted_by_its_words_wherever_they_stand() {
    let words = prop::sample::select(vec![
        ("! [rejected]        main -> main (fetch first)", FailureKind::Rejected),
        (
            "hint: Updates were rejected because the tip is behind (non-fast-forward)",
            FailureKind::Rejected,
        ),
        ("CONFLICT (content): Merge conflict in a.txt", FailureKind::Conflict),
        ("error: could not apply 1a2b3c4... Edit", FailureKind::Conflict),
        (
            "fatal: Authentication failed for 'https://example.com/x.git/'",
            FailureKind::Auth,
        ),
        ("git@github.com: Permission denied (publickey).", FailureKind::Auth),
        ("Host key verification failed.", FailureKind::Auth),
        (
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled",
            FailureKind::Auth,
        ),
        (
            "ssh: Could not resolve hostname github.com: nodename nor servname provided",
            FailureKind::Network,
        ),
        (
            "fatal: unable to access 'https://example.com/': Connection timed out",
            FailureKind::Network,
        ),
        (" ! [rejected] main -> main (stale info)", FailureKind::StaleLease),
    ]);
    let runner = || TestRunner::new(Config::with_cases(300));
    runner()
        .run(
            &(noise(), words, noise(), any::<bool>()),
            |(before, (said, kind), after, hook)| {
                let output = format!("{before}\n{said}\n{after}");
                prop_assert_eq!(classify_failure(&failed(output), hook), kind);
                Ok(())
            },
        )
        .unwrap();
    // Noise alone is no known failure; any text at all is sorted without a panic.
    runner()
        .run(&noise(), |output| {
            prop_assert_eq!(classify_failure(&failed(output), false), FailureKind::Other);
            Ok(())
        })
        .unwrap();
    runner()
        .run(&("\\PC{0,300}", any::<bool>()), |(output, hook)| {
            classify_failure(&failed(output), hook);
            Ok(())
        })
        .unwrap();
}
