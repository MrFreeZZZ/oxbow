//! Git LFS: big files kept on the LFS server, with a small pointer file in the repository.
//!
//! Which files go to LFS is a rule in `.gitattributes` (`*.psd filter=lfs diff=lfs merge=lfs
//! -text`). Oxbow reads those rules itself, so it can say what a repository keeps in LFS even when
//! the `git-lfs` tool is not installed and every such file is still a pointer. Changes go through
//! `git lfs` like any other command.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::AtomicBool;

use serde::Serialize;

use crate::cli::GitCommand;
use crate::error::{Error, FailedPart, Result};
use crate::repo::Repo;
use crate::worktree::{ActionEvent, PartState};

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
    /// The files that are still pointers here, by path; at most [`MAX_LISTED`].
    pub missing_files: Vec<LfsFile>,
}

/// Files listed as not downloaded; the counts of the patterns go on past it.
pub const MAX_LISTED: usize = 5000;

/// Files a download names one by one; for more it downloads them all, which skips the ones that
/// are there already, rather than make the command too long.
const MAX_NAMED: usize = 200;

/// A file kept in LFS that is only a pointer here: not downloaded.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LfsFile {
    pub path: String,
    /// The size of the real file, as the pointer says.
    pub size: u64,
    /// `sha256:4d7a…`, how git-lfs names it when it can't download it.
    pub oid: String,
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

/// `git lfs pull` for the files at `paths`, else those `include` matches, else all.
pub(crate) fn plan_pull(include: Option<&str>, paths: &[String]) -> Vec<GitCommand> {
    let mut args = vec!["lfs".to_owned(), "pull".to_owned()];
    let named = !paths.is_empty() && paths.len() <= MAX_NAMED;
    if named {
        let list: Vec<String> = paths.iter().map(|p| include_path(p)).collect();
        args.push(format!("--include={}", list.join(",")));
    } else if let Some(pattern) = include {
        args.push(format!("--include={pattern}"));
    }
    vec![GitCommand::new(args).comment(if named {
        "downloads these files and puts them in place of their pointers"
    } else if include.is_some() {
        "downloads only the files the pattern matches and puts them in place of their pointers"
    } else {
        "downloads the files of the checked-out commit and puts them in place of their pointers"
    })]
}

/// A path as a `--include` pattern that matches only that file: `/` ties it to the top, a
/// backslash makes `*`, `?` and `[` plain. git-lfs splits the list at commas with no way to
/// escape one, so a comma becomes `?`, which matches it too.
fn include_path(path: &str) -> String {
    let mut out = String::from("/");
    for c in path.chars() {
        match c {
            '\\' | '*' | '?' | '[' | ']' => {
                out.push('\\');
                out.push(c);
            }
            ',' => out.push('?'),
            _ => out.push(c),
        }
    }
    out
}

/// A line of the file `GIT_LFS_PROGRESS` names: `download 2/7 32768/9000000 assets/intro.mov`.
fn progress_line(line: &str) -> Option<(&str, u64, u64)> {
    let mut words = line.splitn(4, ' ');
    if words.next()? != "download" {
        return None;
    }
    let _count = words.next()?;
    let (done, total) = words.next()?.split_once('/')?;
    Some((words.next()?, done.parse().ok()?, total.parse().ok()?))
}

