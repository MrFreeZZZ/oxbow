//! A series of commits before and after it was rewritten: by a rebase, Edit Stack, an amend or
//! a force push. Built on `git range-diff`, which pairs each old commit with its new version
//! and shows how the change itself differs, apart from the base it now sits on.
//!
//! The old series is what the old tip had that the new one doesn't, and the reverse for the new
//! series, less the commits a branch outside the rewrite brings: those are the new base, e.g.
//! the newer `main` a rebase moved onto. The output of `range-diff` is meant for people and may
//! change between git versions, so it is parsed loosely and tested on real output.

use std::collections::HashMap;

use serde::Serialize;

use crate::cli::GitCommand;
use crate::error::{Error, Result};
use crate::repo::Repo;

/// At most this many commits on each side: `range-diff` compares every old commit with every
/// new one.
pub const MAX_SERIES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PairKind {
    /// The same change: only the commit's base, and so its id, is new.
    Same,
    /// The change, its message or its author is different.
    Changed,
    /// Only the new series has it.
    Added,
    /// Only the old series had it.
    Dropped,
}

/// Which version a line of the diff between the two versions belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Outer {
    Both,
    Old,
    New,
}

/// What a line is inside the commit's own change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Inner {
    Context,
    Added,
    Removed,
    /// A hunk of the commit's diff starts, with the function it is in, if any.
    Hunk,
    /// A line of the message or the author, or a note such as "No newline at end of file".
    Text,
    /// Unchanged lines left out between two parts of the section.
    Gap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterLine {
    pub outer: Outer,
    pub inner: Inner,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SectionKind {
    /// The author.
    Metadata,
    Message,
    File,
}

/// One part of a pair's diff between versions: the author, the message, or one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterSection {
    pub kind: SectionKind,
    /// The file's path for a file, with git's note such as "(new)"; empty otherwise.
    pub title: String,
    /// Some line differs between the versions.
    pub changed: bool,
    pub lines: Vec<InterLine>,
}

/// A commit of either series.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesCommit {
    pub id: String,
    pub summary: String,
    pub author_name: String,
    pub author_email: String,
    pub time: i64,
    pub parents: Vec<String>,
}

/// An old commit and its new version, or a commit only one series has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesPair {
    pub kind: PairKind,
    pub old: Option<SeriesCommit>,
    pub new: Option<SeriesCommit>,
    /// Where the commit is in its series, 1 for the oldest.
    pub old_index: Option<usize>,
    pub new_index: Option<usize>,
    pub message: bool,
    pub code: bool,
    pub author: bool,
    /// For a changed pair: how the second version differs from the first.
    pub sections: Vec<InterSection>,
}

/// The old series beside the new one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Series {
    pub old_tip: String,
    pub new_tip: String,
    /// The commit each series sits on; none when it starts at the root or its commits don't
    /// form one line.
    pub old_base: Option<String>,
    pub new_base: Option<String>,
    /// A branch, tag or remote branch at the new base, when it moved.
    pub base_name: Option<String>,
    /// How many commits the new base has that the old one didn't.
    pub base_commits: usize,
    pub old_count: usize,
    pub new_count: usize,
    /// Newest first, as History lists commits; a dropped commit sits where it was.
    pub pairs: Vec<SeriesPair>,
    /// The comparison as one would type it.
    pub command: String,
}

/// How a branch's commits were last replaced by others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RewriteSource {
    /// The branch here was rewritten and is not pushed yet: the remote has the old series.
    NotPushed,
    /// A force push from here replaced the series on the remote.
    Pushed,
    /// A fetch brought a series someone else force pushed.
    Fetched,
}

/// The latest rewrite of a branch: the series before it and after it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rewrite {
    pub old: String,
    pub new: String,
    pub source: RewriteSource,
    /// When the remote branch here moved, in seconds since the Unix epoch; none for
    /// [`RewriteSource::NotPushed`].
    pub time: Option<i64>,
    /// The refs that moved with it, for [`Repo::series`].
    pub rewritten: Vec<String>,
}

