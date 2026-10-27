//! Commits as patch files: saving them with `git format-patch`, and applying patch files. Mailed
//! patches go in with `git am`, which makes their commits and can stop on conflicts like a
//! rebase; a plain diff goes into the files with `git apply`, uncommitted.

use std::path::Path;

use serde::Serialize;

use crate::cli::{GitCommand, literal};
use crate::error::{Error, Result};
use crate::remote::{CommitBrief, parse_briefs};
use crate::repo::Repo;

/// Commits listed in a preview at most: a mailbox can hold a whole branch.
const PREVIEW_COMMITS: usize = 200;

/// What applying patch files would do, before anything runs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchPreview {
    /// The commits the patches make, in order; empty for a plain diff.
    pub commits: Vec<CommitBrief>,
    /// More commits than are listed.
    pub more: usize,
    /// A diff without commits (`git diff` output): its changes go into the files, uncommitted.
    pub plain: bool,
    /// The files the patches change, each once.
    pub files: Vec<PatchFile>,
    /// Files whose changes don't fit the checked-out commit (a plain diff: the files on disk)
    /// as they are. `git am` then merges them three ways and can stop on conflicts; a plain
    /// diff is not applied at all.
    pub not_clean: Vec<String>,
    /// The patches don't fit and git did not say which file is to blame.
    pub fails: bool,
    /// Uncommitted changes `git am` would refuse to work over: staged files, and edits to files
    /// the patches change or add.
    pub in_the_way: Vec<String>,
}

/// A file a patch changes, with the lines it adds and removes; `None` for a binary file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchFile {
    pub path: String,
    pub added: Option<usize>,
    pub removed: Option<usize>,
}

impl Repo {
    /// What applying the patch files at `paths` would bring, and whether they fit.
    pub fn patch_preview(&self, paths: &[String]) -> Result<PatchPreview> {
        let paths = patch_paths(paths)?;
        let (commits, plain) = self.patch_commits(&paths)?;
        let files = self.patch_files(&paths)?;
        let (not_clean, fails) = self.patch_fits(&paths, plain)?;
        let in_the_way = if plain { Vec::new() } else { self.in_the_way_of(&files)? };
        let more = commits.len().saturating_sub(PREVIEW_COMMITS);
        Ok(PatchPreview {
            commits: commits.into_iter().take(PREVIEW_COMMITS).collect(),
            more,
            plain,
            files,
            not_clean,
            fails,
            in_the_way,
        })
    }

    /// The commits `git format-patch` would write: those of `to` that `since` does not have, or
    /// the newest `count` up to `to`. Merges are left out, as format-patch leaves them out.
    pub fn patch_range(&self, to: &str, since: Option<&str>, count: usize) -> Result<Vec<CommitBrief>> {
        let range = match since {
            Some(since) => vec![format!("{since}..{to}")],
            None => vec![format!("--max-count={count}"), to.to_owned()],
        };
        let mut args = vec![
            "log".to_owned(),
            "--no-merges".to_owned(),
            "--reverse".to_owned(),
            "--format=%H%x1f%s%x1f%an%x1f%at".to_owned(),
        ];
        args.extend(range);
        args.push("--".to_owned());
        Ok(parse_briefs(&self.run(&GitCommand::new(args))?.stdout))
    }

    pub(crate) fn plan_save_patches(
        &self,
        to: &str,
        since: Option<&str>,
        count: usize,
        into: &str,
        one_file: bool,
    ) -> Result<Vec<GitCommand>> {
        if !Path::new(into).is_absolute() {
            return Err(Error::Git(format!("{into} is not a full path")));
        }
        let mut args = vec!["format-patch".to_owned()];
        let mut comment = match since {
            Some(since) => {
                args.push(format!("{since}..{to}"));
                format!("the commits {} has that {} doesn’t", short_name(to), short_name(since))
            }
            None => {
                args.extend([format!("-{count}"), to.to_owned()]);
                if count == 1 {
                    format!("-1: just {}", short_name(to))
                } else {
                    format!("-{count}: the newest {count} commits up to {}", short_name(to))
                }
            }
        };
        if one_file {
            args.push(format!("--output={into}"));
            comment.push_str(", all in one file");
        } else {
            args.extend(["-o".to_owned(), into.to_owned()]);
            comment.push_str(", a numbered .patch file each");
        }
        Ok(vec![GitCommand::new(args).comment(comment)])
    }

