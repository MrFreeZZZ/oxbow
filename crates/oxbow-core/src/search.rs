//! Searching the history: commit messages, code changes (`git log -S`), authors and file paths.
//!
//! The search runs `git log` so it finds what git finds, and the command it runs is shown, so a
//! search in Oxbow teaches the same search in a terminal.

use serde::{Deserialize, Serialize};

use crate::cli::GitCommand;
use crate::error::Result;
use crate::repo::Repo;

/// At most this many commits come back; the count says when there are more.
const LIMIT: usize = 5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchMode {
    /// The commit message (`--grep`).
    Message,
    /// Changes that add or remove the text (`-S`, the pickaxe).
    Code,
    /// Author name or email (`--author`).
    Author,
    /// Commits that touch a path containing the text.
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    pub mode: SearchMode,
    pub text: String,
    /// Only commits of this branch; every branch, tag and stash when missing.
    #[serde(default)]
    pub branch: Option<String>,
    /// Only commits after this date, as `git log --since` reads it (`1.week.ago`).
    #[serde(default)]
    pub since: Option<String>,
    /// Only commits by this author, besides the search itself.
    #[serde(default)]
    pub author: Option<String>,
}

/// A commit that matched, and for code and file searches the files that did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub id: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    /// Newest first, as `git log` lists them.
    pub hits: Vec<SearchHit>,
    /// More commits matched than came back.
    pub more: bool,
    /// The search as one would type it: `git log --all --oneline -S RateLimit`.
    pub command: String,
}

impl SearchQuery {
    /// The arguments that pick the commits, shared by the command that runs and the one shown.
    fn filters(&self) -> Vec<String> {
        let mut args = vec![self.branch.clone().unwrap_or_else(|| "--all".to_owned())];
        if let Some(since) = &self.since {
            args.push(format!("--since={since}"));
        }
        // -F: the text is plain text, not a pattern; -i: any case. They apply to --grep and
        // --author alike.
        let fixed = matches!(self.mode, SearchMode::Message | SearchMode::Author) || self.author.is_some();
        if fixed {
            args.push("-i".to_owned());
            args.push("-F".to_owned());
        }
        if let Some(author) = &self.author {
            args.push(format!("--author={author}"));
        }
        match self.mode {
            SearchMode::Message => args.push(format!("--grep={}", self.text)),
            SearchMode::Author => args.push(format!("--author={}", self.text)),
            SearchMode::Code => {
                args.push("-S".to_owned());
                args.push(self.text.clone());
            }
            SearchMode::File => {}
        }
        args
    }

    fn pathspec(&self) -> Option<String> {
        (self.mode == SearchMode::File).then(|| format!(":(icase)*{}*", self.text))
    }

    /// What the search looks like typed in a terminal.
    pub fn command(&self) -> GitCommand {
        let mut args = vec!["log".to_owned()];
        args.extend(self.filters());
        args.insert(2, "--oneline".to_owned());
        if let Some(path) = self.pathspec() {
            args.push("--".to_owned());
            args.push(path);
        }
        GitCommand::new(args)
    }
}

impl Repo {
    pub fn search(&self, query: &SearchQuery) -> Result<SearchResult> {
        let command = query.command().display();
        if query.text.trim().is_empty() {
            return Ok(SearchResult {
                hits: Vec::new(),
                more: false,
                command,
            });
        }
        let files = matches!(query.mode, SearchMode::Code | SearchMode::File);
        let mut args = vec!["log".to_owned()];
        args.extend(query.filters());
        args.push(format!("--max-count={}", LIMIT + 1));
        // \x01 starts each commit, so the file names after it can be told apart.
        args.push("--format=%x01%H".to_owned());
        if files {
            args.push("--name-only".to_owned());
        }
        if let Some(path) = query.pathspec() {
            args.push("--".to_owned());
            args.push(path);
        }
        let out = self.run(&GitCommand::new(args))?;
        let mut hits = parse_log(&out.stdout);
        // File search: only the files whose path matched, not every file of the commit.
        if query.mode == SearchMode::File {
            let needle = query.text.to_lowercase();
            for hit in &mut hits {
                hit.files.retain(|f| f.to_lowercase().contains(&needle));
            }
            // A merge lists no files of its own, so it isn't one that changed them.
            hits.retain(|hit| !hit.files.is_empty());
        }
        let more = hits.len() > LIMIT;
        hits.truncate(LIMIT);
        Ok(SearchResult { hits, more, command })
    }
}

fn parse_log(out: &str) -> Vec<SearchHit> {
    out.split('\u{1}')
        .filter_map(|chunk| {
            let mut lines = chunk.lines().map(str::trim).filter(|l| !l.is_empty());
            let id = lines.next()?.to_owned();
            Some(SearchHit {
                id,
                files: lines.map(str::to_owned).collect(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(mode: SearchMode, text: &str) -> SearchQuery {
        SearchQuery {
            mode,
            text: text.into(),
            branch: None,
            since: None,
            author: None,
        }
    }

    #[test]
    fn commands_read_like_typed_ones() {
        assert_eq!(
            query(SearchMode::Message, "rate limit").command().display(),
            "git log --all --oneline -i -F '--grep=rate limit'"
        );
        assert_eq!(
            query(SearchMode::Code, "RateLimit").command().display(),
            "git log --all --oneline -S RateLimit"
        );
        let mut q = query(SearchMode::File, "banner");
        q.branch = Some("main".into());
        q.since = Some("1.week.ago".into());
        assert_eq!(
            q.command().display(),
            "git log main --oneline --since=1.week.ago -- ':(icase)*banner*'"
        );
    }

    #[test]
    fn log_with_files_is_split_by_commit() {
        let out = "\u{1}aaa\n\nsrc/a.rs\nsrc/b.rs\n\u{1}bbb\n";
        assert_eq!(
            parse_log(out),
            vec![
                SearchHit {
                    id: "aaa".into(),
                    files: vec!["src/a.rs".into(), "src/b.rs".into()]
                },
                SearchHit {
                    id: "bbb".into(),
                    files: vec![]
                },
            ]
        );
    }
}