/// How many entries of a remote branch's reflog are looked through.
const REWRITE_ENTRIES: usize = 50;

impl Repo {
    /// The latest rewrite of `branch` as its remote branch knows it: the local branch rewritten
    /// since the last push, or else the newest move of the remote branch that was not forward,
    /// from its reflog. `None` when it has no upstream or was never rewritten.
    pub fn last_rewrite(&self, branch: &str) -> Result<Option<Rewrite>> {
        let local = format!("refs/heads/{branch}");
        // A branch name never starts with a dash; `--end-of-options` would be printed back here.
        if branch.starts_with('-') {
            return Ok(None);
        }
        let Ok(upstream) = self.run(&GitCommand::new([
            "rev-parse".to_owned(),
            "--symbolic-full-name".to_owned(),
            format!("{branch}@{{upstream}}"),
        ])) else {
            return Ok(None);
        };
        let upstream = upstream.stdout.trim().to_owned();
        if !upstream.starts_with("refs/remotes/") {
            return Ok(None);
        }
        let rewritten = vec![local.clone(), upstream.clone()];
        if let (Ok(here), Ok(there)) = (self.commit_id(&local), self.commit_id(&upstream))
            && here != there
            && !self.is_ancestor(&there, &here)
            && !self.is_ancestor(&here, &there)
        {
            return Ok(Some(Rewrite {
                old: there,
                new: here,
                source: RewriteSource::NotPushed,
                time: None,
                rewritten,
            }));
        }
        let Ok(out) = self.run(&GitCommand::new([
            "reflog".to_owned(),
            "show".to_owned(),
            "--date=unix".to_owned(),
            "--format=%H%x1f%gd%x1f%gs".to_owned(),
            format!("--max-count={REWRITE_ENTRIES}"),
            upstream.clone(),
            "--".to_owned(),
        ])) else {
            return Ok(None);
        };
        let moves: Vec<(&str, Option<i64>, &str)> = out
            .stdout
            .lines()
            .filter_map(|line| {
                let mut parts = line.split('\u{1f}');
                let id = parts.next()?;
                let time = parts
                    .next()?
                    .rsplit_once("@{")
                    .and_then(|(_, t)| t.trim_end_matches('}').parse().ok());
                Some((id, time, parts.next().unwrap_or_default()))
            })
            .collect();
        for pair in moves.windows(2) {
            let ((new, time, message), (old, ..)) = (pair[0], pair[1]);
            if new != old && !self.is_ancestor(old, new) && self.object_exists(old) {
                return Ok(Some(Rewrite {
                    old: old.to_owned(),
                    new: new.to_owned(),
                    source: if message.starts_with("update by push") {
                        RewriteSource::Pushed
                    } else {
                        RewriteSource::Fetched
                    },
                    time,
                    rewritten,
                }));
            }
        }
        Ok(None)
    }