    pub(crate) fn plan_apply_patches(&self, paths: &[String]) -> Result<Vec<GitCommand>> {
        let paths = patch_paths(paths)?;
        let (_, plain) = self.patch_commits(&paths)?;
        if plain {
            let mut args = vec!["apply".to_owned(), "--".to_owned()];
            args.extend(paths.iter().map(|p| p.to_string()));
            return Ok(vec![GitCommand::new(args).comment(
                "a plain diff: its changes go into your files, uncommitted; nothing changes if any of it doesn’t fit",
            )]);
        }
        let mut args = vec!["am".to_owned(), "--3way".to_owned()];
        if !crate::config::run_hooks() {
            args.push("--no-verify".to_owned());
        }
        args.push("--".to_owned());
        args.extend(paths.iter().map(|p| p.to_string()));
        Ok(vec![GitCommand::new(args).comment(
            "a commit for each patch, with its author and message; --3way: merges what doesn’t fit, and stops on conflicts",
        )])
    }

    /// The commits in the patch files, and whether they are a plain diff instead. Every file is
    /// one or the other: a mix is refused.
    fn patch_commits(&self, paths: &[&str]) -> Result<(Vec<CommitBrief>, bool)> {
        let dir = crate::cli::private_temp_folder("oxbow-patches-").map_err(|err| Error::Git(err.to_string()))?;
        let mails = dir.path().join("mails");
        let mut commits = Vec::new();
        let mut plain = Vec::new();
        for path in paths {
            let _ = std::fs::remove_dir_all(&mails);
            crate::cli::create_private_dir(&mails).map_err(|err| Error::Git(err.to_string()))?;
            self.run(&GitCommand::new([
                "mailsplit".to_owned(),
                // -b: a file that doesn't start with a `From ` line is one mail, or a plain diff.
                "-b".to_owned(),
                format!("-o{}", mails.display()),
                "--".to_owned(),
                path.to_string(),
            ]))?;
            let mut split: Vec<_> = std::fs::read_dir(&mails)
                .map_err(|err| Error::Git(err.to_string()))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .collect();
            split.sort();
            let mut found = 0;
            for mail in split {
                let text = std::fs::read(&mail).map_err(|err| Error::Git(err.to_string()))?;
                let msg = dir.path().join("msg").display().to_string();
                let patch = dir.path().join("patch").display().to_string();
                let out = self.spawn(&GitCommand::new(["mailinfo", &msg, &patch]), Some(&text))?;
                let info = String::from_utf8_lossy(&out.stdout);
                let field = |name: &str| {
                    info.lines()
                        .find_map(|l| l.strip_prefix(name))
                        .map(|v| v.trim().to_owned())
                        .filter(|v| !v.is_empty())
                };
                let (Some(author), Some(summary)) = (field("Author: "), field("Subject: ")) else {
                    continue;
                };
                let time = field("Date: ")
                    .and_then(|d| gix::date::parse(&d, None).ok())
                    .map_or(0, |t| t.seconds);
                commits.push(CommitBrief {
                    id: mail_id(&text).unwrap_or_default(),
                    summary,
                    author_name: author,
                    time,
                });
                found += 1;
            }
            plain.push(found == 0);
        }
        match (plain.iter().all(|p| *p), plain.iter().any(|p| *p)) {
            (true, _) => Ok((Vec::new(), true)),
            (false, false) => Ok((commits, false)),
            (false, true) => Err(Error::Git(
                "some of the files are patches with commits and some are plain diffs; apply them apart".into(),
            )),
        }
    }

    /// Each file the patches change, with its lines added and removed over all of them.
    fn patch_files(&self, paths: &[&str]) -> Result<Vec<PatchFile>> {
        let mut args = vec![
            "apply".to_owned(),
            "--numstat".to_owned(),
            "-z".to_owned(),
            "--".to_owned(),
        ];
        args.extend(paths.iter().map(|p| p.to_string()));
        let out = self.run(&GitCommand::new(args))?.stdout;
        let mut files: Vec<PatchFile> = Vec::new();
        let mut fields = out.split('\0');
        while let Some(entry) = fields.next() {
            let mut parts = entry.splitn(3, '\t');
            let (Some(added), Some(removed), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
                continue;
            };
            // A rename leaves the path empty and gives the old and new paths after it.
            let path = if path.is_empty() {
                fields.next();
                fields.next().unwrap_or_default().to_owned()
            } else {
                path.to_owned()
            };
            let (added, removed) = (added.parse().ok(), removed.parse().ok());
            match files.iter_mut().find(|f| f.path == path) {
                Some(file) => {
                    file.added = file.added.zip(added).map(|(a, b)| a + b);
                    file.removed = file.removed.zip(removed).map(|(a, b)| a + b);
                }
                None => files.push(PatchFile { path, added, removed }),
            }
        }
        Ok(files)
    }

