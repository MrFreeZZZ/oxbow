//! Reviewing a pull request: the open ones with GitHub's verdict and checks for the Reviews
//! screen, its head and base fetched to compare here, which files this account marked viewed,
//! and the draft of a review, kept in `.git/oxbow/reviews` until it is sent. A merge request on
//! GitLab is reviewed the same way; GitLab keeps no Viewed ticks, so they stay here too.
//!
//! The draft's comments are on lines of the commit the review started on. Sending them goes
//! through [`Call`](crate::pulls::Call)s: a pending review of that commit, its comments and
//! replies, then the verdict.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, Weak};

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

/// The full forge identity. A delimiter in a name must never join two projects' drafts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reviewed {
    pub gitlab: bool,
    pub host: String,
    pub project: String,
}

impl Reviewed {
    pub fn github(repo: &GitHubRepo) -> Self {
        Self {
            gitlab: false,
            host: "github.com".into(),
            project: repo.full_name().to_lowercase(),
        }
    }

    pub fn gitlab(project: &GitLabProject) -> Self {
        Self {
            gitlab: true,
            host: project.host.to_lowercase(),
            project: project.path.clone(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.host.is_empty()
            || self.host.contains(['/', '\\', '\0'])
            || (!self.gitlab && self.host != "github.com")
            || self.project.split('/').count() < 2
            || self
                .project
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            || self.project.contains(['\\', '\0'])
        {
            return Err(Error::Git("Invalid review project.".into()));
        }
        Ok(())
    }

    fn prefix(&self) -> String {
        let identity = serde_json::to_vec(self).expect("review identity serializes");
        let mut hash = gix::hash::hasher(gix::hash::Kind::Sha1);
        hash.update(&identity);
        format!("v2-{}-", hash.try_finalize().expect("SHA-1 is available").to_hex())
    }

    pub fn draft_name(&self, number: u64) -> String {
        format!("{}{number}.json", self.prefix())
    }

    /// Project-scoped refs, shared by its drafts and fetched diffs.
    pub fn reference(&self, number: u64, part: &str) -> String {
        format!("refs/oxbow/reviews/{}{number}/{part}", self.prefix())
    }

    fn legacy_name(&self, number: u64) -> String {
        let prefix = if self.gitlab {
            format!("gitlab-{}", self.project.replace('/', "-"))
        } else {
            self.project.replace('/', "-")
        };
        format!("{prefix}-{number}.json")
    }
}

// A worktree's reviews live in the common directory. Serialize read/check/write and removal
// across windows, without holding an unrelated repository's saves up.
type FileLocks = HashMap<PathBuf, Weak<Mutex<()>>>;
static FILE_LOCKS: LazyLock<Mutex<FileLocks>> = LazyLock::new(|| Mutex::new(HashMap::new()));
fn file_lock(path: &Path) -> Arc<Mutex<()>> {
    let mut locks = FILE_LOCKS.lock().unwrap_or_else(|e| e.into_inner());
    locks.retain(|_, lock| lock.strong_count() > 0);
    let key = path
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .map_or_else(|| path.to_path_buf(), |p| p.join(path.file_name().unwrap_or_default()));
    if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(key, Arc::downgrade(&lock));
    lock
}

fn atomic_json(file: &Path, value: &impl Serialize) -> Result<()> {
    let io = |err: std::io::Error| Error::Git(err.to_string());
    let dir = file
        .parent()
        .ok_or_else(|| Error::Git("Review folder is missing.".into()))?;
    std::fs::create_dir_all(dir).map_err(io)?;
    let mut new = tempfile::NamedTempFile::new_in(dir).map_err(io)?;
    serde_json::to_writer_pretty(&mut new, value).map_err(Error::git)?;
    new.flush().map_err(io)?;
    new.persist(file).map_err(|err| io(err.error))?;
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct ReviewRecord {
    revision: String,
    draft: Option<ReviewDraft>,
}

fn next_revision() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{nanos}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// Read with the draft: the next write must still replace this version.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDraftState {
    pub draft: Option<ReviewDraft>,
    pub revision: String,
    pub head_ref: String,
    pub base_ref: String,
    pub file: String,
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
    /// `git fetch` of a pull request's head and base into its project refs, so its changes can be
    /// read here; a pull request from a fork included.
    pub fn pull_fetch_command(&self, place: &Reviewed, remote: &str, number: u64, base: &str) -> GitCommand {
        GitCommand::new([
            "fetch".to_owned(),
            "--no-tags".to_owned(),
            "--no-write-fetch-head".to_owned(),
            remote.to_owned(),
            format!("+refs/pull/{number}/head:{}", place.reference(number, "head")),
            format!("+refs/heads/{base}:{}", place.reference(number, "base")),
        ])
        .comment(format!(
            "pull request #{number} and {base}, kept out of the branch lists"
        ))
    }

    /// `git fetch` of a GitLab merge request's head (GitLab keeps it as
    /// `refs/merge-requests/<iid>/head`, a fork's too) and its target into project refs.
    pub fn merge_request_fetch_command(&self, place: &Reviewed, remote: &str, number: u64, base: &str) -> GitCommand {
        GitCommand::new([
            "fetch".to_owned(),
            "--no-tags".to_owned(),
            "--no-write-fetch-head".to_owned(),
            remote.to_owned(),
            format!("+refs/merge-requests/{number}/head:{}", place.reference(number, "head")),
            format!("+refs/heads/{base}:{}", place.reference(number, "base")),
        ])
        .comment(format!(
            "merge request !{number} and {base}, kept out of the branch lists"
        ))
    }

    fn draft_file(&self, place: &Reviewed, number: u64) -> PathBuf {
        self.common_dir()
            .canonicalize()
            .unwrap_or_else(|_| self.common_dir())
            .join("oxbow")
            .join("reviews")
            .join(place.draft_name(number))
    }

    /// The draft review of pull request `number`, if one was started here.
    pub fn review_draft(&self, place: &Reviewed, number: u64) -> Result<Option<ReviewDraft>> {
        Ok(self.review_draft_state(place, number)?.draft)
    }

    fn read_draft(&self, place: &Reviewed, number: u64) -> Result<Option<ReviewRecord>> {
        let file = self.draft_file(place, number);
        if !file.exists() {
            let old_name = place.legacy_name(number);
            let legacy = std::fs::read_dir(file.parent().unwrap_or(Path::new("")))
                .ok()
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
                .find(|path| {
                    path.file_name().is_some_and(|name| {
                        let name = name.to_string_lossy();
                        if place.gitlab {
                            name == old_name
                        } else {
                            name.eq_ignore_ascii_case(&old_name)
                        }
                    })
                });
            if let Some(legacy) = legacy {
                return Err(Error::Changed(format!(
                    "An older review draft is preserved at {}. Its project was not recorded; move it to {} only after checking which project it belongs to.",
                    legacy.display(),
                    file.display()
                )));
            }
        }
        match std::fs::read_to_string(self.draft_file(place, number)) {
            Ok(text) => serde_json::from_str::<ReviewRecord>(&text)
                // A legacy file moved here explicitly by its owner gains a revision on save.
                .or_else(|_| {
                    serde_json::from_str::<ReviewDraft>(&text).map(|draft| ReviewRecord {
                        revision: format!(
                            "imported:{}",
                            gix::objs::compute_hash(gix::hash::Kind::Sha1, gix::object::Kind::Blob, text.as_bytes())
                                .expect("SHA-1 is available")
                        ),
                        draft: Some(draft),
                    })
                })
                .map(Some)
                .map_err(|err| Error::Git(format!("The draft review of #{number} can't be read: {err}"))),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(Error::Git(err.to_string())),
        }
    }

    pub fn review_draft_state(&self, place: &Reviewed, number: u64) -> Result<ReviewDraftState> {
        place.validate()?;
        let lock = file_lock(&self.draft_file(place, number));
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        self.read_draft_state(place, number)
    }

    fn read_draft_state(&self, place: &Reviewed, number: u64) -> Result<ReviewDraftState> {
        let record = self.read_draft(place, number)?;
        let (draft, revision) = record.map_or((None, "missing".into()), |r| (r.draft, r.revision));
        Ok(ReviewDraftState {
            revision,
            draft,
            head_ref: place.reference(number, "head"),
            base_ref: place.reference(number, "base"),
            file: place.draft_name(number),
        })
    }

    /// A checked save also serializes deletion: a late old editor must not revive a sent draft.
    pub fn update_review_draft(
        &self,
        place: &Reviewed,
        number: u64,
        draft: Option<&ReviewDraft>,
        expected: &str,
    ) -> Result<String> {
        place.validate()?;
        let lock = file_lock(&self.draft_file(place, number));
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        if self.read_draft_state(place, number)?.revision != expected {
            return Err(Error::Changed("This review draft changed in another window. Reopen the review before saving again; your text is kept in this window.".into()));
        }
        match draft {
            Some(draft) => self.write_draft(place, number, draft)?,
            None => self.remove_draft(place, number)?,
        }
        Ok(self.read_draft_state(place, number)?.revision)
    }

    pub fn save_review_draft(&self, place: &Reviewed, number: u64, draft: &ReviewDraft) -> Result<()> {
        place.validate()?;
        let lock = file_lock(&self.draft_file(place, number));
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        self.write_draft(place, number, draft)
    }

    fn write_draft(&self, place: &Reviewed, number: u64, draft: &ReviewDraft) -> Result<()> {
        atomic_json(
            &self.draft_file(place, number),
            &ReviewRecord {
                revision: next_revision(),
                draft: Some(draft.clone()),
            },
        )?;
        let reviewed = place.reference(number, "reviewed");
        if self.commit_id(&reviewed).ok().as_deref() != Some(draft.commit.as_str()) {
            let _ = self.run(&GitCommand::new(["update-ref", &reviewed, &draft.commit]));
        }
        Ok(())
    }

    /// Put the draft away once it is sent, or thrown away: a copy goes to `.git/oxbow/removed`
    /// for 30 days.
    pub fn drop_review_draft(&self, place: &Reviewed, number: u64) -> Result<()> {
        place.validate()?;
        let lock = file_lock(&self.draft_file(place, number));
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        self.remove_draft(place, number)
    }

    fn remove_draft(&self, place: &Reviewed, number: u64) -> Result<()> {
        let file = self.draft_file(place, number);
        if self
            .read_draft(place, number)?
            .is_some_and(|record| record.draft.is_some())
        {
            let io = |err: std::io::Error| Error::Git(err.to_string());
            let root = self.common_dir().join("oxbow").join("removed");
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or_default();
            crate::stash::prune_removed(&root, now);
            std::fs::create_dir_all(&root).map_err(io)?;
            let folder = tempfile::Builder::new()
                .prefix(&format!("{now}-review-{number}-"))
                .tempdir_in(&root)
                .map_err(io)?
                .keep();
            std::fs::copy(&file, folder.join(file.file_name().unwrap_or_default())).map_err(io)?;
        }
        // Keep a tombstone revision so an editor opened before send/discard cannot revive it.
        atomic_json(
            &file,
            &ReviewRecord {
                revision: next_revision(),
                draft: None,
            },
        )?;
        let _ = self.run(&GitCommand::new([
            "update-ref",
            "-d",
            &place.reference(number, "reviewed"),
        ]));
        Ok(())
    }

    /// The draft reviews kept here, by pull request number, for the Reviews screen.
    pub fn review_drafts(&self, place: &Reviewed) -> HashMap<u64, ReviewDraft> {
        let prefix = place.prefix();
        let dir = self.common_dir().join("oxbow").join("reviews");
        let Ok(entries) = std::fs::read_dir(dir) else {
            return HashMap::new();
        };
        entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let number = name.strip_prefix(&prefix)?.strip_suffix(".json")?.parse::<u64>().ok()?;
                let draft = self.review_draft(place, number).ok().flatten()?;
                Some((number, draft))
            })
            .collect()
    }
}

impl Repo {
    fn viewed_file(&self, place: &Reviewed, number: u64) -> PathBuf {
        self.common_dir()
            .canonicalize()
            .unwrap_or_else(|_| self.common_dir())
            .join("oxbow")
            .join("reviews")
            .join(format!("{}{number}.viewed.json", place.prefix()))
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
        place.validate()?;
        let file = self.viewed_file(place, number);
        let lock = file_lock(&file);
        let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
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
        atomic_json(&file, &kept)
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
