mod support;

use oxbow_core::Repo;
use oxbow_core::repofiles::{Fix, Level, RepoFileKind};
use support::Fixture;

fn problems(check: &oxbow_core::repofiles::FileCheck, level: Level) -> Vec<(usize, String)> {
    check
        .problems
        .iter()
        .filter(|p| p.level == level)
        .map(|p| (p.line, p.message.clone()))
        .collect()
}

fn fix_text(check: &oxbow_core::repofiles::FileCheck, line: usize) -> Option<String> {
    check
        .problems
        .iter()
        .filter(|p| p.line == line)
        .find_map(|p| match &p.fix {
            Some(Fix::Replace { text, .. }) => text.clone(),
            _ => None,
        })
}

fn note(check: &oxbow_core::repofiles::FileCheck, line: usize) -> Option<String> {
    check.notes.iter().find(|n| n.line == line).map(|n| n.text.clone())
}

#[test]
fn the_list_says_what_is_there_and_what_is_not_committed() {
    let mut fx = Fixture::new();
    fx.commit(".gitignore", "# build\n/target\n\n*.log\n", "ignore");
    fx.write(".gitignore", "# build\n/target\n*.log\n.env\n");
    fx.write(".gitattributes", "*.txt eol=crfl\n");
    let repo = Repo::open(&fx.path).unwrap();

    let files = repo.repo_files(false).unwrap();
    let ignore = files.iter().find(|f| f.kind == RepoFileKind::Gitignore).unwrap();
    assert!(ignore.exists && ignore.uncommitted);
    assert_eq!(ignore.rules, 3);
    assert!(ignore.problem.is_none());
    let attributes = files.iter().find(|f| f.kind == RepoFileKind::Gitattributes).unwrap();
    assert!(
        attributes
            .problem
            .as_ref()
            .is_some_and(|p| p.message.contains("eol=crfl"))
    );
    let mailmap = files.iter().find(|f| f.kind == RepoFileKind::Mailmap).unwrap();
    assert!(!mailmap.exists && !mailmap.uncommitted);
    let owners = files.iter().find(|f| f.kind == RepoFileKind::Codeowners).unwrap();
    assert_eq!(owners.path, ".github/CODEOWNERS");
    let owners = repo
        .repo_files(true)
        .unwrap()
        .into_iter()
        .find(|f| f.kind == RepoFileKind::Codeowners)
        .unwrap();
    assert_eq!(owners.path, "CODEOWNERS", "GitLab reads it at the top first");
}

#[test]
fn gitignore_errors_and_what_each_line_matches() {
    let mut fx = Fixture::new();
    fx.commit("config/local.json", "{}", "config");
    fx.write("dist/a.js", "a");
    fx.write("dist/b.js", "b");
    fx.write("logs/keep.txt", "keep");
    fx.write("app.log", "x");
    let repo = Repo::open(&fx.path).unwrap();
    let text = "node_modules/\ndist/\nbuild\\out\n./coverage\n*.log\nlogs/\n!logs/keep.txt\nconfig/local.json\ndocs/old-notes.md\n*.log\n";
    let check = repo
        .check_repo_file(RepoFileKind::Gitignore, false, text, true)
        .unwrap();

    let errors = problems(&check, Level::Error);
    assert_eq!(errors.iter().map(|e| e.0).collect::<Vec<_>>(), vec![3, 4]);
    assert_eq!(fix_text(&check, 3).as_deref(), Some("build/out"));
    assert_eq!(fix_text(&check, 4).as_deref(), Some("/coverage"));

    let warnings = problems(&check, Level::Warn);
    assert!(
        warnings.iter().any(|(l, m)| *l == 7 && m.contains("`logs/` on line 6")),
        "{warnings:?}"
    );
    assert_eq!(fix_text(&check, 7).as_deref(), Some("logs/*"), "the fix is on line 6");
    assert!(warnings.iter().any(|(l, m)| *l == 8 && m.contains("already committed")));
    let untrack = check.problems.iter().find(|p| p.line == 8).and_then(|p| p.fix.clone());
    assert!(matches!(untrack, Some(Fix::Untrack { paths, .. }) if paths == vec!["config/local.json".to_owned()]));
    assert!(warnings.iter().any(|(l, m)| *l == 9 && m.contains("docs/old-notes.md")));
    assert!(warnings.iter().any(|(l, m)| *l == 10 && m.contains("Line 5")));

    assert_eq!(note(&check, 1).as_deref(), Some("nothing yet"));
    assert_eq!(note(&check, 2).as_deref(), Some("1 folder · 2 files"));
    assert_eq!(note(&check, 3).as_deref(), Some("matches nothing"));
    assert_eq!(note(&check, 5).as_deref(), Some("1 file"));
}

