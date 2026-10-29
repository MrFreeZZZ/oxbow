//! Reviewing a pull request: the open ones with GitHub's verdict and checks for the Reviews
//! screen, its head and base fetched to compare here, which files this account marked viewed,
//! and the draft of a review, kept in `.git/oxbow/reviews` until it is sent. A merge request on
//! GitLab is reviewed the same way; GitLab keeps no Viewed ticks, so they stay here too.
//!
//! The draft's comments are on lines of the commit the review started on. Sending them goes
//! through [`Call`](crate::pulls::Call)s: a pending review of that commit, its comments and
//! replies, then the verdict.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::GitCommand;
use crate::error::{Error, Result};
use crate::github::Client;
use crate::gitlab::GitLabProject;
use crate::pulls::{GitHubRepo, Spot, Verdict, graphql_error};
use crate::repo::Repo;

const STATUSES: &str = "query($owner: String!, $name: String!) { repository(owner: $owner, name: $name) { pullRequests(states: OPEN, first: 100, orderBy: {field: UPDATED_AT, direction: DESC}) { nodes { number reviewDecision commits(last: 1) { nodes { commit { statusCheckRollup { state } } } } } } } }";
const FILES: &str = "query($owner: String!, $name: String!, $number: Int!, $after: String) { repository(owner: $owner, name: $name) { pullRequest(number: $number) { files(first: 100, after: $after) { nodes { path viewerViewedState } pageInfo { hasNextPage endCursor } } reviews(states: PENDING, first: 5) { nodes { id viewerDidAuthor commit { oid } comments { totalCount } } } } } }";

/// GitHub's say on an open pull request, for the Reviews screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullStatus {
    pub number: u64,
    /// `approved`, `changesRequested` or `reviewRequired`; `None` when no review is required.
    pub decision: Option<String>,
    /// The checks of its head together: `success`, `failure`, `pending`, `error` or `expected`.
    pub checks: Option<String>,
}

/// A review this account started on GitHub and hasn't sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingReview {
    pub id: String,
    pub commit: Option<String>,
    pub comments: u32,
}

/// Where this account is with reviewing one pull request on GitHub.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewing {
    /// The files ticked Viewed. A file that changed since is `dismissed` by GitHub, unticked.
    pub viewed: Vec<String>,
    pub dismissed: Vec<String>,
    pub pending: Option<PendingReview>,
}

impl Client {
    /// Decision and checks of the repository's open pull requests, the latest updated first.
    pub fn pull_statuses(&self, repo: &GitHubRepo) -> Result<Vec<PullStatus>> {
        let answer = self.graphql(STATUSES, json!({ "owner": repo.owner, "name": repo.name }))?;
        Ok(answer
            .pointer("/data/repository/pullRequests/nodes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|pull| PullStatus {
                number: pull.get("number").and_then(Value::as_u64).unwrap_or(0),
                decision: pull.get("reviewDecision").and_then(Value::as_str).map(camel),
                checks: pull
                    .pointer("/commits/nodes/0/commit/statusCheckRollup/state")
                    .and_then(Value::as_str)
                    .map(camel),
            })
            .collect())
    }

    /// Which files of pull request `number` this account ticked Viewed, and its review there
    /// that isn't sent yet. At most 3000 files, as GitHub lists.
    pub fn viewing(&self, repo: &GitHubRepo, number: u64) -> Result<Viewing> {
        let mut out = Viewing::default();
        let mut after: Option<String> = None;
        for page in 0..30 {
            let vars = json!({ "owner": repo.owner, "name": repo.name, "number": number, "after": after });
            let answer = self.graphql(FILES, vars)?;
            let Some(pull) = answer.pointer("/data/repository/pullRequest") else {
                break;
            };
            for file in pull
                .pointer("/files/nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let path = file.get("path").and_then(Value::as_str).unwrap_or_default().to_owned();
                match file.get("viewerViewedState").and_then(Value::as_str) {
                    Some("VIEWED") => out.viewed.push(path),
                    Some("DISMISSED") => out.dismissed.push(path),
                    _ => {}
                }
            }
            if page == 0 {
                out.pending = pull
                    .pointer("/reviews/nodes")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .find(|r| r.get("viewerDidAuthor").and_then(Value::as_bool) == Some(true))
                    .map(|r| PendingReview {
                        id: r.get("id").and_then(Value::as_str).unwrap_or_default().to_owned(),
                        commit: r.pointer("/commit/oid").and_then(Value::as_str).map(str::to_owned),
                        comments: r.pointer("/comments/totalCount").and_then(Value::as_u64).unwrap_or(0) as u32,
                    });
            }
            let more = pull.pointer("/files/pageInfo/hasNextPage").and_then(Value::as_bool) == Some(true);
            after = pull
                .pointer("/files/pageInfo/endCursor")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if !more || after.is_none() {
                break;
            }
        }
        Ok(out)
    }

    fn graphql(&self, query: &str, variables: Value) -> Result<Value> {
        let response = self.call(&self.graphql_request(query, variables))?;
        match graphql_error(&response.body) {
            Some(message) => Err(Error::GitHub {
                status: Some(response.status),
                message,
            }),
            None => Ok(response.body),
        }
    }
}

