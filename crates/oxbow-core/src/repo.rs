use std::path::{Path, PathBuf};

use gix::bstr::ByteSlice;
use gix::refs::Category;
use serde::Serialize;

use crate::error::{Error, Result};
use crate::remote::Tracking;

/// An open repository. Cheap to share between threads; every call works on its own
/// thread-local handle.
#[derive(Clone)]
pub struct Repo {
    shared: gix::ThreadSafeRepository,
    workdir: PathBuf,
}

/// What `HEAD` points at.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HeadInfo {
    /// Checked-out branch, `None` when `HEAD` is detached.
    pub branch: Option<String>,
    /// Commit `HEAD` points at, `None` in a repository without commits.
    pub commit: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum RefKind {
    Local,
    Remote,
    Tag,
    /// An entry of `refs/stash` (only used for labels, `refs()` never returns it).
    Stash,
}

/// A branch or tag and the commit it points at.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RefInfo {
    /// Short name: `main`, `origin/main`, `v1.0.0`.
    pub name: String,
    pub kind: RefKind,
    /// Commit the ref points at (tags are peeled).
    pub target: String,
    /// For remote branches, the remote's name.
    pub remote: Option<String>,
    /// For local branches, the upstream and how far apart the two are.
    pub tracking: Option<Tracking>,
}

impl Repo {
    /// Open the repository that contains `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let shared = gix::ThreadSafeRepository::discover(path).map_err(|_| Error::NotARepository {
            path: path.display().to_string(),
        })?;
        let workdir = shared.work_dir().ok_or(Error::Bare)?.to_path_buf();
        Ok(Repo { shared, workdir })
    }

    /// Root of the working tree.
    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// Folder name of the working tree, used as the repository's display name.
    pub fn name(&self) -> String {
        self.workdir.file_name().map_or_else(
            || self.workdir.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    }

    pub(crate) fn local(&self) -> gix::Repository {
        let mut repo = self.shared.to_thread_local();
        repo.object_cache_size_if_unset(64 * 1024 * 1024);
        repo
    }

    pub fn head(&self) -> Result<HeadInfo> {
        let repo = self.local();
        let branch = repo
            .head_name()
            .map_err(Error::git)?
            .map(|name| name.shorten().to_str_lossy().into_owned());
        let commit = repo.head_id().ok().map(|id| id.to_string());
        Ok(HeadInfo { branch, commit })
    }

    /// All local branches, remote branches and tags. Symbolic refs like `origin/HEAD` are skipped.
    pub fn refs(&self) -> Result<Vec<RefInfo>> {
        let repo = self.local();
        let platform = repo.references().map_err(Error::git)?;
        let mut out = Vec::new();
        for reference in platform.all().map_err(Error::git)? {
            let Ok(mut reference) = reference else { continue };
            if matches!(reference.target(), gix::refs::TargetRef::Symbolic(_)) {
                continue;
            }
            let Some((category, short)) = reference.name().category_and_short_name() else {
                continue;
            };
            let kind = match category {
                Category::LocalBranch => RefKind::Local,
                Category::RemoteBranch => RefKind::Remote,
                Category::Tag => RefKind::Tag,
                _ => continue,
            };
            let name = short.to_str_lossy().into_owned();
            let Ok(id) = reference.peel_to_id() else { continue };
            // Tags may point at trees or blobs; only commits belong in the history.
            let is_commit = repo
                .find_header(id)
                .is_ok_and(|header| header.kind() == gix::object::Kind::Commit);
            if !is_commit {
                continue;
            }
            let remote = (kind == RefKind::Remote).then(|| name.split('/').next().unwrap_or_default().to_owned());
            out.push(RefInfo {
                name,
                kind,
                target: id.to_string(),
                remote,
                tracking: None,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// Names of the configured remotes.
    pub fn remotes(&self) -> Vec<String> {
        let repo = self.local();
        repo.remote_names().iter().map(|name| name.to_string()).collect()
    }

    /// Stash entries, newest (`stash@{0}`) first.
    pub fn stashes(&self) -> Result<Vec<StashInfo>> {
        let repo = self.local();
        let Some(reference) = repo.try_find_reference("refs/stash").map_err(Error::git)? else {
            return Ok(Vec::new());
        };
        let mut platform = reference.log_iter();
        let Some(lines) = platform.rev().map_err(Error::git)? else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for (index, line) in lines.enumerate() {
            let line = line.map_err(Error::git)?;
            out.push(StashInfo {
                index,
                id: line.new_oid.to_string(),
                message: line.message.to_str_lossy().into_owned(),
            });
        }
        Ok(out)
    }
}

/// One entry of the stash.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StashInfo {
    /// `n` in `stash@{n}`.
    pub index: usize,
    /// The stash commit. Its first parent is the commit the stash was made on.
    pub id: String,
    pub message: String,
}