#[test]
fn save_writes_only_a_file_without_errors() {
    let fx = Fixture::new();
    let repo = Repo::open(&fx.path).unwrap();
    let refused = repo.save_repo_file(RepoFileKind::Gitignore, false, "./dist\n");
    assert!(refused.is_err());
    assert!(!fx.path.join(".gitignore").exists());

    let path = repo.save_repo_file(RepoFileKind::Gitignore, false, "/dist\n").unwrap();
    assert_eq!(path, ".gitignore");
    assert_eq!(std::fs::read_to_string(fx.path.join(".gitignore")).unwrap(), "/dist\n");
    let owners = repo
        .save_repo_file(RepoFileKind::Codeowners, false, "* @acme/core\n")
        .unwrap();
    assert_eq!(owners, ".github/CODEOWNERS");
    assert!(fx.path.join(".github/CODEOWNERS").is_file());
    assert!(repo.save_repo_file(RepoFileKind::Gitmodules, false, "").is_err());
    // Nothing is staged.
    let status = std::process::Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(&fx.path)
        .output()
        .unwrap();
    assert!(status.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn save_never_writes_through_a_link() {
    let fx = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path().join("x"), fx.path.join(".mailmap")).unwrap();
    let repo = Repo::open(&fx.path).unwrap();
    assert!(
        repo.save_repo_file(RepoFileKind::Mailmap, false, "A <a@b.c> <x@y.z>\n")
            .is_err()
    );
    assert!(!outside.path().join("x").exists());
}

#[cfg(unix)]
#[test]
fn save_never_writes_through_a_link_waiting_where_its_new_file_was() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fixture::new();
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(outside.path(), "outside original").unwrap();
    // The name the new file had before: the process id made it easy to guess.
    let guessed = fx.path.join(format!("..gitignore.oxbow-{}", std::process::id()));
    std::os::unix::fs::symlink(outside.path(), &guessed).unwrap();
    let repo = Repo::open(&fx.path).unwrap();
    repo.save_repo_file(RepoFileKind::Gitignore, false, "*.log\n").unwrap();
    assert_eq!(std::fs::read_to_string(outside.path()).unwrap(), "outside original");
    let saved = std::fs::symlink_metadata(fx.path.join(".gitignore")).unwrap();
    assert!(saved.file_type().is_file());
    assert_eq!(std::fs::read_to_string(fx.path.join(".gitignore")).unwrap(), "*.log\n");
    // A new file can be read by others as one git writes would be, an old one keeps its mode.
    assert_eq!(saved.permissions().mode() & 0o644, 0o644);
    std::fs::set_permissions(fx.path.join(".gitignore"), std::fs::Permissions::from_mode(0o640)).unwrap();
    repo.save_repo_file(RepoFileKind::Gitignore, false, "*.tmp\n").unwrap();
    let saved = std::fs::metadata(fx.path.join(".gitignore")).unwrap();
    assert_eq!(saved.permissions().mode() & 0o777, 0o640);
    // Nothing of its own is left beside it.
    let left: Vec<_> = std::fs::read_dir(&fx.path)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("oxbow"))
        .collect();
    assert_eq!(left, vec![guessed.file_name().unwrap().to_string_lossy().into_owned()]);
}

#[test]
fn codeowners_saved_on_github_opens_again_up_to_its_limit() {
    let fx = Fixture::new();
    let repo = Repo::open(&fx.path).unwrap();
    let text = format!("#{}\n* @team\n", "a".repeat(1024 * 1024));
    repo.save_repo_file(RepoFileKind::Codeowners, false, &text).unwrap();
    let file = repo.repo_file_text(RepoFileKind::Codeowners, false).unwrap();
    assert_eq!(file.read_only, None);
    assert_eq!(file.text, text);
    let listed = repo.repo_files(false).unwrap();
    let owners = listed.iter().find(|f| f.kind == RepoFileKind::Codeowners).unwrap();
    assert!(owners.problem.is_none());
    // On GitLab the same file is over Oxbow's own limit, and says so when it opens.
    std::fs::create_dir_all(fx.path.join(".gitlab")).unwrap();
    std::fs::write(fx.path.join(".gitlab/CODEOWNERS"), &text).unwrap();
    let file = repo.repo_file_text(RepoFileKind::Codeowners, true).unwrap();
    assert!(file.read_only.unwrap().contains("over 1 MB"));
    let too_big = format!("#{}\n", "a".repeat(3 * 1024 * 1024));
    assert!(repo.save_repo_file(RepoFileKind::Codeowners, false, &too_big).is_err());
}