    /// The commits `old_tip` had beside the ones `new_tip` has in their place. Branches named in
    /// `rewritten` (full ref names) moved with the rewrite, so what they have is not taken for
    /// the new base.
    pub fn series(&self, old_tip: &str, new_tip: &str, rewritten: &[String]) -> Result<Series> {
        let old_tip = self.commit_id(old_tip)?;
        let new_tip = self.commit_id(new_tip)?;
        let old_own = self.own_commits(&old_tip, &new_tip, rewritten)?;
        let new_own = self.own_commits(&new_tip, &old_tip, rewritten)?;
        if old_own.is_empty() && new_own.is_empty() {
            return Err(Error::Git(
                "Nothing was rewritten: the two tips have the same commits".into(),
            ));
        }
        let too_many = old_own.len().max(new_own.len());
        if too_many > MAX_SERIES {
            return Err(Error::Git(format!(
                "Too many commits to compare: {too_many}, at most {MAX_SERIES} on each side"
            )));
        }
        let old_base = self.base_of(&old_own, &old_tip)?;
        let new_base = self.base_of(&new_own, &new_tip)?;
        // A side whose commits don't form one line falls back to all it has that the other
        // doesn't, base commits and all.
        let old_range = match &old_base {
            Base::Line(base) => format!("{base}..{old_tip}"),
            _ => format!("{new_tip}..{old_tip}"),
        };
        let new_range = match &new_base {
            Base::Line(base) => format!("{base}..{new_tip}"),
            _ => format!("{old_tip}..{new_tip}"),
        };
        let out = self.run(&GitCommand::new([
            "-c",
            "core.quotePath=false",
            "-c",
            "log.showSignature=false",
            "range-diff",
            "--no-color",
            "--no-abbrev",
            "--no-notes",
            &old_range,
            &new_range,
        ]))?;
        let parsed = parse_range_diff(&out.stdout);

        let ids: Vec<&str> = parsed
            .iter()
            .flat_map(|p| p.old.iter().chain(&p.new))
            .map(String::as_str)
            .collect();
        let info = self.commit_infos(&ids)?;
        let mut pairs: Vec<SeriesPair> = parsed
            .into_iter()
            .map(|p| {
                let (message, author, code) = p.flags();
                SeriesPair {
                    kind: p.kind,
                    old: p.old.as_ref().and_then(|id| info.get(id).cloned()),
                    new: p.new.as_ref().and_then(|id| info.get(id).cloned()),
                    old_index: p.old_index,
                    new_index: p.new_index,
                    message,
                    code,
                    author,
                    sections: p.sections,
                }
            })
            .collect();
        pairs.reverse();

        let (old_base, new_base) = (old_base.id(), new_base.id());
        let (base_name, base_commits) = match (&old_base, &new_base) {
            (Some(old), Some(new)) if old != new => (
                self.name_at(new),
                self.run(&GitCommand::new(["rev-list", "--count", &format!("{old}..{new}")]))
                    .ok()
                    .and_then(|out| out.stdout.trim().parse().ok())
                    .unwrap_or(0),
            ),
            _ => (None, 0),
        };
        let short = |range: &str| {
            range
                .split("..")
                .map(|id| &id[..id.len().min(7)])
                .collect::<Vec<_>>()
                .join("..")
        };
        Ok(Series {
            command: format!("git range-diff {} {}", short(&old_range), short(&new_range)),
            old_count: pairs.iter().filter(|p| p.old.is_some()).count(),
            new_count: pairs.iter().filter(|p| p.new.is_some()).count(),
            old_tip,
            new_tip,
            old_base,
            new_base,
            base_name,
            base_commits,
            pairs,
        })
    }

    /// The commits `tip` has that `other` doesn't and that no branch outside the rewrite has,
    /// newest first, each with its parents.
    fn own_commits(&self, tip: &str, other: &str, rewritten: &[String]) -> Result<Vec<(String, Vec<String>)>> {
        // Refs that hold the whole of `tip` came after it, e.g. main once the branch was merged
        // or a branch stacked on it: they say nothing about where it starts. Only branches mark
        // where a series starts: a tag on one of its commits stays behind on the old one when it
        // is rebased, and would cut the old series short.
        let refs = self.run(&GitCommand::new([
            "for-each-ref".to_owned(),
            format!("--no-contains={tip}"),
            "--format=%(refname)".to_owned(),
            "refs/heads/".to_owned(),
            "refs/remotes/".to_owned(),
        ]))?;
        let mut input = format!("{tip}\n^{other}\n");
        for name in refs.stdout.lines() {
            let skip = rewritten.iter().any(|r| r == name);
            if !skip {
                input.push_str(&format!("^{name}\n"));
            }
        }
        let out = self.run_with_input(
            &GitCommand::new([
                "rev-list".to_owned(),
                "--topo-order".to_owned(),
                "--parents".to_owned(),
                format!("--max-count={}", MAX_SERIES + 1),
                "--stdin".to_owned(),
            ]),
            Some(input.as_bytes()),
        )?;
        Ok(out
            .stdout
            .lines()
            .filter_map(|line| {
                let mut ids = line.split_whitespace().map(str::to_owned);
                Some((ids.next()?, ids.collect()))
            })
            .collect())
    }

