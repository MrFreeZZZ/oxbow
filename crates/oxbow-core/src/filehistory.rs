//! The history of one file: the commits that changed it (across renames), who last changed each
//! line (`git blame`), and every commit that touched one line (`git log -L`).

use std::collections::HashMap;

use serde::Serialize;

use crate::cli::{GitCommand, literal};
use crate::commit::FileStatus;
use crate::error::{Error, Result};
use crate::repo::Repo;

/// At most this many commits of a file are listed.
const LIMIT: usize = 2000;
const FORMAT: &str = "--format=%x01%H%x1f%P%x1f%s%x1f%an%x1f%ae%x1f%at";

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
        let rev = self.commit_id(rev.unwrap_or("HEAD"))?;
        let mut commits = self.file_log(&["--follow".to_owned(), rev.clone()], path)?;
        let more = commits.len() > LIMIT;
        commits.truncate(LIMIT);
        let mut elsewhere = self.file_log(&["--all".to_owned(), "--not".to_owned(), rev], path)?;
        elsewhere.truncate(LIMIT);
        Ok(FileHistory {
            path: path.to_owned(),
            commits,
            elsewhere,
            more,
        })
    }

    /// `git log` of one file, with its path and added/removed line counts in each commit.
    fn file_log(&self, revs: &[String], path: &str) -> Result<Vec<FileCommit>> {
        let run = |flag: &str| {
            let mut args = vec![
                "log".to_owned(),
                "-M".to_owned(),
                format!("--max-count={}", LIMIT + 1),
                FORMAT.to_owned(),
                flag.to_owned(),
            ];
            args.extend(revs.iter().cloned());
            args.push("--".to_owned());
            args.push(literal(path));
            self.run(&GitCommand::new(args))
        };
        let names = run("--name-status")?;
        let numbers = run("--numstat")?;
        let mut counts: HashMap<String, (u32, u32)> = HashMap::new();
        for (head, body) in chunks(&numbers.stdout) {
            let id = head.split('\u{1f}').next().unwrap_or_default().to_owned();
            if let Some(line) = body.first() {
                let mut fields = line.split('\t');
                // Binary files count as "-".
                let add = fields.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                let del = fields.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                counts.insert(id, (add, del));
            }
        }
        Ok(chunks(&names.stdout)
            .filter_map(|(head, body)| {
                let mut f = head.split('\u{1f}');
                let id = f.next()?.to_owned();
                let parents = f
                    .next()
                    .unwrap_or_default()
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect();
                let summary = f.next().unwrap_or_default().to_owned();
                let author_name = f.next().unwrap_or_default().to_owned();
                let author_email = f.next().unwrap_or_default().to_owned();
                let time = f.next().and_then(|t| t.parse().ok()).unwrap_or(0);
                // A merge lists no files of its own; it keeps the path of the commit after it.
                let (status, path, old_path) =
                    body.first()
                        .map(|line| name_status(line))
                        .unwrap_or((FileStatus::Modified, path.to_owned(), None));
                let (additions, deletions) = counts.get(&id).copied().unwrap_or((0, 0));
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
        let rev = self.commit_id(rev)?;
        let command = GitCommand::new(["blame", "--porcelain", &rev, "--", path]);
        let out = self.run(&command)?;
        let (commits, lines) = parse_porcelain(&out.stdout);
        Ok(Blame {
            command: GitCommand::new(["blame", &rev[..7], "--", path]).display(),
            rev,
            path: path.to_owned(),
            commits,
            lines,
        })
    }

    /// The commits that changed line `line` of `path`, as of `rev`, newest first (`git log -L`).
    pub fn line_history(&self, path: &str, line: u32, rev: &str) -> Result<Vec<String>> {
        let rev = self.commit_id(rev)?;
        let out = self.run(&GitCommand::new([
            "log".to_owned(),
            "--no-patch".to_owned(),
            "--format=%H".to_owned(),
            format!("-L{line},{line}:{path}"),
            rev,
        ]))?;
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
        let rev = self.commit_id(rev)?;
        let run = |revs: &[&str]| -> Result<Vec<String>> {
            let mut args = vec!["log".to_owned(), "--format=%H".to_owned(), format!("-S{text}")];
            if !match_case {
                args.push("-i".to_owned());
            }
            args.extend(revs.iter().map(|r| (*r).to_owned()));
            args.push("--".to_owned());
            args.push(literal(path));
            let out = self.run(&GitCommand::new(args))?;
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

/// Commits of `git log` output whose format starts with \x01: the format line and the lines after it.
fn chunks(out: &str) -> impl Iterator<Item = (&str, Vec<&str>)> {
    out.split('\u{1}').filter_map(|chunk| {
        let mut lines = chunk.lines();
        let head = lines.next()?.trim();
        if head.is_empty() {
            return None;
        }
        Some((head, lines.map(str::trim).filter(|l| !l.is_empty()).collect()))
    })
}

/// `M\tpath`, `A\tpath`, `R075\told\tnew`.
fn name_status(line: &str) -> (FileStatus, String, Option<String>) {
    let mut fields = line.split('\t');
    let code = fields.next().unwrap_or_default();
    let first = fields.next().unwrap_or_default().to_owned();
    match (code.chars().next(), fields.next()) {
        (Some('R'), Some(new)) => (FileStatus::Renamed, new.to_owned(), Some(first)),
        (Some('C'), Some(new)) => (FileStatus::Copied, new.to_owned(), Some(first)),
        (Some('A'), _) => (FileStatus::Added, first, None),
        (Some('D'), _) => (FileStatus::Deleted, first, None),
        _ => (FileStatus::Modified, first, None),
    }
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
            "filename" => c.path = value.to_owned(),
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
