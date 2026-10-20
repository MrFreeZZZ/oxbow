//! `git bisect`: finding the commit that broke something. git checks out commits between one
//! that worked and one that is broken; each answer (good, bad, or skip when a commit can't be
//! tested) halves what is left, until one commit remains: the first bad one.
//!
//! The state is git's own (`BISECT_START`, `BISECT_LOG`, `refs/bisect/*`), so a bisect started
//! in a terminal shows here too. Oxbow only remembers the stash it made before starting, to put
//! the uncommitted changes back when the bisect is done.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::cli::GitCommand;
use crate::error::Result;
use crate::repo::{RefKind, Repo};

/// Where Oxbow notes the stash it made before a bisect, inside the git directory.
const STASH_NOTE: &str = "oxbow/bisect-stash";

/// The message of that stash.
pub const STASH_MESSAGE: &str = "before bisect";

/// Commits listed as still possible; more than this and the graph just doesn't fade.
const MAX_CANDIDATES: usize = 20_000;

/// An answer about one commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BisectMark {
    /// It works at this commit: the first bad commit is newer.
    Good,
    /// It is broken at this commit: the first bad commit is this one or older.
    Bad,
    /// It can't be tested here, e.g. it doesn't build.
    Skip,
}

/// A bisect in progress, or one that found its commit and waits to be finished.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bisect {
    /// Where finishing goes back to: a branch, or a commit when `HEAD` was detached.
    pub start: String,
    pub start_is_branch: bool,
    /// The commits of the first good and bad answers, which set the range, and their names:
    /// a branch or tag there, else a short sha.
    pub first_good: Option<String>,
    pub first_good_label: Option<String>,
    pub first_bad: Option<String>,
    pub first_bad_label: Option<String>,
    /// Every commit answered good, bad or skip so far.
    pub good: Vec<String>,
    pub bad: Vec<String>,
    pub skipped: Vec<String>,
    /// Commits the first bad one may still be, the current bad one included. Empty before
    /// both a good and a bad commit are known, or when there are too many to list.
    pub candidates: Vec<String>,
    /// Answers given after the range was set.
    pub steps: usize,
    /// git's own numbers for the commit under test: revisions left after it, roughly how many
    /// more steps.
    pub left: usize,
    pub steps_left: usize,
    /// The commit under test (`HEAD`).
    pub testing: Option<String>,
    /// The first bad commit, once found.
    pub found: Option<String>,
    /// Only skipped commits are left: the first bad commit is one of these.
    pub possible: Vec<String>,
    /// What git printed for the last answer, as it would in a terminal.
    pub output: Vec<String>,
    /// `git bisect log`, to copy and replay.
    pub log: String,
    /// Oxbow stashed uncommitted changes before starting; finishing puts them back.
    pub stashed: bool,
    /// git's words for bad and good: `bad`/`good` unless the bisect was started with other terms.
    pub term_bad: String,
    pub term_good: String,
}