    /// Where `own`, newest first, starts: the parent of its oldest commit when they form one line.
    fn base_of(&self, own: &[(String, Vec<String>)], tip: &str) -> Result<Base> {
        let Some((_, parents)) = own.last() else {
            // Nothing of its own: the tip is the base, e.g. every commit was dropped.
            return Ok(Base::Line(tip.to_owned()));
        };
        let line = own.first().is_some_and(|(id, _)| id == tip)
            && own.windows(2).all(|w| w[0].1.len() == 1 && w[0].1[0] == w[1].0)
            && parents.len() <= 1;
        Ok(match (line, parents.first()) {
            (true, Some(parent)) => Base::Line(parent.clone()),
            (true, None) => Base::Root,
            (false, _) => Base::Tangled,
        })
    }

    /// Summary, author and time of each commit.
    fn commit_infos(&self, ids: &[&str]) -> Result<HashMap<String, SeriesCommit>> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let out = self.run_with_input(
            &GitCommand::new([
                "log",
                "--no-walk=unsorted",
                "--format=%H%x1f%s%x1f%an%x1f%ae%x1f%at%x1f%P",
                "--stdin",
            ]),
            Some((ids.join("\n") + "\n").as_bytes()),
        )?;
        Ok(out
            .stdout
            .lines()
            .filter_map(|line| {
                let mut parts = line.split('\u{1f}');
                let id = parts.next()?.to_owned();
                Some((
                    id.clone(),
                    SeriesCommit {
                        id,
                        summary: parts.next().unwrap_or_default().to_owned(),
                        author_name: parts.next().unwrap_or_default().to_owned(),
                        author_email: parts.next().unwrap_or_default().to_owned(),
                        time: parts.next().and_then(|t| t.parse().ok()).unwrap_or(0),
                        parents: parts
                            .next()
                            .unwrap_or_default()
                            .split_whitespace()
                            .map(str::to_owned)
                            .collect(),
                    },
                ))
            })
            .collect())
    }

    /// A short name for `id`: a local branch, then a remote branch, then a tag pointing at it.
    fn name_at(&self, id: &str) -> Option<String> {
        let out = self
            .run(&GitCommand::new([
                "for-each-ref".to_owned(),
                format!("--points-at={id}"),
                "--format=%(refname)".to_owned(),
                "refs/heads".to_owned(),
                "refs/remotes".to_owned(),
                "refs/tags".to_owned(),
            ]))
            .ok()?;
        let names: Vec<&str> = out.stdout.lines().filter(|n| !n.ends_with("/HEAD")).collect();
        ["refs/heads/", "refs/remotes/", "refs/tags/"]
            .iter()
            .find_map(|prefix| names.iter().find_map(|n| n.strip_prefix(prefix)))
            .map(str::to_owned)
    }
}

enum Base {
    /// The commits form one line on top of this commit.
    Line(String),
    /// One line from a root commit.
    Root,
    /// Merges or several starts.
    Tangled,
}

impl Base {
    fn id(&self) -> Option<String> {
        match self {
            Base::Line(id) => Some(id.clone()),
            _ => None,
        }
    }
}

/// One pair as `range-diff` printed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedPair {
    pub kind: PairKind,
    pub old: Option<String>,
    pub new: Option<String>,
    pub old_index: Option<usize>,
    pub new_index: Option<usize>,
    pub sections: Vec<InterSection>,
}

impl ParsedPair {
    /// Whether the message, the author and the code differ.
    fn flags(&self) -> (bool, bool, bool) {
        let changed = |kind: SectionKind| self.sections.iter().any(|s| s.kind == kind && s.changed);
        let (message, author, code) = (
            changed(SectionKind::Message),
            changed(SectionKind::Metadata),
            changed(SectionKind::File),
        );
        // git saw a difference the parse didn't place: call it the code.
        let code = code || (self.kind == PairKind::Changed && !message && !author);
        (message, author, code)
    }
}

