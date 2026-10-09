//! Git LFS: big files kept on the LFS server, with a small pointer file in the repository.
//!
//! Which files go to LFS is a rule in `.gitattributes` (`*.psd filter=lfs diff=lfs merge=lfs
//! -text`). Oxbow reads those rules itself, so it can say what a repository keeps in LFS even when
//! the `git-lfs` tool is not installed and every such file is still a pointer. Changes go through
//! `git lfs` like any other command.

use std::io::Read;
use std::path::Path;

use serde::Serialize;

use crate::cli::GitCommand;
use crate::error::Result;
use crate::repo::Repo;

/// What every pointer file starts with.
const POINTER_START: &str = "version https://git-lfs.github.com/spec/v1";

/// Pointer files are tiny; anything bigger is the real file.
const MAX_POINTER: u64 = 1024;

/// Files at least this big get a note in Changes: GitHub warns at 50 MB and refuses 100 MB.
pub const BIG_FILE: u64 = 50 * 1024 * 1024;

/// The LFS side of a repository.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LfsStatus {
    /// `3.4.1`; `None` when `git lfs` does not run.
    pub version: Option<String>,
    /// `git lfs install` has set up the filters that swap pointers and files.
    pub filters: bool,
    /// Rules of the top `.gitattributes` that send files to LFS, in the file's order.
    pub patterns: Vec<LfsPattern>,
    /// Where Homebrew is, to install git-lfs with it.
    pub brew: Option<String>,
}

/// One `filter=lfs` rule and the files it covers.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LfsPattern {
    pub pattern: String,
    /// Files in the index the rule matches.
    pub files: u32,
    /// Their size in bytes: the real file's, or the one a pointer names.
    pub size: u64,
    /// Of those, files that are still a pointer here: not downloaded.
    pub missing: u32,
    /// The rule is not in the last commit's `.gitattributes` yet.
    pub new: bool,
}

/// A pointer file: which LFS object stands in for the file.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LfsPointer {
    /// `sha256:4d7a…`
    pub oid: String,
    pub size: u64,
}

/// A change to a file kept in LFS: the pointers before and after, where they are known.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct LfsChange {
    pub old: Option<LfsPointer>,
    pub new: Option<LfsPointer>,
}

/// Parse a pointer file; `None` for anything else.
pub fn parse_pointer(data: &[u8]) -> Option<LfsPointer> {
    if data.len() as u64 > MAX_POINTER || !data.starts_with(POINTER_START.as_bytes()) {
        return None;
    }
    let text = std::str::from_utf8(data).ok()?;
    let mut oid = None;
    let mut size = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("oid ") {
            oid = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("size ") {
            size = value.trim().parse().ok();
        }
    }
    Some(LfsPointer { oid: oid?, size: size? })
}

/// The patterns of `.gitattributes` text whose files go to LFS.
pub fn lfs_patterns(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        let Some(pattern) = words.next() else { continue };
        let lfs = words.any(|w| w == "filter=lfs");
        if lfs && !out.iter().any(|p| p == pattern) {
            out.push(pattern.to_owned());
        }
    }
    out
}

/// The pathspec that lists the files a `.gitattributes` pattern matches: one without a slash
/// matches a name in any folder, one with a slash is a path from the top.
fn pathspec(pattern: &str) -> String {
    let anchored = pattern.trim_end_matches('/').contains('/');
    if anchored {
        format!(":(glob){}", pattern.trim_start_matches('/'))
    } else {
        format!(":(glob)**/{pattern}")
    }
}

/// `git lfs track` and staging what it changed: `.gitattributes`, and `paths` so they go in as
/// pointers.
pub(crate) fn plan_track(pattern: &str, paths: &[String], stage: bool) -> Vec<GitCommand> {
    let mut commands = vec![GitCommand::new(["lfs", "track", pattern]).comment(format!(
        "writes {} filter=lfs diff=lfs merge=lfs -text to .gitattributes",
        pattern
    ))];
    if !stage {
        return commands;
    }
    let mut add = vec!["add".to_owned(), "--".to_owned(), ".gitattributes".to_owned()];
    add.extend(paths.iter().map(|p| crate::cli::literal(p)));
    commands.push(GitCommand::new(add).comment(if paths.is_empty() {
        "the rule goes in with the next commit"
    } else {
        "git stores the pointer, Git LFS keeps the file"
    }));
    commands
}

pub(crate) fn plan_untrack(pattern: &str) -> Vec<GitCommand> {
    vec![
        GitCommand::new(["lfs", "untrack", pattern]).comment("removes the rule from .gitattributes"),
        GitCommand::new(["add", "--", ".gitattributes"])
            .comment("files already in LFS stay there; new versions are stored by git itself"),
    ]
}