impl Repo {
    /// The bisect in progress, if any.
    pub fn bisect(&self) -> Result<Option<Bisect>> {
        let dir = self.git_dir();
        let Ok(start) = std::fs::read_to_string(dir.join("BISECT_START")) else {
            return Ok(None);
        };
        let start = start.trim().to_owned();
        let log = std::fs::read_to_string(dir.join("BISECT_LOG")).unwrap_or_default();
        let (term_bad, term_good) = std::fs::read_to_string(dir.join("BISECT_TERMS"))
            .ok()
            .and_then(|text| {
                let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
                Some((lines.next()?.to_owned(), lines.next()?.to_owned()))
            })
            .unwrap_or_else(|| ("bad".to_owned(), "good".to_owned()));

        let parsed = parse_log(&log, &term_bad, &term_good);
        let bisect_refs = self.bisect_refs(&term_bad, &term_good);
        let good = if bisect_refs.good.is_empty() {
            parsed.good.clone()
        } else {
            bisect_refs.good
        };
        let skipped = if bisect_refs.skipped.is_empty() {
            parsed.skipped.clone()
        } else {
            bisect_refs.skipped
        };
        let current_bad = bisect_refs.bad.or_else(|| parsed.bad.last().cloned());

        let refs = self.refs().unwrap_or_default();
        let name_of = |sha: &str| {
            [RefKind::Local, RefKind::Tag, RefKind::Remote]
                .iter()
                .find_map(|kind| refs.iter().find(|r| r.kind == *kind && r.target == sha))
                .map_or_else(|| short(sha), |r| r.name.clone())
        };
        let start_is_branch = !(start.len() == 40 && start.bytes().all(|b| b.is_ascii_hexdigit()));
        let testing = self.head().ok().and_then(|h| h.commit);

        let mut candidates = Vec::new();
        let (mut left, mut steps_left) = (0, 0);
        if let Some(bad) = current_bad.as_deref().filter(|_| !good.is_empty()) {
            let not_good = || good.iter().map(|g| format!("^{g}"));
            let range: Vec<String> = std::iter::once(bad.to_owned()).chain(not_good()).collect();
            let list = self
                .run(&GitCommand::new(
                    ["rev-list".to_owned(), format!("--max-count={}", MAX_CANDIDATES + 1)]
                        .into_iter()
                        .chain(range.iter().cloned()),
                ))
                .map(|out| out.stdout.lines().map(str::to_owned).collect::<Vec<_>>())
                .unwrap_or_default();
            if list.len() <= MAX_CANDIDATES {
                candidates = list;
            }
            if parsed.found.is_none() {
                let vars = self
                    .run(&GitCommand::new(
                        ["rev-list".to_owned(), "--bisect-vars".to_owned()]
                            .into_iter()
                            .chain(range),
                    ))
                    .map(|out| out.stdout)
                    .unwrap_or_default();
                let var = |name: &str| {
                    vars.lines()
                        .find_map(|line| line.strip_prefix(&format!("{name}=")))
                        .and_then(|v| v.trim_matches('\'').parse::<usize>().ok())
                        .unwrap_or(0)
                };
                left = var("bisect_nr");
                steps_left = var("bisect_steps");
            }
        }

        let output = self.bisect_output(&parsed, &name_of, testing.as_deref(), left, steps_left);
        Ok(Some(Bisect {
            start,
            start_is_branch,
            first_good_label: parsed.first_good.as_deref().map(name_of),
            first_good: parsed.first_good,
            first_bad_label: parsed.first_bad.as_deref().map(name_of),
            first_bad: parsed.first_bad,
            good,
            bad: parsed.bad,
            skipped,
            candidates,
            steps: parsed.steps,
            left,
            steps_left,
            testing,
            found: parsed.found,
            possible: parsed.possible,
            output,
            log,
            stashed: self.bisect_stash().is_some(),
            term_bad,
            term_good,
        }))
    }

    /// The commits `refs/bisect/*` point at: the bad one, the good ones, the skipped ones.
    fn bisect_refs(&self, term_bad: &str, term_good: &str) -> BisectRefs {
        let mut found = BisectRefs::default();
        let Ok(out) = self.run(&GitCommand::new([
            "for-each-ref",
            "--format=%(objectname) %(refname)",
            "refs/bisect/",
        ])) else {
            return found;
        };
        for (id, name) in out.stdout.lines().filter_map(|l| l.split_once(' ')) {
            let Some(name) = name.strip_prefix("refs/bisect/") else {
                continue;
            };
            if name == term_bad {
                found.bad = Some(id.to_owned());
            } else if name.starts_with(&format!("{term_good}-")) {
                found.good.push(id.to_owned());
            } else if name.starts_with("skip-") {
                found.skipped.push(id.to_owned());
            }
        }
        found
    }