/// The pairs of `git range-diff --no-color --no-abbrev` output, in its order: the new series,
/// oldest first, with each dropped commit where it was.
pub(crate) fn parse_range_diff(text: &str) -> Vec<ParsedPair> {
    let mut pairs: Vec<ParsedPair> = Vec::new();
    let mut body: Vec<&str> = Vec::new();
    let finish = |pairs: &mut Vec<ParsedPair>, body: &mut Vec<&str>| {
        if let Some(last) = pairs.last_mut() {
            last.sections = parse_body(body);
        }
        body.clear();
    };
    for line in text.lines() {
        if let Some(pair) = (!line.starts_with("    ")).then(|| parse_header(line)).flatten() {
            finish(&mut pairs, &mut body);
            pairs.push(pair);
        } else if !pairs.is_empty() {
            body.push(line);
        }
    }
    finish(&mut pairs, &mut body);
    pairs
}

/// `1:  <id> ! 2:  <id> Subject`, the numbers padded to the same width.
fn parse_header(line: &str) -> Option<ParsedPair> {
    let mut words = line.split_whitespace();
    let old_index = words.next()?.strip_suffix(':')?;
    let old = words.next()?;
    let mark = words.next()?;
    let new_index = words.next()?.strip_suffix(':')?;
    let new = words.next()?;
    let index = |n: &str| -> Option<Option<usize>> {
        if n.chars().all(|c| c == '-') {
            Some(None)
        } else {
            n.parse().ok().map(Some)
        }
    };
    let id = |id: &str| -> Option<Option<String>> {
        if id.chars().all(|c| c == '-') {
            Some(None)
        } else if id.len() >= 7 && id.chars().all(|c| c.is_ascii_hexdigit()) {
            Some(Some(id.to_owned()))
        } else {
            None
        }
    };
    let kind = match mark {
        "=" => PairKind::Same,
        "!" => PairKind::Changed,
        ">" => PairKind::Added,
        "<" => PairKind::Dropped,
        _ => return None,
    };
    let (old, new) = (id(old)?, id(new)?);
    // The side that is missing is all dashes, the other is a commit.
    match kind {
        PairKind::Added if old.is_some() || new.is_none() => return None,
        PairKind::Dropped if old.is_none() || new.is_some() => return None,
        PairKind::Same | PairKind::Changed if old.is_none() || new.is_none() => return None,
        _ => {}
    }
    Some(ParsedPair {
        kind,
        old,
        new,
        old_index: index(old_index)?,
        new_index: index(new_index)?,
        sections: Vec::new(),
    })
}