pub(crate) fn plan_pull(include: Option<&str>) -> Vec<GitCommand> {
    let mut args = vec!["lfs".to_owned(), "pull".to_owned()];
    if let Some(pattern) = include {
        args.push(format!("--include={pattern}"));
    }
    vec![GitCommand::new(args).comment(match include {
        Some(_) => "downloads only the files the pattern matches and puts them in place of their pointers",
        None => "downloads the files of the checked-out commit and puts them in place of their pointers",
    })]
}

pub(crate) fn plan_prune() -> Vec<GitCommand> {
    vec![
        GitCommand::new(["lfs", "prune", "--verify-remote"])
            .comment("deletes local copies of old versions; --verify-remote: only ones the LFS server has"),
    ]
}

pub(crate) fn plan_install(brew: Option<&str>, pull: bool) -> Vec<GitCommand> {
    let mut commands = Vec::new();
    if let Some(brew) = brew {
        // Plain `brew` when that is the one a shell finds, as people type it.
        let on_path = crate::config::find_on_path("brew").is_some_and(|p| p == std::path::Path::new(brew));
        commands.push(
            GitCommand::new(["install", "git-lfs"])
                .program(if on_path { "brew" } else { brew })
                .comment("the Git LFS command line tool"),
        );
    }
    commands.push(
        GitCommand::new(["lfs", "install"]).comment("turns on the LFS filters in ~/.gitconfig, once per computer"),
    );
    if pull {
        commands.extend(
            plan_pull(None)
                .into_iter()
                .map(|c| c.comment("replaces the pointers with the real files")),
        );
    }
    commands
}

/// More of `.gitattributes` than anyone writes by hand; the rest is not read.
const MAX_ATTRIBUTES_BYTES: u64 = 1024 * 1024;