    /// The last answer and what git said to it, rebuilt from the state so it also shows after
    /// a restart or for a bisect run in a terminal.
    fn bisect_output(
        &self,
        parsed: &ParsedLog,
        name_of: &dyn Fn(&str) -> String,
        testing: Option<&str>,
        left: usize,
        steps_left: usize,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some((word, sha)) = &parsed.last {
            lines.push(format!("$ git bisect {word} {}", name_of(sha)));
        }
        let plural = |n: usize, one: &str, many: &str| if n == 1 { one.to_owned() } else { many.to_owned() };
        if let Some(found) = &parsed.found {
            lines.push(format!("{} is the first {} commit", short(found), parsed.term_bad));
            if let Ok(out) = self.run(&GitCommand::new(["log", "-1", "--format=%an%x1f%s", found, "--"]))
                && let Some((author, subject)) = out.stdout.trim_end().split_once('\x1f')
            {
                lines.push(format!("Author: {author}"));
                lines.push(format!("    {subject}"));
            }
        } else if !parsed.possible.is_empty() {
            lines.push("There are only 'skip'ped commits left to test.".to_owned());
            lines.push(format!(
                "The first {} commit could be any of: {}",
                parsed.term_bad,
                parsed.possible.iter().map(|s| short(s)).collect::<Vec<_>>().join(", ")
            ));
        } else if parsed.ready
            && let Some(head) = testing
        {
            lines.push(format!(
                "Bisecting: {left} {} left to test after this (roughly {steps_left} {})",
                plural(left, "revision", "revisions"),
                plural(steps_left, "step", "steps"),
            ));
            if let Ok(out) = self.run(&GitCommand::new(["log", "-1", "--format=%s", head, "--"])) {
                lines.push(format!("[{}] {}", short(head), out.stdout.trim()));
            }
        } else if parsed.bad.is_empty() {
            lines.push(format!("status: waiting for a {} commit", parsed.term_bad));
        } else {
            lines.push(format!(
                "status: waiting for {} commit(s), {} commit known",
                parsed.term_good, parsed.term_bad
            ));
        }
        lines
    }

    /// The stash Oxbow made before the bisect, while it is still in the stash list.
    pub(crate) fn bisect_stash(&self) -> Option<String> {
        let id = std::fs::read_to_string(self.git_dir().join(STASH_NOTE)).ok()?;
        let id = id.trim();
        self.stash_name(id).map(|_| id.to_owned())
    }

    /// Note the stash just made before a bisect, when there is a new one since `before`.
    pub(crate) fn note_bisect_stash(&self, before: Option<&str>) {
        let now = self
            .run(&GitCommand::new(["rev-parse", "-q", "--verify", "refs/stash"]))
            .ok()
            .map(|out| out.stdout.trim().to_owned());
        if let Some(now) = now.filter(|now| Some(now.as_str()) != before) {
            let file = self.git_dir().join(STASH_NOTE);
            if let Some(parent) = file.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(file, format!("{now}\n"));
        }
    }

    /// Forget the stash note once no bisect is going on any more.
    pub(crate) fn forget_bisect_stash(&self) {
        if !self.git_dir().join("BISECT_START").exists() {
            let _ = std::fs::remove_file(self.git_dir().join(STASH_NOTE));
        }
    }

    /// `refs/stash` now, to tell whether a stash push made a new one.
    pub(crate) fn stash_tip(&self) -> Option<String> {
        self.run(&GitCommand::new(["rev-parse", "-q", "--verify", "refs/stash"]))
            .ok()
            .map(|out| out.stdout.trim().to_owned())
    }

    /// Start bisecting between `good` and `bad`, after stashing uncommitted changes with
    /// `stash`; with `command`, git runs it at every step and answers by its exit code.
    pub fn plan_bisect_start(&self, good: &str, bad: &str, stash: bool, command: Option<&str>) -> Vec<GitCommand> {
        let mut commands = Vec::new();
        if stash {
            commands.push(
                GitCommand::new(["stash", "push", "--include-untracked", "-m", STASH_MESSAGE])
                    .comment("a clean working copy for every step; Finish brings the changes back"),
            );
        }
        commands.push(GitCommand::new(["bisect", "start"]));
        commands.push(GitCommand::new(["bisect", "bad", bad]).comment("broken here"));
        commands.push(
            GitCommand::new(["bisect", "good", good]).comment("worked here; git checks out the commit in the middle"),
        );
        if let Some(command) = command.map(str::trim).filter(|c| !c.is_empty()) {
            commands.push(
                GitCommand::new(["bisect", "run", "sh", "-c", command])
                    .comment("exit code 0 means good, 125 skip, anything else bad"),
            );
        }
        commands
    }

