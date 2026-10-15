//! When this clone last heard from each remote: the ahead/behind counts are only as fresh as
//! that. Git keeps no such date itself, so it is pieced together from what fetches leave behind:
//!
//! - `FETCH_HEAD`, which every fetch and pull rewrites, even one that brings nothing new. Its
//!   lines name the remote's address, so its time belongs to the remotes it names.
//! - Oxbow's own note of those times (`oxbow/fetched`), kept because the next fetch of another
//!   remote rewrites `FETCH_HEAD` and the earlier remote's time would be lost.
//! - The reflogs of the remote's branches: a clone or a fetch that moved one of them left a
//!   dated entry. Pushes move them too, but a push says nothing about what else is there, so
//!   those entries don't count.
//!
//! A fetch in a terminal counts the same as one in Oxbow.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::UNIX_EPOCH;

use serde::Serialize;

use crate::repo::{RefInfo, RefKind, Repo};

const RECORD: &str = "oxbow/fetched";

/// The last fetch of one remote.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFetch {
    pub remote: String,
    /// Seconds since the Unix epoch; none when nothing shows this clone ever fetched it.
    pub time: Option<i64>,
}

impl Repo {
    /// When each remote was last fetched, in the order of `remotes()`.
    pub fn fetch_times(&self) -> Vec<RemoteFetch> {
        let refs = self.refs().unwrap_or_default();
        self.fetch_times_of(&self.remotes(), &refs)
    }

    pub(crate) fn fetch_times_of(&self, remotes: &[String], refs: &[RefInfo]) -> Vec<RemoteFetch> {
        if remotes.is_empty() {
            return Vec::new();
        }
        let repo = self.local();
        let git_dir = repo.git_dir().to_owned();
        let record_path = git_dir.join(RECORD);
        let mut record = read_record(&record_path);
        let before = record.clone();

        // FETCH_HEAD's time goes to the remotes whose address it names.
        if let Some((time, urls)) = fetch_head(&git_dir.join("FETCH_HEAD")) {
            let mut named: Vec<&String> = remotes
                .iter()
                .filter(|remote| {
                    let Ok(found) = repo.find_remote(remote.as_str()) else {
                        return false;
                    };
                    let addresses = [
                        found
                            .url(gix::remote::Direction::Fetch)
                            .map(|u| u.to_bstring().to_string()),
                        found
                            .url(gix::remote::Direction::Push)
                            .map(|u| u.to_bstring().to_string()),
                    ];
                    addresses
                        .into_iter()
                        .flatten()
                        .any(|address| urls.contains(&normalize(&address)))
                })
                .collect();
            // An address written some other way, e.g. a relative path: with one remote there is
            // no doubt which it was.
            if named.is_empty() && remotes.len() == 1 {
                named.push(&remotes[0]);
            }
            for remote in named {
                let known = record.entry(remote.clone()).or_insert(time);
                *known = (*known).max(time);
            }
        }
        if record != before {
            // Only a note for later; the times stand without it.
            let text: String = record
                .iter()
                .map(|(remote, time)| format!("{time}\t{remote}\n"))
                .collect();
            if let Some(dir) = record_path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&record_path, text);
        }

        remotes
            .iter()
            .map(|remote| {
                let prefix = format!("{remote}/");
                // A clone writes its entry for the remote's HEAD only.
                let moved = refs
                    .iter()
                    .filter(|r| r.kind == RefKind::Remote && r.name.starts_with(&prefix))
                    .map(|r| format!("refs/remotes/{}", r.name))
                    .chain([format!("refs/remotes/{remote}/HEAD")])
                    .filter_map(|name| last_fetch_move(&repo, &name))
                    .max();
                RemoteFetch {
                    remote: remote.clone(),
                    time: record.get(remote).copied().max(moved),
                }
            })
            .collect()
    }
}

/// When the newest reflog entry of a remote branch was made, if a fetch, pull or clone made it.
fn last_fetch_move(repo: &gix::Repository, name: &str) -> Option<i64> {
    let reference = repo.try_find_reference(name).ok()??;
    let mut platform = reference.log_iter();
    let mut lines = platform.rev().ok()??;
    let line = lines.next()?.ok()?;
    let message = line.message.to_string();
    let by_fetch = ["fetch", "pull", "clone"].iter().any(|verb| {
        message
            .strip_prefix(verb)
            .is_some_and(|rest| rest.starts_with([':', ' ']))
    });
    by_fetch.then_some(line.signature.time.seconds)
}

/// The time `FETCH_HEAD` was written and the addresses it names, normalized.
fn fetch_head(path: &Path) -> Option<(i64, Vec<String>)> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let time = modified.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut urls: Vec<String> = text
        .lines()
        .filter_map(fetch_head_url)
        .map(|url| normalize(&url))
        .collect();
    urls.sort();
    urls.dedup();
    Some((time, urls))
}

/// `<id>\t[not-for-merge]\tbranch 'main' of github.com:acme/app` → `github.com:acme/app`.
fn fetch_head_url(line: &str) -> Option<String> {
    let description = line.splitn(3, '\t').nth(2)?;
    let (_, url) = description.rsplit_once(" of ")?;
    Some(url.trim().to_owned())
}

/// An address as `FETCH_HEAD` writes it: no user or password, no trailing `/` or `.git`.
pub(crate) fn normalize(url: &str) -> String {
    let mut url = url.trim().to_owned();
    if let Some(scheme_end) = url.find("://") {
        let rest = &url[scheme_end + 3..];
        let host_end = rest.find('/').unwrap_or(rest.len());
        if let Some(at) = rest[..host_end].rfind('@') {
            url = format!("{}{}", &url[..scheme_end + 3], &rest[at + 1..]);
        }
    } else if !url.starts_with(['/', '.'])
        && let Some(colon) = url.find(':')
        && let Some(at) = url[..colon].rfind('@')
    {
        // `git@github.com:acme/app`: git drops the user here too.
        url = url[at + 1..].to_owned();
    }
    let mut url = url.trim_end_matches('/');
    url = url.strip_suffix(".git").unwrap_or(url);
    url.trim_end_matches('/').to_owned()
}

fn read_record(path: &Path) -> BTreeMap<String, i64> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let (time, remote) = line.split_once('\t')?;
            Some((remote.to_owned(), time.parse().ok()?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_lose_credentials_and_suffixes() {
        assert_eq!(
            normalize("https://me:token@github.com/acme/app.git"),
            "https://github.com/acme/app"
        );
        assert_eq!(normalize("git@github.com:acme/app.git"), "github.com:acme/app");
        assert_eq!(normalize("/srv/git/app.git/"), "/srv/git/app");
        assert_eq!(normalize("ssh://git@host/a@b/app"), "ssh://host/a@b/app");
    }

    #[test]
    fn fetch_head_lines_name_their_remote() {
        assert_eq!(
            fetch_head_url("abc\t\tbranch 'main' of github.com:acme/app").as_deref(),
            Some("github.com:acme/app")
        );
        assert_eq!(
            fetch_head_url("abc\tnot-for-merge\tbranch 'x of y' of https://host/app").as_deref(),
            Some("https://host/app")
        );
        assert_eq!(fetch_head_url("garbage"), None);
    }
}