/// At most `limit` bytes of `path` when it is a plain file. A repository can commit a symlink,
/// say to /dev/zero or a FIFO, where a file is expected; reading through it would never end.
/// The file opened must be the one looked at, so a swap in between is caught too.
pub(crate) fn read_regular(path: &Path, limit: u64) -> Option<Vec<u8>> {
    let seen = std::fs::symlink_metadata(path).ok()?;
    if !seen.file_type().is_file() {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    let opened = file.metadata().ok()?;
    if !opened.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (seen.dev(), seen.ino()) != (opened.dev(), opened.ino()) {
            return None;
        }
    }
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// `.gitattributes` in the working copy, when it is a plain file: git itself never follows a
/// symlink for attributes.
pub(crate) fn read_attributes(workdir: &Path) -> Option<String> {
    read_regular(&workdir.join(".gitattributes"), MAX_ATTRIBUTES_BYTES)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

impl Repo {
    /// Which files the repository keeps in LFS, and whether `git lfs` is there to fetch them.
    pub fn lfs_status(&self) -> Result<LfsStatus> {
        let version = self.run(&GitCommand::new(["lfs", "version"])).ok().map(|out| {
            let first = out.stdout.split_whitespace().next().unwrap_or_default().to_owned();
            first.strip_prefix("git-lfs/").map_or(first.clone(), str::to_owned)
        });
        let text = read_attributes(self.workdir()).unwrap_or_default();
        let committed = self
            .run(&GitCommand::new(["cat-file", "-p", "HEAD:.gitattributes"]))
            .map(|out| lfs_patterns(&out.stdout))
            .unwrap_or_default();
        let mut patterns = Vec::new();
        for pattern in lfs_patterns(&text) {
            let out = self.run(&GitCommand::new(["ls-files", "-z", "--", &pathspec(&pattern)]))?;
            let mut entry = LfsPattern {
                new: !committed.contains(&pattern),
                pattern,
                files: 0,
                size: 0,
                missing: 0,
            };
            for path in out.stdout.split('\0').filter(|p| !p.is_empty()) {
                let Some((size, pointer)) = self.lfs_file_size(path) else {
                    continue;
                };
                entry.files += 1;
                entry.size += size;
                if pointer {
                    entry.missing += 1;
                }
            }
            patterns.push(entry);
        }
        let filters = self
            .run(&GitCommand::new(["config", "--get", "filter.lfs.process"]))
            .is_ok_and(|out| !out.stdout.trim().is_empty());
        Ok(LfsStatus {
            version,
            filters,
            patterns,
            brew: brew().map(|p| p.display().to_string()),
        })
    }

    /// The size of a file in the working copy, and whether it is only a pointer; `None` when it
    /// is not there.
    pub(crate) fn lfs_file_size(&self, path: &str) -> Option<(u64, bool)> {
        let file = self.workdir().join(path);
        // A symlink is a link here, as for git, not the file it points at.
        let meta = std::fs::symlink_metadata(&file).ok()?;
        if !meta.is_file() {
            return None;
        }
        let len = meta.len();
        if len > MAX_POINTER {
            return Some((len, false));
        }
        let data = read_regular(&file, MAX_POINTER)?;
        Some(match parse_pointer(&data) {
            Some(pointer) => (pointer.size, true),
            None => (len, false),
        })
    }

    /// Paths among `paths` that `.gitattributes` sends to LFS.
    pub(crate) fn lfs_paths(&self, paths: &[&str]) -> std::collections::HashSet<String> {
        let mut out = std::collections::HashSet::new();
        if paths.is_empty() {
            return out;
        }
        let input: String = paths.iter().flat_map(|p| [*p, "\0"]).collect();
        let Ok(result) = self.run_with_input(
            &GitCommand::new(["check-attr", "-z", "--stdin", "filter"]),
            Some(input.as_bytes()),
        ) else {
            return out;
        };
        // `<path>\0filter\0<value>\0` for each path.
        let fields: Vec<&str> = result.stdout.split('\0').collect();
        for record in fields.chunks(3) {
            if let [path, _, value] = record
                && *value == "lfs"
            {
                out.insert((*path).to_owned());
            }
        }
        out
    }

    /// How much Free Up Space would delete, e.g. `3 files would be pruned (1.4 GB)`; `None` when
    /// nothing.
    pub fn lfs_prune_preview(&self) -> Result<Option<String>> {
        let out = self.run(&GitCommand::new(["lfs", "prune", "--dry-run"]))?;
        Ok(format!("{}\n{}", out.stdout, out.stderr)
            .lines()
            .find(|line| line.contains("would be pruned"))
            .map(|line| line.trim().trim_start_matches("prune:").trim().to_owned()))
    }
}

/// Homebrew, where it installs itself; apps started from the Dock don't see the shell's `PATH`.
fn brew() -> Option<std::path::PathBuf> {
    crate::config::find_on_path("brew").or_else(|| {
        [
            "/opt/homebrew/bin/brew",
            "/usr/local/bin/brew",
            "/home/linuxbrew/.linuxbrew/bin/brew",
        ]
        .into_iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.is_file())
    })
}

/// Put the folders where Homebrew and other installers put tools on `PATH` when they are not on
/// it: an app started from the Dock gets only `/usr/bin:/bin:/usr/sbin:/sbin`, so git would not
/// find `git-lfs`, and checking out a repository with LFS files would fail.
pub fn extend_path() {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let mut paths: Vec<std::path::PathBuf> = std::env::split_paths(&current).collect();
    let mut changed = false;
    for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"] {
        let dir = std::path::PathBuf::from(dir);
        if dir.is_dir() && !paths.contains(&dir) {
            paths.push(dir);
            changed = true;
        }
    }
    if changed && let Ok(joined) = std::env::join_paths(paths) {
        // SAFETY: called once at start-up, before any other thread reads the environment.
        unsafe { std::env::set_var("PATH", joined) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_pointer() {
        let text = "version https://git-lfs.github.com/spec/v1\noid sha256:4d7a\nsize 195035136\n";
        assert_eq!(
            parse_pointer(text.as_bytes()),
            Some(LfsPointer {
                oid: "sha256:4d7a".into(),
                size: 195035136
            })
        );
        assert_eq!(parse_pointer(b"hello"), None);
    }

    #[test]
    fn finds_lfs_rules() {
        let text = "# assets\n*.psd filter=lfs diff=lfs merge=lfs -text\n*.rs text eol=lf\nfixtures/*.bin filter=lfs diff=lfs merge=lfs -text\n*.psd filter=lfs\n";
        assert_eq!(lfs_patterns(text), ["*.psd", "fixtures/*.bin"]);
    }

    #[test]
    fn patterns_become_pathspecs() {
        assert_eq!(pathspec("*.psd"), ":(glob)**/*.psd");
        assert_eq!(pathspec("fixtures/*.bin"), ":(glob)fixtures/*.bin");
        assert_eq!(pathspec("/assets/demo/**"), ":(glob)assets/demo/**");
    }

    #[test]
    fn track_stages_the_rule_and_the_files() {
        let commands = plan_track("*.mov", &["assets/demo/intro.mov".into()], true);
        assert_eq!(commands[0].display(), "git lfs track '*.mov'");
        assert_eq!(commands[1].display(), "git add -- .gitattributes assets/demo/intro.mov");
    }

    #[test]
    fn track_without_staging_leaves_the_index_alone() {
        let commands = plan_track("*.mov", &["assets/demo/intro.mov".into()], false);
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].display(), "git lfs track '*.mov'");
    }
}