    /// Answer for `commit`, or for the commit under test.
    pub fn plan_bisect_mark(&self, mark: BisectMark, commit: Option<&str>) -> Result<Vec<GitCommand>> {
        let state = self.bisect()?;
        let (term_bad, term_good) = state
            .as_ref()
            .map_or(("bad", "good"), |b| (b.term_bad.as_str(), b.term_good.as_str()));
        let (word, comment) = match mark {
            BisectMark::Good => (term_good, "it works here, so the first bad commit is newer"),
            BisectMark::Bad => (term_bad, "broken here, so the first bad commit is this one or older"),
            BisectMark::Skip => ("skip", "can’t test this one; git picks a commit next to it"),
        };
        let mut args = vec!["bisect".to_owned(), word.to_owned()];
        args.extend(commit.map(str::to_owned));
        Ok(vec![GitCommand::new(args).comment(comment)])
    }

    /// Stop bisecting: back to where it started, and the stashed changes back with it.
    pub fn plan_bisect_reset(&self) -> Result<Vec<GitCommand>> {
        let back = self.bisect()?.map(|b| b.start);
        let mut commands = vec![GitCommand::new(["bisect", "reset"]).comment(match &back {
            Some(start) if start.len() == 40 => format!("back to {}, where it started", short(start)),
            Some(start) => format!("back to {start}, where it started"),
            None => "back to where it started".to_owned(),
        })];
        if let Some(id) = self.bisect_stash()
            && let Some(name) = self.stash_name(&id)
        {
            commands.push(
                GitCommand::new(["stash", "pop", &name]).comment("your uncommitted changes from before the bisect"),
            );
        }
        Ok(commands)
    }
}

#[derive(Default)]
struct BisectRefs {
    bad: Option<String>,
    good: Vec<String>,
    skipped: Vec<String>,
}

/// What `BISECT_LOG` says.
#[derive(Debug, Default, PartialEq, Eq)]
struct ParsedLog {
    good: Vec<String>,
    bad: Vec<String>,
    skipped: Vec<String>,
    first_good: Option<String>,
    first_bad: Option<String>,
    /// Both a good and a bad commit are known.
    ready: bool,
    /// Answers after that.
    steps: usize,
    /// The last answer: git's word for it and the commit.
    last: Option<(String, String)>,
    found: Option<String>,
    possible: Vec<String>,
    term_bad: String,
    term_good: String,
}

fn parse_log(log: &str, term_bad: &str, term_good: &str) -> ParsedLog {
    let mut parsed = ParsedLog {
        term_bad: term_bad.to_owned(),
        term_good: term_good.to_owned(),
        ..ParsedLog::default()
    };
    let bracketed = |rest: &str| {
        rest.trim()
            .strip_prefix('[')
            .and_then(|r| r.split_once(']'))
            .map(|(sha, _)| sha.to_owned())
    };
    // Newer git puts the term in quotes: "# first 'bad' commit:".
    let result = |line: &str, lead: &str| {
        [
            format!("{lead} {term_bad} commit:"),
            format!("{lead} '{term_bad}' commit:"),
        ]
        .iter()
        .find_map(|prefix| line.strip_prefix(prefix.as_str()))
        .map(str::to_owned)
    };
    for line in log.lines() {
        if let Some(rest) = result(line, "# first") {
            parsed.found = bracketed(&rest);
            continue;
        }
        if let Some(rest) = result(line, "# possible first") {
            parsed.possible.extend(bracketed(&rest));
            continue;
        }
        let Some(rest) = line.strip_prefix("git bisect ") else {
            continue;
        };
        let mut words = rest.split_whitespace();
        let (Some(word), Some(sha)) = (words.next(), words.next()) else {
            continue;
        };
        let list = if word == term_good {
            parsed.first_good.get_or_insert_with(|| sha.to_owned());
            &mut parsed.good
        } else if word == term_bad {
            parsed.first_bad.get_or_insert_with(|| sha.to_owned());
            &mut parsed.bad
        } else if word == "skip" {
            &mut parsed.skipped
        } else {
            continue;
        };
        // `skip` can name several commits at once.
        list.push(sha.to_owned());
        if word == "skip" {
            list.extend(words.map(str::to_owned));
        }
        if parsed.ready {
            parsed.steps += 1;
        }
        parsed.ready = parsed.first_good.is_some() && parsed.first_bad.is_some();
        parsed.last = Some((word.to_owned(), sha.to_owned()));
    }
    // A later answer about the same commit replaces the earlier one.
    for list in [&mut parsed.good, &mut parsed.bad, &mut parsed.skipped] {
        let mut seen = HashSet::new();
        list.retain(|sha| seen.insert(sha.clone()));
    }
    parsed
}

