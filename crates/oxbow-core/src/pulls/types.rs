//! Pull request models and calls shared by GitHub and GitLab.

use serde::{Deserialize, Serialize};

/// A repository on github.com, the way a remote points at it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubRepo {
    pub owner: String,
    pub name: String,
    /// The remote that points at it, e.g. `origin`.
    pub remote: String,
}

impl GitHubRepo {
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

// --- What the screen shows ---------------------------------------------------------------------

/// A GitHub account as it appears on a pull request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub login: String,
    pub avatar_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PullState {
    Open,
    Closed,
    Merged,
}

/// A pull request in the repository's list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullSummary {
    pub number: u64,
    pub title: String,
    pub state: PullState,
    pub draft: bool,
    /// The branch it brings in.
    pub head: String,
    pub head_sha: String,
    /// The account the head branch lives under: another one for a pull request from a fork.
    pub head_owner: Option<String>,
    /// The branch it goes into.
    pub base: String,
    pub html_url: String,
    pub body: String,
    pub author: Person,
    pub created: i64,
    pub updated: i64,
    /// GraphQL's id, for the calls only GraphQL has.
    pub node_id: String,
    /// Reviews asked for and not given yet: logins, and teams as `org/team`.
    pub requested: Vec<String>,
}

/// Everything the Pull Request screen shows about one pull request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub load_issues: Vec<LoadIssue>,
    #[serde(flatten)]
    pub summary: PullSummary,
    pub additions: u32,
    pub deletions: u32,
    pub changed_files: u32,
    pub commit_count: u32,
    /// Whether it merges without conflicts; `None` while GitHub is still working it out.
    pub mergeable: Option<bool>,
    /// GitHub's verdict: `clean`, `unstable` (a check that isn't required fails), `blocked`,
    /// `behind`, `dirty` (conflicts), `draft`, `has_hooks` or `unknown`.
    pub mergeable_state: String,
    pub merged_at: Option<i64>,
    pub merge_commit: Option<String>,
    pub merged_by: Option<String>,
    /// The method auto-merge will use, when it is on.
    pub auto_merge: Option<String>,
    pub commits: Vec<PullCommit>,
    /// `commits` has every commit: GitHub lists at most 250, so a bigger pull request can't
    /// say which commits a restack leaves out.
    pub commits_complete: bool,
    /// Each reviewer's latest say, in the order they first spoke.
    pub reviews: Vec<Review>,
    /// The conversation, oldest first; the description is not part of it.
    pub timeline: Vec<Entry>,
    pub checks: Vec<Check>,
    /// Approvals the base branch's rules ask for, when this account may read them.
    pub required_approvals: Option<u32>,
    pub settings: RepoSettings,
    /// What only a merge request on GitLab has; `None` on GitHub.
    pub gitlab: Option<crate::gitlab_api::GitLabFacts>,
}

/// An absent issue means a complete result, including a genuinely empty one.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadIssue {
    pub section: String,
    pub state: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullList {
    pub items: Vec<PullSummary>,
    pub issues: Vec<LoadIssue>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullCommit {
    pub sha: String,
    pub summary: String,
    pub message: String,
    pub author: String,
    pub time: i64,
    /// Lines added and deleted, when the commit is here to count them.
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub author: Person,
    /// `approved`, `changesRequested`, `commented` or `dismissed`.
    pub state: String,
    pub time: i64,
}

/// A line of code a review comment is about, with the lines above it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HunkLine {
    /// Its number in the new file; `None` for a deleted line.
    pub number: Option<u32>,
    pub text: String,
    /// The line the comment is on.
    pub mark: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub author: Person,
    pub body: String,
    pub time: i64,
}

/// One entry of the conversation.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Entry {
    Comment {
        author: Person,
        body: String,
        time: i64,
    },
    Review {
        author: Person,
        state: String,
        body: String,
        time: i64,
    },
    /// Comments on a line of code, the first one and its replies.
    Thread {
        /// The first comment's id, which replies go to.
        id: u64,
        /// GraphQL's id of the thread, for resolving it and replying in a review.
        node: Option<String>,
        path: String,
        line: Option<u32>,
        /// The first line of a comment on several lines.
        start_line: Option<u32>,
        /// `RIGHT` for a line of the new file, `LEFT` for a deleted one.
        side: String,
        hunk: Vec<HunkLine>,
        comments: Vec<Comment>,
        /// `None` when GitHub didn't say.
        resolved: Option<bool>,
        /// The code changed since, so the comment is about an older version.
        outdated: bool,
        /// The commit whose lines it is on, when GitHub or GitLab says: a suggestion in it is
        /// applied from that version of the file.
        #[serde(default)]
        commit: Option<String>,
        time: i64,
    },
    /// Something that happened, in one line: "pushed 2 commits".
    Event {
        actor: String,
        text: String,
        /// `push`, `ok`, `fail`, `review`, `draft` or `dot`.
        icon: String,
        /// `ok` or `bad` for a colored dot.
        tone: Option<String>,
        time: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub name: String,
    pub detail: String,
    /// `ok`, `fail`, `run` or `skip`; `warn` for a GitLab job that failed but may.
    pub state: String,
    /// How long it took, once it finished.
    pub seconds: Option<i64>,
    pub url: Option<String>,
}

/// What the repository allows when merging.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoSettings {
    pub merge_commit: bool,
    pub squash: bool,
    pub rebase: bool,
    pub auto_merge: bool,
    /// GitHub deletes the head branch itself after merging.
    pub delete_branch_on_merge: bool,
    pub default_branch: String,
    /// The account may push, and so merge.
    pub can_push: bool,
}

