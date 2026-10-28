//! The history of one file: the commits that changed it (across renames), who last changed each
//! line (`git blame`), and every commit that touched one line (`git log -L`).

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use crate::cli::{GitCommand, literal};
use crate::commit::FileStatus;
use crate::error::{Error, Result};
use crate::repo::Repo;
use crate::worktree::path_field;

/// At most this many commits of a file are listed.
const LIMIT: usize = 2000;
/// Bytes of `git blame --porcelain` read at most: past them the blame stops at the lines read.
const MAX_BLAME_BYTES: usize = 8 * 1024 * 1024;
const FORMAT: &str = "--format=%x00%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%s";

/// A commit that changed the file, with the file's path in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileCommit {
    pub id: String,
    pub parents: Vec<String>,
    pub summary: String,
    pub author_name: String,
    pub author_email: String,
    pub time: i64,
    /// The file's path after this commit.
    pub path: String,
    /// Its path before, when this commit renamed or copied it.
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHistory {
    pub path: String,
    /// Commits of the revision that changed the file, newest first, following renames.
    pub commits: Vec<FileCommit>,
    /// Commits on other branches that changed it and aren't in the revision.
    pub elsewhere: Vec<FileCommit>,
    pub more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlameCommit {
    pub id: String,
    pub summary: String,
    pub author_name: String,
    pub author_email: String,
    pub time: i64,
    /// The file's path in this commit.
    pub path: String,
    /// The history was cut here (a shallow clone's oldest commit).
    pub boundary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlameLine {
    /// Index into [`Blame::commits`].
    pub commit: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Blame {
    /// The commit blamed at.
    pub rev: String,
    pub path: String,
    pub commits: Vec<BlameCommit>,
    /// One per line of the file, in order.
    pub lines: Vec<BlameLine>,
    pub command: String,
    /// The file is too long to blame whole: `lines` are its first ones.
    pub cut: bool,
    /// The list of commits blame skips, when the repository has one: found, used or not.
    pub ignore_revs: Option<IgnoreRevs>,
}

/// How to blame: Blame's two switches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlameOptions {
    /// `-w`: a line whose only change was whitespace keeps the commit before.
    pub ignore_whitespace: bool,
    /// Skip the commits of [`IGNORE_REVS_FILE`] (or `blame.ignoreRevsFile`): reformatting and
    /// the like, so a line keeps the commit that really changed it.
    pub ignore_revs: bool,
}

/// The file of commits blame skips, by the convention GitHub and VS Code follow.
pub const IGNORE_REVS_FILE: &str = ".git-blame-ignore-revs";

/// A list of commits blame skips that the repository has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IgnoreRevs {
    /// Its path, from the repository's top folder.
    pub file: String,
    /// How many commits it lists.
    pub commits: usize,
    /// This blame skipped them.
    pub used: bool,
    /// Why git couldn't read it, e.g. a line that is no commit: the blame went on without it.
    pub problem: Option<String>,
}

impl Repo {
    /// Every file in HEAD's tree, sorted by path: what File History can be opened on. Empty before
    /// the first commit.
    pub fn files(&self) -> Result<Vec<String>> {
        let repo = self.local();
        let Ok(commit) = repo.head_commit() else {
            return Ok(Vec::new());
        };
        let tree = commit.tree().map_err(Error::git)?;
        let entries = tree.traverse().breadthfirst.files().map_err(Error::git)?;
        let mut paths: Vec<String> = entries
            .into_iter()
            .filter(|e| e.mode.is_blob() || e.mode.is_link())
            .map(|e| e.filepath.to_string())
            .collect();
        paths.sort();
        Ok(paths)
    }

    /// The commits that changed `path`, from `rev` (HEAD when missing) back, across renames;
    /// and the ones on other branches.
    pub fn file_history(&self, path: &str, rev: Option<&str>) -> Result<FileHistory> {
        self.file_history_with(path, rev, &AtomicBool::new(false))
    }

    /// [`Repo::file_history`]; setting `cancel` stops it.
    pub fn file_history_with(&self, path: &str, rev: Option<&str>, cancel: &AtomicBool) -> Result<FileHistory> {
        let rev = self.commit_id(rev.unwrap_or("HEAD"))?;
        let mut commits = self.file_log(&["--follow".to_owned(), rev.clone()], path, cancel)?;
        let more = commits.len() > LIMIT;
        commits.truncate(LIMIT);
        let mut elsewhere = self.file_log(&["--all".to_owned(), "--not".to_owned(), rev], path, cancel)?;
        elsewhere.truncate(LIMIT);
        Ok(FileHistory {
            path: path.to_owned(),
            commits,
            elsewhere,
            more,
        })
    }

    /// `git log` of one file, with its path and added/removed line counts in each commit.
    fn file_log(&self, revs: &[String], path: &str, cancel: &AtomicBool) -> Result<Vec<FileCommit>> {
        // One walk gives both: `--raw` the status and paths (`:<modes> <ids> R075\told\tnew`),
        // `--numstat` the line counts after them.
        let mut args = vec![
            "log".to_owned(),
            "-M".to_owned(),
            format!("--max-count={}", LIMIT + 1),
            FORMAT.to_owned(),
            "--raw".to_owned(),
            "--numstat".to_owned(),
        ];
        args.extend(revs.iter().cloned());
        args.push("--".to_owned());
        args.push(literal(path));
        let out = self.run_cancellable(&GitCommand::new(args), cancel)?;
        Ok(chunks(&out.stdout)
            .filter_map(|(head, body)| {
                let raw = body.iter().find_map(|line| raw_status(line));
                let numbers = body.iter().find(|line| !line.starts_with(':')).map(|line| {
                    let mut fields = line.split('\t');
                    // Binary files count as "-".
                    let add: u32 = fields.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                    let del: u32 = fields.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                    (add, del)
                });
                let mut f = head.split('\u{1f}');
                let id = f.next()?.to_owned();
                let parents = f
                    .next()
                    .unwrap_or_default()
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect();
                let author_name = f.next().unwrap_or_default().to_owned();
                let author_email = f.next().unwrap_or_default().to_owned();
                let time = f.next().and_then(|t| t.parse().ok()).unwrap_or(0);
                // Last, so a \x1f in it stays in it.
                let summary = f.collect::<Vec<_>>().join("\u{1f}");
                // A merge lists no files of its own; it keeps the path of the commit after it.
                let (status, path, old_path) =
                    raw.map(name_status)
                        .unwrap_or((FileStatus::Modified, path.to_owned(), None));
                let (additions, deletions) = numbers.unwrap_or((0, 0));
                Some(FileCommit {
                    id,
                    parents,
                    summary,
                    author_name,
                    author_email,
                    time,
                    path,
                    old_path,
                    status,
                    additions,
                    deletions,
                })
            })
            .collect())
    }

    /// Who last changed each line of `path` as of `rev`.
    pub fn blame(&self, path: &str, rev: &str) -> Result<Blame> {
        self.blame_with(path, rev, &BlameOptions::default(), &AtomicBool::new(false))
    }

    /// The list of commits blame skips: `blame.ignoreRevsFile` when it is set and there, else
    /// [`IGNORE_REVS_FILE`] in the top folder. Its path from there, and how many it lists.
    pub fn ignore_revs_file(&self) -> Option<(String, usize)> {
        let top = self.workdir();
        let configured = self.config_value("blame.ignoreRevsFile").filter(|f| !f.is_empty());
        [configured.as_deref(), Some(IGNORE_REVS_FILE)]
            .into_iter()
            .flatten()
            .find_map(|file| {
                let text = std::fs::read_to_string(top.join(file)).ok()?;
                let commits = text
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .count();
                Some((file.to_owned(), commits))
            })
    }

    /// [`Repo::blame`] with `options`; setting `cancel` stops it. A file whose blame is longer
    /// than [`MAX_BLAME_BYTES`] is blamed as far as that, and comes `cut`.
    pub fn blame_with(&self, path: &str, rev: &str, options: &BlameOptions, cancel: &AtomicBool) -> Result<Blame> {
        let rev = self.commit_id(rev)?;
        let found = self.ignore_revs_file();
        let list = found
            .as_ref()
            .filter(|_| options.ignore_revs)
            .map(|(file, _)| file.clone());
        let list = list.as_deref();
        let mut problem = None;
        let (out, command) = match self.run_blame(path, &rev, options.ignore_whitespace, list, cancel) {
            // A list git can't read (a line that isn't a commit) leaves the blame without it.
            Err(Error::Command { output, .. })
                if list.is_some() && !cancel.load(std::sync::atomic::Ordering::Relaxed) =>
            {
                problem = Some(output.trim().trim_start_matches("fatal: ").to_owned());
                self.run_blame(path, &rev, options.ignore_whitespace, None, cancel)?
            }
            other => other?,
        };
        let text = String::from_utf8_lossy(&out.stdout);
        // A cut blame ends with the last line read whole: its text line comes last.
        let whole = if out.cut { whole_lines(&text) } else { &text };
        let (commits, lines) = parse_porcelain(whole);
        Ok(Blame {
            command,
            rev,
            path: path.to_owned(),
            commits,
            lines,
            cut: out.cut,
            ignore_revs: found.map(|(file, commits)| IgnoreRevs {
                file,
                commits,
                used: list.is_some() && problem.is_none(),
                problem,
            }),
        })
    }

    /// `git blame --porcelain` of `path` at `rev`, skipping the commits of `list`, and the command
    /// as the user would type it. The files git's own settings name are left out first: a
    /// missing one would stop git, and only the list Oxbow found and shows is used.
    fn run_blame(
        &self,
        path: &str,
        rev: &str,
        whitespace: bool,
        list: Option<&str>,
        cancel: &AtomicBool,
    ) -> Result<(crate::cli::Limited, String)> {
        let mut flags: Vec<String> = Vec::new();
        if whitespace {
            flags.push("-w".to_owned());
        }
        let mut shown = flags.clone();
        flags.push("--no-ignore-revs-file".to_owned());
        if let Some(file) = list {
            flags.extend(["--ignore-revs-file".to_owned(), file.to_owned()]);
            shown.extend(["--ignore-revs-file".to_owned(), file.to_owned()]);
        }
        let args = |porcelain: bool, flags: &[String], rev: &str| {
            let mut args = vec!["blame".to_owned()];
            if porcelain {
                args.push("--porcelain".to_owned());
            }
            args.extend(flags.iter().cloned());
            args.extend([rev.to_owned(), "--".to_owned(), path.to_owned()]);
            args
        };
        let command = GitCommand::new(args(true, &flags, rev));
        let out = self.spawn_cancellable_within(&command, MAX_BLAME_BYTES, cancel)?;
        if !out.success && !out.cut {
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            return Err(Error::Command {
                command: command.display(),
                code: out.code,
                output: if stderr.trim().is_empty() {
                    String::from_utf8_lossy(&out.stdout).into_owned()
                } else {
                    stderr
                },
            });
        }
        Ok((out, GitCommand::new(args(false, &shown, &rev[..7])).display()))
    }

    /// The commits that changed line `line` of `path`, as of `rev`, newest first (`git log -L`).
    pub fn line_history(&self, path: &str, line: u32, rev: &str) -> Result<Vec<String>> {
        self.line_history_with(path, line, rev, &AtomicBool::new(false))
    }

    /// [`Repo::line_history`]; setting `cancel` stops it.
    pub fn line_history_with(&self, path: &str, line: u32, rev: &str, cancel: &AtomicBool) -> Result<Vec<String>> {
        let rev = self.commit_id(rev)?;
        let out = self.run_cancellable(
            &GitCommand::new([
                "log".to_owned(),
                "--no-patch".to_owned(),
                "--format=%H".to_owned(),
                format!("-L{line},{line}:{path}"),
                rev,
            ]),
            cancel,
        )?;
        Ok(out
            .stdout
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect())
    }

    /// The commits that added or removed `text` in `path` (`git log -S`), from `rev` back across
    /// renames, then on other branches.
    pub fn file_pickaxe(&self, path: &str, text: &str, match_case: bool, rev: &str) -> Result<Vec<String>> {
        self.file_pickaxe_with(path, text, match_case, rev, &AtomicBool::new(false))
    }

    /// [`Repo::file_pickaxe`]; setting `cancel` stops it.
    pub fn file_pickaxe_with(
        &self,
        path: &str,
        text: &str,
        match_case: bool,
        rev: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<String>> {
        let rev = self.commit_id(rev)?;
        let run = |revs: &[&str]| -> Result<Vec<String>> {
            let mut args = vec!["log".to_owned(), "--format=%H".to_owned(), format!("-S{text}")];
            if !match_case {
                args.push("-i".to_owned());
            }
            args.extend(revs.iter().map(|r| (*r).to_owned()));
            args.push("--".to_owned());
            args.push(literal(path));
            let out = self.run_cancellable(&GitCommand::new(args), cancel)?;
            Ok(out
                .stdout
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect())
        };
        let mut ids = run(&["--follow", &rev])?;
        ids.extend(run(&["--all", "--not", &rev])?);
        Ok(ids)
    }
}

/// Commits of `git log` output whose format starts with NUL: the format line and the lines after
/// it. A subject never holds NUL, so it can't start a commit of its own.
fn chunks(out: &str) -> impl Iterator<Item = (&str, Vec<&str>)> {
    out.split('\0').filter_map(|chunk| {
        let mut lines = chunk.lines();
        let head = lines.next()?.trim();
        if head.is_empty() {
            return None;
        }
        // Only the line ends go: a path may end in a space.
        Some((
            head,
            lines
                .map(|l| l.trim_end_matches('\r'))
                .filter(|l| !l.trim().is_empty())
                .collect(),
        ))
    })
}

/// The `M\tpath` part of a `--raw` line: `:100644 100644 <old id> <new id> M\tpath`.
fn raw_status(line: &str) -> Option<&str> {
    let rest = line.strip_prefix(':')?;
    // The modes and ids hold no spaces; the status and paths come after the fourth.
    rest.splitn(5, ' ').nth(4)
}

/// `M\tpath`, `A\tpath`, `R075\told\tnew`.
fn name_status(line: &str) -> (FileStatus, String, Option<String>) {
    let mut fields = line.split('\t');
    let code = fields.next().unwrap_or_default();
    let first = path_field(fields.next().unwrap_or_default());
    match (code.chars().next(), fields.next().map(path_field)) {
        (Some('R'), Some(new)) => (FileStatus::Renamed, new, Some(first)),
        (Some('C'), Some(new)) => (FileStatus::Copied, new, Some(first)),
        (Some('A'), _) => (FileStatus::Added, first, None),
        (Some('D'), _) => (FileStatus::Deleted, first, None),
        _ => (FileStatus::Modified, first, None),
    }
}

/// The part of cut `git blame --porcelain` output up to its last whole line of the file: each
/// line of the file is a line starting with a tab.
fn whole_lines(text: &str) -> &str {
    let mut end = text.len();
    while let Some(at) = text[..end].rfind("\n\t") {
        if let Some(stop) = text[at + 2..].find('\n') {
            return &text[..at + 3 + stop];
        }
        end = at;
    }
    ""
}

fn parse_porcelain(out: &str) -> (Vec<BlameCommit>, Vec<BlameLine>) {
    let mut commits: Vec<BlameCommit> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut lines = Vec::new();
    let mut current = 0;
    for line in out.lines() {
        if let Some(text) = line.strip_prefix('\t') {
            lines.push(BlameLine {
                commit: current,
                text: text.to_owned(),
            });
            continue;
        }
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        if key.len() == 40 && key.bytes().all(|b| b.is_ascii_hexdigit()) {
            current = *index.entry(key.to_owned()).or_insert_with(|| {
                commits.push(BlameCommit {
                    id: key.to_owned(),
                    summary: String::new(),
                    author_name: String::new(),
                    author_email: String::new(),
                    time: 0,
                    path: String::new(),
                    boundary: false,
                });
                commits.len() - 1
            });
            continue;
        }
        let Some(c) = commits.get_mut(current) else { continue };
        match key {
            "author" => c.author_name = value.to_owned(),
            "author-mail" => c.author_email = value.trim_matches(['<', '>']).to_owned(),
            "author-time" => c.time = value.parse().unwrap_or(0),
            "summary" => c.summary = value.to_owned(),
            "filename" => c.path = path_field(value),
            "boundary" => c.boundary = true,
            _ => {}
        }
    }
    (commits, lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_lines_share_their_commit() {
        let out = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 1 1 2\nauthor Ann\nauthor-mail <ann@x>\nauthor-time 5\nsummary Add\nfilename a.rs\n\tone\naaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 2 2\n\ttwo\nbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb 3 3 1\nauthor Bob\nauthor-mail <bob@x>\nauthor-time 9\nsummary Fix\nprevious aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa a.rs\nfilename a.rs\n\tthree\n";
        let (commits, lines) = parse_porcelain(out);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[1].author_name, "Bob");
        assert_eq!(commits[0].author_email, "ann@x");
        let owners: Vec<_> = lines.iter().map(|l| (l.commit, l.text.as_str())).collect();
        assert_eq!(owners, [(0, "one"), (0, "two"), (1, "three")]);
    }

    #[test]
    fn renames_name_both_paths() {
        assert_eq!(
            name_status("R075\tsrc/settings.rs\tsrc/config.rs"),
            (
                FileStatus::Renamed,
                "src/config.rs".into(),
                Some("src/settings.rs".into())
            )
        );
        assert_eq!(name_status("A\tsrc/a.rs"), (FileStatus::Added, "src/a.rs".into(), None));
    }
}