fn short(sha: &str) -> String {
    sha.chars().take(7).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "git bisect start
# status: waiting for both good and bad commits
# bad: [88ee5a5debfe44e04b81ec49a306395258885b4c] c12
git bisect bad 88ee5a5debfe44e04b81ec49a306395258885b4c
# status: waiting for good commit(s), bad commit known
# good: [12edd5ccd66aedbac8be37a859b6a6b68cf80e06] c2
git bisect good 12edd5ccd66aedbac8be37a859b6a6b68cf80e06
# bad: [97ba9e2ea37ca1cfea4bf1d1f9b0245ae18cf4c3] c7
git bisect bad 97ba9e2ea37ca1cfea4bf1d1f9b0245ae18cf4c3
# skip: [f0eb5a8e092136f6592ca6b62c381d239938c767] c4
git bisect skip f0eb5a8e092136f6592ca6b62c381d239938c767
# good: [b0783d7651a9e81c1accd7681338c8fdfd5827e0] c5
git bisect good b0783d7651a9e81c1accd7681338c8fdfd5827e0
# first bad commit: [33de29d6fe752c594ed6b48b6946722497351a4b] c6
";

    #[test]
    fn the_log_gives_the_range_the_answers_and_the_result() {
        let parsed = parse_log(LOG, "bad", "good");
        assert_eq!(
            parsed.first_bad.as_deref(),
            Some("88ee5a5debfe44e04b81ec49a306395258885b4c")
        );
        assert_eq!(
            parsed.first_good.as_deref(),
            Some("12edd5ccd66aedbac8be37a859b6a6b68cf80e06")
        );
        assert_eq!(parsed.bad.len(), 2);
        assert_eq!(parsed.good.len(), 2);
        assert_eq!(
            parsed.skipped,
            vec!["f0eb5a8e092136f6592ca6b62c381d239938c767".to_owned()]
        );
        // The first good and bad answers set the range; three answers came after.
        assert_eq!(parsed.steps, 3);
        assert_eq!(
            parsed.last,
            Some(("good".to_owned(), "b0783d7651a9e81c1accd7681338c8fdfd5827e0".to_owned()))
        );
        assert_eq!(
            parsed.found.as_deref(),
            Some("33de29d6fe752c594ed6b48b6946722497351a4b")
        );
    }

    #[test]
    fn newer_git_quotes_the_term() {
        let log = "git bisect start\ngit bisect new aaaa\ngit bisect old bbbb\n# first 'new' commit: [aaaa] x\n";
        assert_eq!(parse_log(log, "new", "old").found.as_deref(), Some("aaaa"));
        let log = "git bisect start\ngit bisect bad aaaa\ngit bisect good bbbb\ngit bisect skip cccc\n# possible first 'bad' commit: [aaaa] x\n# possible first 'bad' commit: [cccc] y\n";
        assert_eq!(
            parse_log(log, "bad", "good").possible,
            vec!["aaaa".to_owned(), "cccc".to_owned()]
        );
    }

    #[test]
    fn other_terms_are_read_as_good_and_bad() {
        let log = "git bisect start\ngit bisect new aaaa\ngit bisect old bbbb\ngit bisect new cccc\n";
        let parsed = parse_log(log, "new", "old");
        assert_eq!(parsed.bad, vec!["aaaa".to_owned(), "cccc".to_owned()]);
        assert_eq!(parsed.good, vec!["bbbb".to_owned()]);
        assert_eq!(parsed.steps, 1);
        assert!(parsed.found.is_none());
    }

    #[test]
    fn only_skipped_commits_left() {
        let log = "git bisect start\ngit bisect bad aaaa\ngit bisect good bbbb\ngit bisect skip cccc dddd\n# only skipped commits left to test\n# possible first bad commit: [aaaa] x\n# possible first bad commit: [cccc] y\n";
        let parsed = parse_log(log, "bad", "good");
        assert_eq!(parsed.skipped, vec!["cccc".to_owned(), "dddd".to_owned()]);
        assert_eq!(parsed.possible, vec!["aaaa".to_owned(), "cccc".to_owned()]);
    }
}
