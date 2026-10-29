//! Browse Files at This Commit: the folders and files of any commit's tree, read by gix one
//! folder at a time; a file's text as it is there; and the file saved elsewhere as it is there.

use std::path::Path;

use gix::object::tree::EntryKind;
use serde::Serialize;

use crate::error::{Error, Result};
use crate::repo::Repo;

/// A file bigger than this shows no text: Save As still writes it.
pub const MAX_TEXT: u64 = 8 * 1024 * 1024;

/// What a Git LFS pointer file starts with.
const LFS_POINTER: &[u8] = b"version https://git-lfs.github.com/spec/v1";

/// One entry of a folder in a commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeItem {
    pub name: String,
    /// From the repository's top folder.
    pub path: String,
    pub kind: TreeKind,
    /// Bytes of a file; 0 for the others.
    pub size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TreeKind {
    Folder,
    File,
    /// A file that runs as a program.
    Executable,
    /// A symbolic link: its text is where it points.
    Link,
    /// Another repository's commit.
    Submodule,
}

/// A file as a commit has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAt {
    pub path: String,
    /// Its blob, for drawing a picture from its bytes.
    pub id: String,
    pub size: u64,
    /// Its text, or None for a binary file or one over [`MAX_TEXT`].
    pub text: Option<String>,
    pub binary: bool,
    /// A Git LFS pointer: the file itself is kept on the LFS server.
    pub lfs: bool,
}

impl Repo {
    /// The folder `dir` ("" for the top) of `rev`'s tree: folders first, then files, by name.
    pub fn tree_at(&self, rev: &str, dir: &str) -> Result<Vec<TreeItem>> {
        let repo = self.local();
        let mut tree = tree_of(&repo, rev)?;
        if !dir.is_empty() {
            let entry = tree
                .peel_to_entry_by_path(dir)
                .map_err(Error::git)?
                .filter(|e| e.mode().is_tree())
                .ok_or_else(|| Error::Git(format!("{rev} has no folder {dir}")))?;
            tree = entry.object().map_err(Error::git)?.into_tree();
        }
        let mut out = Vec::new();
        for entry in tree.iter() {
            let entry = entry.map_err(Error::git)?;
            let name = entry.filename().to_string();
            let kind = match entry.kind() {
                EntryKind::Tree => TreeKind::Folder,
                EntryKind::Blob => TreeKind::File,
                EntryKind::BlobExecutable => TreeKind::Executable,
                EntryKind::Link => TreeKind::Link,
                EntryKind::Commit => TreeKind::Submodule,
            };
            let size = match kind {
                TreeKind::File | TreeKind::Executable => repo.find_header(entry.oid()).map_or(0, |h| h.size()),
                _ => 0,
            };
            let path = if dir.is_empty() {
                name.clone()
            } else {
                format!("{dir}/{name}")
            };
            out.push(TreeItem { name, path, kind, size });
        }
        out.sort_by(|a, b| {
            (a.kind != TreeKind::Folder)
                .cmp(&(b.kind != TreeKind::Folder))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(out)
    }

    /// Every file of `rev`'s tree, sorted by path, for finding one by name.
    pub fn files_at(&self, rev: &str) -> Result<Vec<String>> {
        let repo = self.local();
        let tree = tree_of(&repo, rev)?;
        paths_of(&tree)
    }

    /// The file `path` as `rev` has it: its text when it is text and not too big to show.
    pub fn file_at(&self, rev: &str, path: &str) -> Result<FileAt> {
        let repo = self.local();
        let (id, kind) = blob_of(&repo, rev, path)?;
        let size = repo.find_header(id).map_err(Error::git)?.size();
        if size > MAX_TEXT {
            return Ok(FileAt {
                path: path.to_owned(),
                id: id.to_string(),
                size,
                text: None,
                binary: false,
                lfs: false,
            });
        }
        let data = repo.find_object(id).map_err(Error::git)?.detach().data;
        // A symbolic link's text is the path it points to.
        let binary = kind != EntryKind::Link && data.iter().take(8000).any(|b| *b == 0);
        Ok(FileAt {
            path: path.to_owned(),
            id: id.to_string(),
            size,
            lfs: data.starts_with(LFS_POINTER),
            text: (!binary).then(|| String::from_utf8_lossy(&data).into_owned()),
            binary,
        })
    }

    /// Save As: the file `path` as `rev` has it, written to `to`, byte for byte.
    pub fn save_file_at(&self, rev: &str, path: &str, to: &Path) -> Result<()> {
        let repo = self.local();
        let (id, _) = blob_of(&repo, rev, path)?;
        let data = repo.find_object(id).map_err(Error::git)?.detach().data;
        std::fs::write(to, data).map_err(|err| Error::Git(format!("Couldn’t write {}: {err}", to.display())))
    }
}

/// The tree of `rev`: a commit, a branch, a tag or a tree.
fn tree_of<'r>(repo: &'r gix::Repository, rev: &str) -> Result<gix::Tree<'r>> {
    repo.rev_parse_single(rev)
        .map_err(|_| Error::UnknownCommit(rev.to_owned()))?
        .object()
        .map_err(Error::git)?
        .peel_to_tree()
        .map_err(Error::git)
}

/// The blob of a file of `rev`, and whether it is a link or a file.
fn blob_of(repo: &gix::Repository, rev: &str, path: &str) -> Result<(gix::ObjectId, EntryKind)> {
    let mut tree = tree_of(repo, rev)?;
    let entry = tree
        .peel_to_entry_by_path(path)
        .map_err(Error::git)?
        .filter(|e| e.mode().is_blob() || e.mode().is_link())
        .ok_or_else(|| Error::Git(format!("{rev} has no file {path}")))?;
    Ok((entry.object_id(), entry.mode().kind()))
}

/// Every file and link of `tree`, sorted by path.
pub(crate) fn paths_of(tree: &gix::Tree<'_>) -> Result<Vec<String>> {
    let entries = tree.traverse().breadthfirst.files().map_err(Error::git)?;
    let mut paths: Vec<String> = entries
        .into_iter()
        .filter(|e| e.mode.is_blob() || e.mode.is_link())
        .map(|e| e.filepath.to_string())
        .collect();
    paths.sort();
    Ok(paths)
}