/// `REVIEW_REQUIRED` as `reviewRequired`.
fn camel(text: &str) -> String {
    let mut out = String::new();
    for (i, word) in text.split('_').enumerate() {
        let word = word.to_lowercase();
        if i == 0 {
            out.push_str(&word);
        } else {
            let mut chars = word.chars();
            out.extend(chars.next().map(|c| c.to_ascii_uppercase()));
            out.push_str(chars.as_str());
        }
    }
    out
}

/// Where a pull request's commits are kept here: `refs/oxbow/pull/<number>/head` and `/base`,
/// out of every list, and `/reviewed` for the commit a draft review is on.
pub fn pull_ref(number: u64, part: &str) -> String {
    format!("refs/oxbow/pull/{number}/{part}")
}

/// Where a GitLab merge request's commits are kept here, as [`pull_ref`] keeps a pull request's:
/// `refs/oxbow/mr/<iid>/head`, `/base` and `/reviewed`.
pub fn mr_ref(number: u64, part: &str) -> String {
    format!("refs/oxbow/mr/{number}/{part}")
}

/// Where the reviews of one repository's pull requests are kept here: files named from the
/// repository on GitHub or the project on GitLab, commits under [`pull_ref`] or [`mr_ref`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reviewed {
    prefix: String,
    gitlab: bool,
}

impl Reviewed {
    pub fn github(repo: &GitHubRepo) -> Self {
        Reviewed {
            prefix: format!("{}-{}-", repo.owner, repo.name),
            gitlab: false,
        }
    }

    pub fn gitlab(project: &GitLabProject) -> Self {
        Reviewed {
            prefix: format!("gitlab-{}-", project.path.replace('/', "-")),
            gitlab: true,
        }
    }

    /// Where pull request (or merge request) `number`'s commit `part` is kept.
    pub fn reference(&self, number: u64, part: &str) -> String {
        if self.gitlab {
            mr_ref(number, part)
        } else {
            pull_ref(number, part)
        }
    }
}

// --- The draft -----------------------------------------------------------------------------------

/// A comment of the draft, on lines of the draft's commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftComment {
    /// Oxbow's own id, to edit or drop it.
    pub id: String,
    pub spot: Spot,
    pub body: String,
    pub time: i64,
}

/// A reply of the draft to a thread on GitHub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftReply {
    pub id: String,
    /// The thread's first comment, by its REST id.
    pub to: u64,
    /// The thread's GraphQL id.
    pub thread: String,
    pub body: String,
    pub time: i64,
}