#[test]
fn editorconfig_range_of_the_biggest_numbers_is_an_error() {
    let fx = Fixture::new();
    let repo = Repo::open(&fx.path).unwrap();
    for text in [
        "[{-9223372036854775808..9223372036854775807}]\nindent_size = 2\n",
        "[{9223372036854775807..-9223372036854775808}]\nindent_size = 2\n",
        "[{0..1001}]\nindent_size = 2\n",
    ] {
        let check = repo
            .check_repo_file(RepoFileKind::Editorconfig, false, text, true)
            .unwrap();
        assert_eq!(
            problems(&check, Level::Error).iter().map(|e| e.0).collect::<Vec<_>>(),
            vec![1],
            "{text}"
        );
    }
    let check = repo
        .check_repo_file(
            RepoFileKind::Editorconfig,
            false,
            "[file{1..3}.txt]\nindent_size = 2\n",
            true,
        )
        .unwrap();
    assert!(problems(&check, Level::Error).is_empty());
}

#[test]
fn blame_ignore_revs_needs_full_hashes() {
    let mut fx = Fixture::new();
    let id = fx.commit("a.rs", "fn main() {}\n", "Format all Rust code");
    let repo = Repo::open(&fx.path).unwrap();
    let text = format!(
        "# formatting\n{id}\n{}\n{}  # gone\nnot-a-hash\n",
        &id[..7],
        "e5b8d2c61a0f4937b8e2d1c6a5f0b93e7d4c2a18"
    );
    let check = repo
        .check_repo_file(RepoFileKind::BlameIgnoreRevs, false, &text, true)
        .unwrap();
    assert!(note(&check, 2).is_some_and(|n| n.starts_with("Format all Rust code · ")));
    assert_eq!(
        problems(&check, Level::Error).iter().map(|e| e.0).collect::<Vec<_>>(),
        vec![3, 5]
    );
    assert_eq!(fix_text(&check, 3), Some(id.clone()));
    assert!(
        problems(&check, Level::Warn)
            .iter()
            .any(|(l, m)| *l == 4 && m.contains("No such commit"))
    );
}

#[test]
fn mailmap_knows_the_history() {
    let mut fx = Fixture::new();
    fx.commit("a", "1", "one");
    fx.commit("a", "2", "two");
    fx.git(&[
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "three",
        "--author",
        "Alexander <alex@old-laptop.local>",
    ]);
    fx.git(&[
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "four",
        "--author",
        "Lee Okafor <lee@oxbow.dev>",
    ]);
    let repo = Repo::open(&fx.path).unwrap();

    let starter = repo.repo_file_text(RepoFileKind::Mailmap, false).unwrap();
    assert!(!starter.exists);
    assert!(
        starter
            .text
            .contains("Alexander <alexander@example.com> <alex@old-laptop.local>"),
        "{}",
        starter.text
    );

    let text = "Alexander <alexander@example.com> <alex@old-laptop.local>\nLee Okafor <lee@oxbow.dev> <lee@oxbw.dev>\nSam Ruiz sam@oxbow.dev\n";
    let check = repo.check_repo_file(RepoFileKind::Mailmap, false, text, true).unwrap();
    assert_eq!(note(&check, 1).as_deref(), Some("1 commit"));
    assert_eq!(note(&check, 2).as_deref(), Some("no commit uses this e-mail"));
    assert_eq!(
        problems(&check, Level::Error).iter().map(|e| e.0).collect::<Vec<_>>(),
        vec![3]
    );
    assert_eq!(fix_text(&check, 3).as_deref(), Some("Sam Ruiz <sam@oxbow.dev>"));
    let typo = problems(&check, Level::Warn);
    assert!(
        typo.iter()
            .any(|(l, m)| *l == 2 && m.contains("commits as `lee@oxbow.dev`")),
        "{typo:?}"
    );
    assert_eq!(
        fix_text(&check, 2).as_deref(),
        Some("Lee Okafor <lee@oxbow.dev> <lee@oxbow.dev>")
    );
}