    /// Whether the patches fit as they are: mailed ones the checked-out commit, in an index of
    /// its own so staged changes don't count; a plain diff the files on disk. The files that
    /// don't, as git names them, and whether it failed without naming any.
    fn patch_fits(&self, paths: &[&str], plain: bool) -> Result<(Vec<String>, bool)> {
        let dir = crate::cli::private_temp_folder("oxbow-patches-").map_err(|err| Error::Git(err.to_string()))?;
        let index = dir.path().join("index").display().to_string();
        let mut check = vec!["apply".to_owned(), "--check".to_owned()];
        if !plain {
            let read = if self.head()?.commit.is_some() {
                GitCommand::new(["read-tree", "HEAD"])
            } else {
                GitCommand::new(["read-tree", "--empty"])
            };
            self.run(&read.env("GIT_INDEX_FILE", &index))?;
            check.push("--cached".to_owned());
        }
        check.push("--".to_owned());
        check.extend(paths.iter().map(|p| p.to_string()));
        let mut check = GitCommand::new(check);
        if !plain {
            check = check.env("GIT_INDEX_FILE", &index);
        }
        let out = self.spawn(&check, None)?;
        if out.status.success() {
            return Ok((Vec::new(), false));
        }
        let mut files: Vec<String> = Vec::new();
        for line in String::from_utf8_lossy(&out.stderr).lines() {
            let Some(rest) = line.strip_prefix("error: ") else {
                continue;
            };
            let path = [
                ": patch does not apply",
                ": does not exist in index",
                ": No such file or directory",
                ": already exists in index",
                ": already exists in working directory",
                ": does not match index",
            ]
            .iter()
            .find_map(|end| rest.strip_suffix(end));
            if let Some(path) = path
                && !files.iter().any(|f| f == path)
            {
                files.push(path.to_owned());
            }
        }
        let fails = files.is_empty();
        Ok((files, fails))
    }

    /// Uncommitted changes `git am` refuses to work over: every staged file, and the files the
    /// patches change that are edited or, for new ones, already on disk.
    fn in_the_way_of(&self, files: &[PatchFile]) -> Result<Vec<String>> {
        let out = self
            .run(&GitCommand::new(["diff", "--cached", "--name-only", "-z"]))?
            .stdout;
        let mut found: Vec<String> = out.split('\0').filter(|p| !p.is_empty()).map(str::to_owned).collect();
        if files.is_empty() {
            return Ok(found);
        }
        let mut args = vec![
            "status".to_owned(),
            "--porcelain=v1".to_owned(),
            "-z".to_owned(),
            "--untracked-files=all".to_owned(),
            "--".to_owned(),
        ];
        args.extend(files.iter().map(|f| literal(&f.path)));
        let out = self.run(&GitCommand::new(args))?.stdout;
        let mut entries = out.split('\0');
        while let Some(entry) = entries.next() {
            if entry.len() < 4 {
                continue;
            }
            let (code, path) = entry.split_at(3);
            // A rename names the old path next; it is staged, so already counted.
            if code.starts_with('R') || code.starts_with('C') {
                entries.next();
            }
            if !found.iter().any(|f| f == path) {
                found.push(path.to_owned());
            }
        }
        Ok(found)
    }
}

/// The patch files, each a full path: a relative one would be read from wherever git runs.
fn patch_paths(paths: &[String]) -> Result<Vec<&str>> {
    if paths.is_empty() {
        return Err(Error::Git("no patch files were given".into()));
    }
    paths
        .iter()
        .map(|p| {
            if Path::new(p).is_absolute() {
                Ok(p.as_str())
            } else {
                Err(Error::Git(format!("{p} is not a full path")))
            }
        })
        .collect()
}

/// The sha on the `From <sha> Mon Sep 17 00:00:00 2001` line format-patch starts a mail with.
pub(crate) fn mail_id(mail: &[u8]) -> Option<String> {
    let first = mail.split(|b| *b == b'\n').next()?;
    let sha = std::str::from_utf8(first.strip_prefix(b"From ")?)
        .ok()?
        .split(' ')
        .next()?;
    (sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit())).then(|| sha.to_owned())
}

/// A sha shortened; a branch name as it is.
fn short_name(rev: &str) -> String {
    if rev.len() == 40 && rev.bytes().all(|b| b.is_ascii_hexdigit()) {
        rev[..7].to_owned()
    } else {
        rev.to_owned()
    }
}