/// A review written here and not sent yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDraft {
    /// The pull request's head when the review started: the comments are on its lines.
    pub commit: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub verdict: Option<Verdict>,
    #[serde(default)]
    pub comments: Vec<DraftComment>,
    #[serde(default)]
    pub replies: Vec<DraftReply>,
}

impl Repo {
    /// `git fetch` of a pull request's head and base into [`pull_ref`]s, so its changes can be
    /// read here; a pull request from a fork included.
    pub fn pull_fetch_command(&self, remote: &str, number: u64, base: &str) -> GitCommand {
        GitCommand::new([
            "fetch".to_owned(),
            "--no-tags".to_owned(),
            "--no-write-fetch-head".to_owned(),
            remote.to_owned(),
            format!("+refs/pull/{number}/head:{}", pull_ref(number, "head")),
            format!("+refs/heads/{base}:{}", pull_ref(number, "base")),
        ])
        .comment(format!(
            "pull request #{number} and {base}, kept out of the branch lists"
        ))
    }

    /// `git fetch` of a GitLab merge request's head (GitLab keeps it as
    /// `refs/merge-requests/<iid>/head`, a fork's too) and its target into [`mr_ref`]s.
    pub fn merge_request_fetch_command(&self, remote: &str, number: u64, base: &str) -> GitCommand {
        GitCommand::new([
            "fetch".to_owned(),
            "--no-tags".to_owned(),
            "--no-write-fetch-head".to_owned(),
            remote.to_owned(),
            format!("+refs/merge-requests/{number}/head:{}", mr_ref(number, "head")),
            format!("+refs/heads/{base}:{}", mr_ref(number, "base")),
        ])
        .comment(format!(
            "merge request !{number} and {base}, kept out of the branch lists"
        ))
    }

    fn draft_file(&self, place: &Reviewed, number: u64) -> PathBuf {
        self.common_dir()
            .join("oxbow")
            .join("reviews")
            .join(format!("{}{number}.json", place.prefix))
    }

    /// The draft review of pull request `number`, if one was started here.
    pub fn review_draft(&self, place: &Reviewed, number: u64) -> Result<Option<ReviewDraft>> {
        match std::fs::read_to_string(self.draft_file(place, number)) {
            Ok(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|err| Error::Git(format!("The draft review of #{number} can't be read: {err}"))),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(Error::Git(err.to_string())),
        }
    }

    /// Keep `draft`, written whole to a new file that then takes the old one's place. Its commit
    /// gets a ref, so it stays here after the pull request moves on.
    pub fn save_review_draft(&self, place: &Reviewed, number: u64, draft: &ReviewDraft) -> Result<()> {
        let io = |err: std::io::Error| Error::Git(err.to_string());
        let file = self.draft_file(place, number);
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let text = serde_json::to_string_pretty(draft).map_err(|err| Error::Git(err.to_string()))?;
        let new = file.with_extension(format!("json.{}", std::process::id()));
        std::fs::write(&new, text).map_err(io)?;
        std::fs::rename(&new, &file).map_err(io)?;
        let reviewed = place.reference(number, "reviewed");
        if self.commit_id(&reviewed).ok().as_deref() != Some(draft.commit.as_str()) {
            // Without the commit here (not fetched yet) there's nothing to keep.
            let _ = self.run(&GitCommand::new(["update-ref", &reviewed, &draft.commit]));
        }
        Ok(())
    }

    /// Put the draft away once it is sent, or thrown away: a copy goes to `.git/oxbow/removed`
    /// for 30 days.
    pub fn drop_review_draft(&self, place: &Reviewed, number: u64) -> Result<()> {
        let file = self.draft_file(place, number);
        if file.exists() {
            let io = |err: std::io::Error| Error::Git(err.to_string());
            let root = self.common_dir().join("oxbow").join("removed");
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or_default();
            crate::stash::prune_removed(&root, now);
            let folder = root.join(format!("{now}-review-{number}-{}", std::process::id()));
            std::fs::create_dir_all(&folder).map_err(io)?;
            std::fs::rename(&file, folder.join(file.file_name().unwrap_or_default())).map_err(io)?;
        }
        let _ = self.run(&GitCommand::new([
            "update-ref",
            "-d",
            &place.reference(number, "reviewed"),
        ]));
        Ok(())
    }