#[test]
fn gitattributes_rules() {
    let mut fx = Fixture::new();
    fx.commit("art/a.psd", "x", "art");
    let repo = Repo::open(&fx.path).unwrap();
    let text =
        "* text=auto\n!*.bin binary\n*.txt eol=crfl\n*.psd filter=lfs\n*.md\n*.svg diff=nosuchdriver\n*.c diff=cpp\n";
    let check = repo
        .check_repo_file(RepoFileKind::Gitattributes, false, text, true)
        .unwrap();
    assert_eq!(
        problems(&check, Level::Error).iter().map(|e| e.0).collect::<Vec<_>>(),
        vec![2, 3]
    );
    let warnings = problems(&check, Level::Warn);
    assert!(warnings.iter().any(|(l, _)| *l == 4));
    assert_eq!(
        fix_text(&check, 4).as_deref(),
        Some("*.psd filter=lfs diff=lfs merge=lfs -text")
    );
    assert!(warnings.iter().any(|(l, m)| *l == 5 && m.contains("sets nothing")));
    assert!(warnings.iter().any(|(l, m)| *l == 6 && m.contains("diff.nosuchdriver")));
    assert!(!warnings.iter().any(|(l, _)| *l == 7));
    assert_eq!(note(&check, 4).as_deref(), Some("1 file"));
}

#[test]
fn codeowners_on_github_and_gitlab() {
    let mut fx = Fixture::new();
    fx.commit("src/api.rs", "x", "api");
    let repo = Repo::open(&fx.path).unwrap();
    let text = "* @acme/core\n/src/ @octocat dev@acme.io\n[Docs] @acme/docs\n/docs/ @writer\n!*.md @x\nlib/ owner\n";
    let github = repo
        .check_repo_file(RepoFileKind::Codeowners, false, text, true)
        .unwrap();
    assert_eq!(
        problems(&github, Level::Error).iter().map(|e| e.0).collect::<Vec<_>>(),
        vec![3, 5, 6]
    );
    assert!(
        problems(&github, Level::Warn)
            .iter()
            .any(|(l, m)| *l == 4 && m.contains("No committed file"))
    );
    assert_eq!(note(&github, 2).as_deref(), Some("1 file"));
    let gitlab = repo
        .check_repo_file(RepoFileKind::Codeowners, true, text, true)
        .unwrap();
    assert_eq!(
        problems(&gitlab, Level::Error).iter().map(|e| e.0).collect::<Vec<_>>(),
        vec![5, 6]
    );
}

#[test]
fn editorconfig_and_lfsconfig() {
    let mut fx = Fixture::new();
    fx.commit("src/a.rs", "x", "a");
    let repo = Repo::open(&fx.path).unwrap();
    let text = "indent_size = 2\n[*]\nroot = true\nindent_style = tabs\nindent_size = tab\nmy_own = 1\n[*.py]\nend_of_line = lf\n[*.rs\n";
    let check = repo
        .check_repo_file(RepoFileKind::Editorconfig, false, text, true)
        .unwrap();
    assert_eq!(
        problems(&check, Level::Error).iter().map(|e| e.0).collect::<Vec<_>>(),
        vec![1, 3, 4, 9]
    );
    assert!(problems(&check, Level::Warn).iter().any(|(l, _)| *l == 7));
    assert_eq!(note(&check, 2).as_deref(), Some("1 file"));
    assert_eq!(note(&check, 6).as_deref(), Some("not a standard property"));

    let text = "[lfs]\n\turl = https://bot:secret@lfs.example.com/info/lfs\n\tbatch = true\n";
    let check = repo
        .check_repo_file(RepoFileKind::Lfsconfig, false, text, true)
        .unwrap();
    assert_eq!(
        problems(&check, Level::Error).iter().map(|e| e.0).collect::<Vec<_>>(),
        vec![2]
    );
    assert_eq!(
        fix_text(&check, 2).as_deref(),
        Some("\turl = https://lfs.example.com/info/lfs")
    );
    let warnings = problems(&check, Level::Warn);
    assert!(warnings.iter().any(|(l, m)| *l == 3 && m.contains("lfs.batch")));
    assert!(warnings.iter().any(|(l, _)| *l == 0), "no LFS rules yet");
    let broken = repo
        .check_repo_file(RepoFileKind::Lfsconfig, false, "[lfs\nurl = x\n", true)
        .unwrap();
    assert_eq!(problems(&broken, Level::Error).len(), 1);
}

#[test]
fn stop_tracking_keeps_the_files_on_disk() {
    let mut fx = Fixture::new();
    fx.commit("config/local.json", "{}", "config");
    let repo = Repo::open(&fx.path).unwrap();
    let action = oxbow_core::Action::Untrack {
        paths: vec!["config/local.json".into()],
    };
    assert_eq!(
        repo.plan(&action).unwrap().commands[0].display(),
        "git rm -r --cached --quiet -- config/local.json"
    );
    repo.perform(&action).unwrap();
    assert!(fx.path.join("config/local.json").is_file());
    assert_eq!(fx.git(&["status", "--porcelain"]), "D  config/local.json\n?? config/");
}