// --- What changes them -------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Method {
    Merge,
    Squash,
    Rebase,
}

/// What a review says of the pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    Comment,
    Approve,
    RequestChanges,
}

/// The lines a comment is on, as GitHub numbers them: `line` (and `start_line` for several) of
/// the new file on the `RIGHT`, of the old one on the `LEFT` for deleted lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spot {
    pub path: String,
    pub line: u32,
    pub side: String,
    #[serde(default)]
    pub start_line: Option<u32>,
    /// Both numbers of its last and first line, as GitLab names a line of a diff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<SpotLines>,
}

/// The last line of a spot and, for several, its first, as GitLab numbers lines of a diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotLines {
    pub end: LinePair,
    #[serde(default)]
    pub start: Option<LinePair>,
}

/// A line of a diff by both files' counts at its row: an added line has the old file's next
/// number, a deleted one the new file's next, as GitLab counts them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinePair {
    pub old: u32,
    pub new: u32,
    /// `added`, `removed` or `context`.
    pub kind: String,
}

/// What a merge request on GitLab is opened with besides a pull request's fields: people by
/// their account ids, labels by name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLabCreate {
    #[serde(default)]
    pub reviewer_ids: Vec<u64>,
    #[serde(default)]
    pub assignee_ids: Vec<u64>,
    #[serde(default)]
    pub labels: Vec<String>,
    /// GitLab deletes the source branch once it is merged.
    #[serde(default)]
    pub remove_source_branch: bool,
    /// It is squashed into one commit when merged.
    #[serde(default)]
    pub squash: bool,
}

/// A change to a pull request, shown as its request before it is sent. `number` 0 means the
/// pull request a `Create` earlier in the same run made, a `review` of `None` the review a
/// `StartReview` earlier in the run started.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Call {
    Merge {
        number: u64,
        method: Method,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        message: Option<String>,
        /// Merge only if the head is still this commit.
        sha: String,
        /// GitLab deletes the source branch once merged (GitHub's is deleted by a push).
        #[serde(default)]
        delete_branch: bool,
    },
    Create {
        head: String,
        base: String,
        title: String,
        body: String,
        draft: bool,
        /// What only a merge request on GitLab is opened with.
        #[serde(default)]
        gitlab: Option<GitLabCreate>,
    },
    RequestReviewers {
        number: u64,
        reviewers: Vec<String>,
        /// GitLab: every reviewer it is to have, by account id, the ones it has included.
        #[serde(default)]
        ids: Vec<u64>,
    },
    Update {
        number: u64,
        #[serde(default)]
        base: Option<String>,
        #[serde(default)]
        body: Option<String>,
    },
    ReadyForReview {
        number: u64,
        id: String,
        /// The title it gets: GitLab marks a draft by the title's `Draft:`.
        #[serde(default)]
        title: Option<String>,
    },
    /// Make it a draft again.
    ConvertToDraft {
        number: u64,
        id: String,
        #[serde(default)]
        title: Option<String>,
    },
    /// Close it, or open it again.
    SetState { number: u64, open: bool },
    /// Merge the base into the head on GitHub, like its Update branch button, if the head is
    /// still this commit.
    UpdateBranch { number: u64, sha: String },
    /// Turn auto-merge on with `method`, or off without one.
    AutoMerge {
        number: u64,
        id: String,
        #[serde(default)]
        method: Option<Method>,
    },
    /// Tick or untick a file's Viewed box, as on GitHub.
    Viewed { id: String, path: String, viewed: bool },
    /// A comment on lines of `commit`, sent at once rather than in a review.
    Comment {
        number: u64,
        commit: String,
        spot: Spot,
        body: String,
    },
    /// A reply, sent at once, to the thread comment `to` starts.
    Reply {
        number: u64,
        to: u64,
        body: String,
        /// GitLab: the discussion, by its id.
        #[serde(default)]
        thread: Option<String>,
    },
    /// Resolve the thread with GraphQL id `thread`, or open it again.
    Resolve {
        thread: String,
        resolved: bool,
        /// GitLab: the merge request the discussion is on.
        #[serde(default)]
        number: u64,
    },
    /// Start a review of `commit`, pending until it is submitted.
    StartReview { id: String, commit: String },
    /// A comment in a pending review.
    ReviewComment {
        #[serde(default)]
        review: Option<String>,
        spot: Spot,
        body: String,
    },
    /// A reply in a pending review to the thread with GraphQL id `thread`.
    ReviewReply {
        #[serde(default)]
        review: Option<String>,
        thread: String,
        body: String,
    },
    /// Send a pending review with its verdict and summary.
    SubmitReview {
        #[serde(default)]
        review: Option<String>,
        verdict: Verdict,
        body: String,
        /// GitLab: take back the approval this account gave before.
        #[serde(default)]
        revoke: bool,
    },
    /// Throw away a pending review.
    DeleteReview {
        #[serde(default)]
        review: Option<String>,
    },
}

impl Call {
    /// The review a `StartReview` made, for the calls of the run that go into it.
    pub fn in_review(&mut self, id: &str) {
        match self {
            Call::ReviewComment { review, .. }
            | Call::ReviewReply { review, .. }
            | Call::SubmitReview { review, .. }
            | Call::DeleteReview { review }
                if review.is_none() =>
            {
                *review = Some(id.to_owned());
            }
            _ => {}
        }
    }
}