/// What git-lfs said about one file it couldn't download: its line names the object,
/// `[4d7a…] Object does not exist on the server: [404] Object does not exist`.
fn error_for(output: &str, oid: &str) -> Option<String> {
    let hex = oid.strip_prefix("sha256:").unwrap_or(oid);
    output
        .lines()
        .find(|line| line.contains(hex))
        .map(|line| line.replace(&format!("[{hex}] "), "").trim().to_owned())
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
            plan_pull(None, &[])
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
        let version = self.lfs_version();
        let text = read_attributes(self.workdir()).unwrap_or_default();
        let wanted = lfs_patterns(&text);
        let committed = if wanted.is_empty() {
            Vec::new()
        } else {
            self.run(&GitCommand::new(["cat-file", "-p", "HEAD:.gitattributes"]))
                .map(|out| lfs_patterns(&out.stdout))
                .unwrap_or_default()
        };
        let mut patterns = Vec::new();
        let mut missing_files = Vec::new();
        let mut listed = HashSet::new();
        for pattern in wanted {
            let out = self.run(&GitCommand::new(["ls-files", "-z", "--", &pathspec(&pattern)]))?;
            let mut entry = LfsPattern {
                new: !committed.contains(&pattern),
                pattern,
                files: 0,
                size: 0,
                missing: 0,
            };
            for path in out.stdout.split('\0').filter(|p| !p.is_empty()) {
                let Some((size, pointer)) = self.lfs_file(path) else {
                    continue;
                };
                entry.files += 1;
                entry.size += size;
                if let Some(pointer) = pointer {
                    entry.missing += 1;
                    if missing_files.len() < MAX_LISTED && listed.insert(path.to_owned()) {
                        missing_files.push(LfsFile {
                            path: path.to_owned(),
                            size,
                            oid: pointer.oid,
                        });
                    }
                }
            }
            patterns.push(entry);
        }
        missing_files.sort_by(|a, b| a.path.cmp(&b.path));
        let filters = self
            .run(&GitCommand::new(["config", "--get", "filter.lfs.process"]))
            .is_ok_and(|out| !out.stdout.trim().is_empty());
        Ok(LfsStatus {
            version,
            filters,
            patterns,
            brew: brew().map(|p| p.display().to_string()),
            missing_files,
        })
    }

    /// The version of git-lfs, `None` when it is not installed. Once found it is not asked again
    /// while the app runs: starting git-lfs on every reload costs more than the rest of the status.
    /// Not having it is asked again each time, so installing it in a terminal shows at once.
    fn lfs_version(&self) -> Option<String> {
        static FOUND: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
        if let Some(version) = FOUND.lock().ok().and_then(|found| found.clone()) {
            return Some(version);
        }
        // Asked outside the repository: where git-lfs is missing, git would run an alias `lfs`
        // from the repository's own .git/config instead, and only reading must not run that.
        let version = crate::cli::run_in(&std::env::temp_dir(), &GitCommand::new(["lfs", "version"]), None)
            .ok()
            .map(|out| {
                let first = out.stdout.split_whitespace().next().unwrap_or_default().to_owned();
                first.strip_prefix("git-lfs/").map_or(first.clone(), str::to_owned)
            })?;
        if let Ok(mut found) = FOUND.lock() {
            *found = Some(version.clone());
        }
        Some(version)
    }

    /// The size of a file in the working copy, and whether it is only a pointer; `None` when it
    /// is not there.
    pub(crate) fn lfs_file_size(&self, path: &str) -> Option<(u64, bool)> {
        self.lfs_file(path).map(|(size, pointer)| (size, pointer.is_some()))
    }

    /// The size of a file in the working copy, and its pointer while it is only that.
    fn lfs_file(&self, path: &str) -> Option<(u64, Option<LfsPointer>)> {
        let file = self.workdir().join(path);
        // A symlink is a link here, as for git, not the file it points at.
        let meta = std::fs::symlink_metadata(&file).ok()?;
        if !meta.is_file() {
            return None;
        }
        let len = meta.len();
        if len > MAX_POINTER {
            return Some((len, None));
        }
        let data = read_regular(&file, MAX_POINTER)?;
        Some(match parse_pointer(&data) {
            Some(pointer) => (pointer.size, Some(pointer)),
            None => (len, None),
        })
    }

    /// The files `include` matches, or all LFS files, that are still pointers here.
    fn lfs_missing(&self, include: Option<&str>) -> Result<Vec<LfsFile>> {
        let patterns = match include {
            Some(pattern) => vec![pattern.to_owned()],
            None => lfs_patterns(&read_attributes(self.workdir()).unwrap_or_default()),
        };
        let mut files: Vec<LfsFile> = Vec::new();
        let mut seen = HashSet::new();
        for pattern in patterns {
            let out = self.run(&GitCommand::new(["ls-files", "-z", "--", &pathspec(&pattern)]))?;
            for path in out.stdout.split('\0').filter(|p| !p.is_empty()) {
                if let Some((size, Some(pointer))) = self.lfs_file(path)
                    && seen.insert(path.to_owned())
                {
                    files.push(LfsFile {
                        path: path.to_owned(),
                        size,
                        oid: pointer.oid,
                    });
                }
            }
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }

    /// Download LFS files in place of their pointers: those at `paths`, else those `include`
    /// matches, else all. Each file is a part of the action, with how much of it came; one
    /// that fails doesn't stop the others, and when any is still a pointer after, the answer
    /// is [`Error::PartlyDone`] with what git-lfs said about it.
    pub(crate) fn lfs_download(
        &self,
        include: Option<&str>,
        paths: &[String],
        on_event: &mut dyn FnMut(ActionEvent),
        cancel: &AtomicBool,
    ) -> Result<String> {
        let files: Vec<LfsFile> = if paths.is_empty() {
            self.lfs_missing(include)?
        } else {
            paths
                .iter()
                .filter_map(|path| match self.lfs_file(path) {
                    Some((size, Some(pointer))) => Some(LfsFile {
                        path: path.clone(),
                        size,
                        oid: pointer.oid,
                    }),
                    _ => None,
                })
                .collect()
        };
        let named: Vec<String> = files.iter().map(|f| f.path.clone()).collect();
        let progress = self.git_dir().join("oxbow").join("lfs-progress");
        // Without it the files still come, only without their progress.
        if let Some(dir) = progress.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&progress, "");
        let command = plan_pull(include, if paths.is_empty() { &[] } else { &named })
            .remove(0)
            .env("GIT_LFS_PROGRESS", progress.display().to_string())
            .env("GIT_LFS_FORCE_PROGRESS", "1");
        let command = crate::ssh::interactive(command);
        if !files.is_empty() {
            on_event(ActionEvent::Parts { names: named.clone() });
        }
        on_event(ActionEvent::Command {
            display: command.display(),
        });

        // What git-lfs wrote to the progress file since the last look, a file at a time.
        let wanted: HashSet<&str> = named.iter().map(String::as_str).collect();
        let mut read = ProgressReader::default();
        let mut started = HashSet::new();
        let mut finished = HashSet::new();
        let mut look = |on_event: &mut dyn FnMut(ActionEvent)| {
            for (path, done, total) in read.next(&progress) {
                if !wanted.contains(path.as_str()) {
                    continue;
                }
                if started.insert(path.clone()) {
                    on_event(ActionEvent::Part {
                        name: path.clone(),
                        state: PartState::Started,
                        error: None,
                        inner: None,
                    });
                }
                // All of it came: git-lfs puts it in place once the others are there too.
                if done == total && finished.insert(path.clone()) {
                    on_event(ActionEvent::Part {
                        name: path,
                        state: PartState::Done,
                        error: None,
                        inner: None,
                    });
                } else if !finished.contains(&path) {
                    on_event(ActionEvent::PartProgress {
                        name: path,
                        done,
                        total,
                    });
                }
            }
        };
        let ran = crate::console::main(|| {
            self.run_streaming(
                &command,
                &mut |line| {
                    on_event(ActionEvent::Line(line));
                    look(&mut *on_event);
                },
                cancel,
            )
        });
        let _ = std::fs::remove_file(&progress);
        let output = match &ran {
            Ok(out) => format!("{}\n{}", out.stdout, out.stderr),
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(Error::Command { output, .. }) => output.clone(),
            Err(other) => other.to_string(),
        };
        let mut failed = Vec::new();
        for file in &files {
            let still = matches!(self.lfs_file(&file.path), Some((_, Some(_))));
            let error = still.then(|| {
                error_for(&output, &file.oid).unwrap_or_else(|| match &ran {
                    Ok(_) => "git-lfs left it a pointer".to_owned(),
                    Err(_) => output.trim().to_owned(),
                })
            });
            if !still && finished.contains(&file.path) {
                continue;
            }
            on_event(ActionEvent::Part {
                name: file.path.clone(),
                state: if still { PartState::Failed } else { PartState::Done },
                error: error.clone(),
                inner: None,
            });
            if let Some(error) = error {
                failed.push(FailedPart {
                    name: file.path.clone(),
                    error,
                    inner: None,
                });
            }
        }
        match ran {
            _ if !failed.is_empty() => Err(Error::PartlyDone {
                of: files.len(),
                failed,
            }),
            Ok(out) => Ok(if out.stdout.trim().is_empty() {
                out.stderr
            } else {
                out.stdout
            }),
            Err(err) => Err(err),
        }
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

/// Reads the lines git-lfs adds to its progress file, from where the last read stopped; a
/// line it is still writing waits for the next read.
#[derive(Default)]
struct ProgressReader {
    offset: u64,
    partial: String,
}

impl ProgressReader {
    /// The newest numbers for each file that moved since the last read, in the order they came.
    fn next(&mut self, file: &Path) -> Vec<(String, u64, u64)> {
        let Ok(mut f) = std::fs::File::open(file) else {
            return Vec::new();
        };
        let mut text = String::new();
        if f.seek(SeekFrom::Start(self.offset)).is_err() || f.read_to_string(&mut text).is_err() {
            return Vec::new();
        }
        self.offset += text.len() as u64;
        self.partial.push_str(&text);
        let Some(end) = self.partial.rfind('\n') else {
            return Vec::new();
        };
        let complete: String = self.partial.drain(..=end).collect();
        let mut newest: Vec<(String, u64, u64)> = Vec::new();
        let mut at: HashMap<String, usize> = HashMap::new();
        for (path, done, total) in complete.lines().filter_map(progress_line) {
            match at.get(path) {
                Some(&i) => newest[i] = (path.to_owned(), done, total),
                None => {
                    at.insert(path.to_owned(), newest.len());
                    newest.push((path.to_owned(), done, total));
                }
            }
        }
        newest
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
    fn a_download_names_each_file_so_it_matches_only_that_one() {
        let commands = plan_pull(None, &["f1.bin".into(), "st*r [1].bin".into(), "a,b/c.bin".into()]);
        assert_eq!(commands[0].args[2], r"--include=/f1.bin,/st\*r \[1\].bin,/a?b/c.bin");
        // Too many to name: all of them, which skips what is there already.
        let many: Vec<String> = (0..=MAX_NAMED).map(|i| format!("{i}.bin")).collect();
        assert_eq!(
            plan_pull(Some("*.bin"), &many)[0].display(),
            "git lfs pull '--include=*.bin'"
        );
    }

    #[test]
    fn reads_the_progress_file_a_line_at_a_time() {
        assert_eq!(
            progress_line("download 2/7 32768/9000000 d,x/a b.bin"),
            Some(("d,x/a b.bin", 32768, 9000000))
        );
        assert_eq!(progress_line("clean 1/1 1024/3000000 f1.bin"), None);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("progress");
        let mut read = ProgressReader::default();
        std::fs::write(
            &file,
            "download 1/2 10/100 a.bin\ndownload 1/2 50/100 a.bin\ndownload 2/2 5/",
        )
        .unwrap();
        assert_eq!(read.next(&file), [("a.bin".to_owned(), 50, 100)]);
        let mut f = std::fs::OpenOptions::new().append(true).open(&file).unwrap();
        std::io::Write::write_all(&mut f, b"9 b.bin\n").unwrap();
        assert_eq!(read.next(&file), [("b.bin".to_owned(), 5, 9)]);
        assert_eq!(read.next(&file), []);
    }

    #[test]
    fn finds_what_git_lfs_said_about_a_file() {
        let output = "Downloading LFS objects: 100% (7/7)\n[0f1cee] Object does not exist: [404] Object does not exist\nFailed to fetch some objects from 'http://x/'";
        assert_eq!(
            error_for(output, "sha256:0f1cee").as_deref(),
            Some("Object does not exist: [404] Object does not exist")
        );
        assert_eq!(error_for(output, "sha256:abcdef"), None);
    }

    #[test]
    fn track_without_staging_leaves_the_index_alone() {
        let commands = plan_track("*.mov", &["assets/demo/intro.mov".into()], false);
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].display(), "git lfs track '*.mov'");
    }
}