/// The diff between the two versions of a changed pair, indented by four spaces: an `@@ name`
/// line starts each part of it, every other line is the version mark (space, `-` or `+`)
/// followed by a line of the commit's own patch as `range-diff` writes it: `## path ##` or
/// `## Commit message ##` to start a section, the message indented by four spaces, and the
/// file's diff with its own marks.
fn parse_body(lines: &[&str]) -> Vec<InterSection> {
    let mut sections: Vec<InterSection> = Vec::new();
    // A file's section header seen in an earlier part, by its title.
    let mut titles: Vec<String> = Vec::new();
    let mut previous_header: Option<Outer> = None;
    // range-diff puts an empty line before each section header, and a part may open with one;
    // a file's own line `## x ##` has neither.
    let mut header_may_follow = true;
    for raw in lines {
        let Some(line) = raw.strip_prefix("    ") else {
            continue;
        };
        if let Some(label) = line.strip_prefix("@@") {
            let label = label.trim();
            let (kind, title) = match label {
                "Metadata" => (SectionKind::Metadata, String::new()),
                "Commit message" => (SectionKind::Message, String::new()),
                _ => (
                    SectionKind::File,
                    titles
                        .iter()
                        .filter(|t| label.starts_with(t.as_str()))
                        .max_by_key(|t| t.len())
                        .cloned()
                        .unwrap_or_else(|| label.split(": ").next().unwrap_or(label).to_owned()),
                ),
            };
            match sections.last_mut() {
                Some(last) if last.kind == kind && last.title == title => last.lines.push(InterLine {
                    outer: Outer::Both,
                    inner: Inner::Gap,
                    text: String::new(),
                }),
                _ => sections.push(InterSection {
                    kind,
                    title,
                    changed: false,
                    lines: Vec::new(),
                }),
            }
            previous_header = None;
            header_may_follow = true;
            continue;
        }
        let mut chars = line.chars();
        let outer = match chars.next() {
            Some('-') => Outer::Old,
            Some('+') => Outer::New,
            _ => Outer::Both,
        };
        let inner = chars.as_str();
        let header = header_may_follow || previous_header.is_some();
        if let Some(name) = inner
            .strip_prefix(" ## ")
            .and_then(|t| t.strip_suffix(" ##"))
            .filter(|_| header)
        {
            let (kind, title) = if name == "Commit message" {
                (SectionKind::Message, String::new())
            } else {
                (SectionKind::File, name.to_owned())
            };
            if kind == SectionKind::File && !titles.contains(&title) {
                titles.push(title.clone());
            }
            // `-## a ##` then `+## a (new) ##`: one file whose note changed.
            let renamed = previous_header == Some(Outer::Old) && outer == Outer::New;
            match sections.last_mut() {
                Some(last) if renamed => {
                    last.title = title;
                    last.changed = true;
                }
                // The part that this header opens was started by its `@@` line.
                Some(last) if last.lines.is_empty() && last.kind == kind => {
                    last.title = title;
                    last.changed |= outer != Outer::Both;
                }
                _ => sections.push(InterSection {
                    kind,
                    title,
                    changed: outer != Outer::Both,
                    lines: Vec::new(),
                }),
            }
            previous_header = Some(outer);
            header_may_follow = false;
            continue;
        }
        previous_header = None;
        header_may_follow = inner.is_empty();
        let Some(section) = sections.last_mut() else {
            continue;
        };
        let (inner, text) = match section.kind {
            SectionKind::Metadata => (Inner::Text, inner.trim_start().to_owned()),
            SectionKind::Message => (
                Inner::Text,
                inner.strip_prefix("    ").unwrap_or(inner.trim_start()).to_owned(),
            ),
            SectionKind::File => {
                if let Some(rest) = inner.strip_prefix("@@") {
                    (Inner::Hunk, rest.trim().to_owned())
                } else if let Some(rest) = inner.strip_prefix('+') {
                    (Inner::Added, rest.to_owned())
                } else if let Some(rest) = inner.strip_prefix('-') {
                    (Inner::Removed, rest.to_owned())
                } else if let Some(rest) = inner.strip_prefix(' ') {
                    (Inner::Context, rest.to_owned())
                } else if inner.is_empty() {
                    (Inner::Context, String::new())
                } else {
                    (Inner::Text, inner.trim_start_matches('\\').trim().to_owned())
                }
            }
        };
        section.changed |= outer != Outer::Both;
        section.lines.push(InterLine { outer, inner, text });
    }
    for section in &mut sections {
        // The blank line range-diff puts between sections.
        while section
            .lines
            .last()
            .is_some_and(|l| l.outer == Outer::Both && l.text.is_empty() && l.inner != Inner::Hunk)
        {
            section.lines.pop();
        }
        // A file's section often ends with a lone `@@` that only says its diff starts there.
        if section.kind == SectionKind::File
            && section.lines.len() == 1
            && section.lines[0].inner == Inner::Hunk
            && section.lines[0].text.is_empty()
            && !section.changed
        {
            section.lines.clear();
        }
    }
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "1111111111111111111111111111111111111111";
    const B: &str = "2222222222222222222222222222222222222222";
    const C: &str = "3333333333333333333333333333333333333333";
    const D: &str = "4444444444444444444444444444444444444444";
    const DASH: &str = "----------------------------------------";

    #[test]
    fn headers_pair_old_and_new_commits() {
        let text = format!("1:  {A} = 1:  {B} One\n2:  {C} < -:  {DASH} Two\n-:  {DASH} > 2:  {D} Three: with colon\n");
        let pairs = parse_range_diff(&text);
        assert_eq!(pairs.len(), 3);
        assert_eq!(pairs[0].kind, PairKind::Same);
        assert_eq!((pairs[0].old.as_deref(), pairs[0].new.as_deref()), (Some(A), Some(B)));
        assert_eq!(pairs[1].kind, PairKind::Dropped);
        assert_eq!((pairs[1].old_index, pairs[1].new_index), (Some(2), None));
        assert_eq!(pairs[2].kind, PairKind::Added);
        assert_eq!(pairs[2].new.as_deref(), Some(D));
    }

    #[test]
    fn padded_numbers_still_parse() {
        let text = format!(" 9:  {A} = 10:  {B} Nine\n10:  {C} ! 11:  {D} Ten\n");
        let pairs = parse_range_diff(&text);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[1].old_index, Some(10));
        assert_eq!(pairs[1].new_index, Some(11));
    }

    #[test]
    fn message_and_code_changes_are_told_apart() {
        let text = format!(
            "1:  {A} ! 1:  {B} Two\n\
             \x20   @@ Metadata\n\
             \x20    Author: A <a@b>\n\
             \x20    \n\
             \x20     ## Commit message ##\n\
             \x20   -    two\n\
             \x20   +    two improved\n\
             \x20    \n\
             \x20     ## f.txt ##\n\
             \x20    @@\n\
             2:  {C} ! 2:  {D} Three\n\
             \x20   @@ g.txt (new)\n\
             \x20    @@\n\
             \x20    +new\n\
             \x20    +more\n\
             \x20   ++line3\n\
             \x20   +\n\
             \x20   + ## z.txt (new) ##\n\
             \x20   +@@\n\
             \x20   ++q\n"
        );
        let pairs = parse_range_diff(&text);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].flags(), (true, false, false));
        let message = &pairs[0].sections[1];
        assert_eq!(message.kind, SectionKind::Message);
        assert_eq!(
            message.lines,
            [
                InterLine {
                    outer: Outer::Old,
                    inner: Inner::Text,
                    text: "two".into()
                },
                InterLine {
                    outer: Outer::New,
                    inner: Inner::Text,
                    text: "two improved".into()
                },
            ]
        );
        // The unchanged file says nothing.
        assert_eq!(pairs[0].sections[2].title, "f.txt");
        assert!(pairs[0].sections[2].lines.is_empty());

        assert_eq!(pairs[1].flags(), (false, false, true));
        let files: Vec<_> = pairs[1]
            .sections
            .iter()
            .map(|s| (s.title.as_str(), s.changed))
            .collect();
        assert_eq!(files, [("g.txt (new)", true), ("z.txt (new)", true)]);
        let g = &pairs[1].sections[0].lines;
        assert_eq!(
            g[1],
            InterLine {
                outer: Outer::Both,
                inner: Inner::Added,
                text: "new".into()
            }
        );
        assert_eq!(
            g[3],
            InterLine {
                outer: Outer::New,
                inner: Inner::Added,
                text: "line3".into()
            }
        );
        assert_eq!(pairs[1].sections[1].lines[1].inner, Inner::Added);
    }

    #[test]
    fn a_later_part_of_a_file_joins_its_section() {
        let text = format!(
            "1:  {A} ! 1:  {B} Change\n\
             \x20   @@ src/a.rs: fn one()\n\
             \x20     ## src/a.rs ##\n\
             \x20    @@ fn one()\n\
             \x20   -+    old\n\
             \x20   ++    new\n\
             \x20   @@ src/a.rs: fn two()\n\
             \x20     context\n\
             \x20   -+    x\n"
        );
        let pairs = parse_range_diff(&text);
        let sections = &pairs[0].sections;
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].title, "src/a.rs");
        let gaps = sections[0].lines.iter().filter(|l| l.inner == Inner::Gap).count();
        assert_eq!(gaps, 1);
        assert_eq!(sections[0].lines.last().unwrap().outer, Outer::Old);
    }

    #[test]
    fn noise_is_not_a_pair() {
        assert!(parse_range_diff("warning: something\nfatal: no\n").is_empty());
        assert!(parse_header(&format!("1:  {A} ? 1:  {B} x")).is_none());
        assert!(parse_header(&format!("1:  {DASH} = 1:  {B} x")).is_none());
    }
}