    /// The draft reviews kept here, by pull request number, for the Reviews screen.
    pub fn review_drafts(&self, place: &Reviewed) -> HashMap<u64, ReviewDraft> {
        let prefix = &place.prefix;
        let dir = self.common_dir().join("oxbow").join("reviews");
        let Ok(entries) = std::fs::read_dir(dir) else {
            return HashMap::new();
        };
        entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let number = name.strip_prefix(prefix)?.strip_suffix(".json")?.parse::<u64>().ok()?;
                let draft = self.review_draft(place, number).ok().flatten()?;
                Some((number, draft))
            })
            .collect()
    }
}

impl Repo {
    fn viewed_file(&self, place: &Reviewed, number: u64) -> PathBuf {
        self.common_dir()
            .join("oxbow")
            .join("reviews")
            .join(format!("{}{number}.viewed.json", place.prefix))
    }

    /// The files of merge request `number` ticked Viewed here, for a server that keeps no ticks
    /// (GitLab). A file whose content at the fetched head is not what it was when ticked is
    /// `dismissed`, as GitHub does.
    pub fn viewed_here(&self, place: &Reviewed, number: u64) -> Viewing {
        let kept: HashMap<String, String> = std::fs::read_to_string(self.viewed_file(place, number))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        let mut paths: Vec<&String> = kept.keys().collect();
        paths.sort();
        let now = self.blobs_at(&place.reference(number, "head"), &paths);
        let mut out = Viewing::default();
        for (path, was) in paths.into_iter().zip(now) {
            if now_or_gone(&was) == kept.get(path).map(String::as_str).unwrap_or_default() {
                out.viewed.push(path.clone());
            } else {
                out.dismissed.push(path.clone());
            }
        }
        out
    }

    /// Tick or untick `path` of merge request `number` Viewed here, at its content on the
    /// fetched head.
    pub fn set_viewed_here(&self, place: &Reviewed, number: u64, path: &str, viewed: bool) -> Result<()> {
        let io = |err: std::io::Error| Error::Git(err.to_string());
        let file = self.viewed_file(place, number);
        let mut kept: HashMap<String, String> = std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        if viewed {
            let path = path.to_owned();
            let blob = self.blobs_at(&place.reference(number, "head"), &[&path]).remove(0);
            kept.insert(path, now_or_gone(&blob));
        } else {
            kept.remove(path);
        }
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let text = serde_json::to_string_pretty(&kept).map_err(|err| Error::Git(err.to_string()))?;
        let new = file.with_extension(format!("json.{}", std::process::id()));
        std::fs::write(&new, text).map_err(io)?;
        std::fs::rename(&new, &file).map_err(io)
    }

    /// The blob of each of `paths` at `rev`, `None` where it has none (deleted there).
    fn blobs_at(&self, rev: &str, paths: &[&String]) -> Vec<Option<String>> {
        if paths.is_empty() {
            return Vec::new();
        }
        let query: String = paths.iter().map(|p| format!("{rev}:{p}\n")).collect();
        let lines: Vec<String> = self
            .run_with_input(
                &GitCommand::new(["cat-file", "--batch-check=%(objectname) %(objecttype)"]),
                Some(query.as_bytes()),
            )
            .map(|out| out.stdout.lines().map(str::to_owned).collect())
            .unwrap_or_default();
        (0..paths.len())
            .map(|i| {
                lines
                    .get(i)
                    .and_then(|line| line.strip_suffix(" blob"))
                    .map(str::to_owned)
            })
            .collect()
    }
}

/// A blob as the Viewed file keeps it: its id, or `deleted`.
fn now_or_gone(blob: &Option<String>) -> String {
    blob.clone().unwrap_or_else(|| "deleted".to_owned())
}
