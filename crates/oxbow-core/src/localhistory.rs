//! Local History: the versions of files saved in the working copy between commits, kept for a
//! few days, so a file can be brought back as it was at any save, whatever editor saved it.
//!
//! The watch (see `watch.rs`) tells which files changed; each one that isn't ignored, isn't in a
//! submodule and is at most [`MAX_FILE`] is read once it has been quiet a moment. A version the
//! index already stages (a checkout, a reset, an edit taken back) isn't kept: git has it. One git
//! has elsewhere (a stash brought back, an older commit) costs a line of the list only. The rest
//! is kept as a blob, compressed as git keeps loose objects, in `oxbow/local-history/objects` of
//! the worktree's Git folder: outside git's own objects, so `git gc` neither counts nor keeps
//! them. Saves of a file within [`GROUP`] of the first make one version, the last: an editor
//! saving every second keeps two versions a minute.
//!
//! The versions are listed one per line in `oxbow/local-history/<day>.jsonl`, a file a day,
//! appended as they come. When a window opens the repository and every hour after, versions older
//! than the days in Settings go (7 by default), then the oldest ones until the kept blobs take at
//! most [`MAX_BYTES`]; blobs no version names are deleted.
//!
//! Measured on a clone of VS Code (21,000 files, 261 MB): a day's work on 20 files keeps a few
//! MB, a formatter run over every file 90 MB once, a branch switch nothing (docs/speed.md).

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gix::ObjectId;
use gix::bstr::ByteSlice;
use gix::objs::Write as _;
use serde::{Deserialize, Serialize};

use crate::FileDiff;
use crate::cli::{GitCommand, literal};
use crate::commit::DiffContext;
use crate::error::{Error, Result};
use crate::repo::Repo;

/// Bigger files aren't kept: mostly built or downloaded, and costly to keep many versions of.
pub const MAX_FILE: u64 = 1024 * 1024;
/// Most the kept blobs of one worktree take; the oldest versions go first.
pub const MAX_BYTES: u64 = 256 * 1024 * 1024;
/// Saves of a file this soon after the first of them make one version, in milliseconds.
pub const GROUP: u64 = 30_000;
/// Days kept when Settings doesn't say.
pub const DEFAULT_DAYS: u32 = 7;
/// Versions of one file listed, the newest.
pub const MAX_LISTED: usize = 300;
/// Versions of every file listed, the newest.
pub const MAX_LISTED_ALL: usize = 2000;
/// Files one change of the watch may name; past that the rest of it isn't kept.
pub const MAX_TOLD: usize = 50_000;

const DAY: u64 = 24 * 60 * 60 * 1000;
/// How often old versions go while a window has the repository open.
const PRUNE_EVERY: Duration = Duration::from_secs(60 * 60);
/// Bytes of new blobs after which old versions go without waiting the hour.
const PRUNE_AFTER: u64 = 32 * 1024 * 1024;

static KEEP_DAYS: AtomicU32 = AtomicU32::new(DEFAULT_DAYS);

/// How many days of versions are kept, from Settings › General; 0 keeps no new ones, and those
/// already kept go after [`DEFAULT_DAYS`].
pub fn set_keep_days(days: u32) {
    KEEP_DAYS.store(days, Ordering::Relaxed);
}

fn keep_days() -> u32 {
    KEEP_DAYS.load(Ordering::Relaxed)
}

/// A line of the list: a version of `path`, saved last at `at`; `since` is when the first save
/// of its group was, both in milliseconds since 1970.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    path: String,
    id: String,
    at: u64,
    since: u64,
    size: u64,
}

/// A version of a file Local History kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalVersion {
    pub path: String,
    /// The blob of the file's bytes as they were saved.
    pub id: String,
    /// When it was saved, in milliseconds since 1970.
    pub at: u64,
    pub size: u64,
    /// What it changed from: the version saved before it, or for the oldest the file as `HEAD`
    /// has it; none when the file wasn't there. Only in one file's list.
    pub base: Option<String>,
    /// The base is the file as `HEAD` has it, not an older version.
    pub from_head: bool,
    /// Lines added and removed since the base; none for a binary file, and in the list of every
    /// file.
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
    pub binary: bool,
    /// Its bytes can still be read; git may have dropped a blob it had and Local History didn't.
    pub kept: bool,
    /// The file isn't on disk now.
    pub gone: bool,
}

/// The versions kept for one worktree, and the writing of new ones.
pub struct LocalHistory {
    dir: PathBuf,
    objects: gix::odb::loose::Store,
    /// The newest version of each file: a save soon after it joins its group.
    last: HashMap<String, Entry>,
    /// Bytes of blobs written since old versions last went.
    written: u64,
}

/// Where a worktree's Local History is.
fn folder(repo: &Repo) -> PathBuf {
    repo.local().git_dir().join("oxbow").join("local-history")
}

fn object_store(repo: &Repo, dir: &Path) -> gix::odb::loose::Store {
    gix::odb::loose::Store::at(dir.join("objects"), repo.local().object_hash())
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Files an editor writes beside the one being edited, for itself: swap and backup files, a
/// JetBrains IDE's safe-write copies, Emacs's locks and auto-saves, Finder's folder settings.
fn scratch_name(name: &str) -> bool {
    name == ".DS_Store"
        || name.ends_with('~')
        || name.contains("___jb_")
        || name.starts_with(".#")
        || (name.starts_with('#') && name.ends_with('#'))
        || (name.starts_with('.') && [".swp", ".swo", ".swx"].iter().any(|end| name.ends_with(end)))
}

impl LocalHistory {
    pub fn open(repo: &Repo) -> Self {
        let dir = folder(repo);
        LocalHistory {
            objects: object_store(repo, &dir),
            dir,
            last: HashMap::new(),
            written: 0,
        }
    }

    /// Keep the files at `paths` (in the working copy, with `/` between folders) as they are on
    /// disk now, `now` in milliseconds since 1970. How many versions it noted.
    pub fn keep(&mut self, repo: &Repo, paths: &[String], now: u64) -> usize {
        if keep_days() == 0 || paths.is_empty() {
            return 0;
        }
        let local = repo.local();
        let hash = local.object_hash();
        let index = local.index_or_empty().ok();
        // Files in a submodule's folder are another repository's.
        let gitlinks: Vec<String> = index
            .iter()
            .flat_map(|index| {
                index
                    .entries()
                    .iter()
                    .filter(|e| e.mode == gix::index::entry::Mode::COMMIT)
                    .map(|e| format!("{}/", e.path(index)))
            })
            .collect();
        let mut seen = HashSet::new();
        let mut lines = String::new();
        let mut kept = 0;
        for path in paths {
            if !seen.insert(path.as_str())
                || gitlinks.iter().any(|link| path.starts_with(link.as_str()))
                || path.rsplit('/').next().is_some_and(scratch_name)
            {
                continue;
            }
            let full = repo.workdir().join(path);
            let Ok(meta) = std::fs::symlink_metadata(&full) else {
                continue;
            };
            if !meta.is_file() || meta.len() > MAX_FILE {
                continue;
            }
            let Ok(data) = std::fs::read(&full) else {
                continue;
            };
            if data.len() as u64 > MAX_FILE {
                continue;
            }
            let Ok(id) = gix::objs::compute_hash(hash, gix::object::Kind::Blob, &data) else {
                continue;
            };
            let staged = index
                .as_ref()
                .and_then(|index| index.entry_by_path(path.as_bytes().as_bstr()))
                .is_some_and(|entry| entry.id == id);
            if staged {
                // The file is back as git has it: a save after this is a version of its own.
                self.last.remove(path);
                continue;
            }
            let last = self.last.get(path);
            if last.is_some_and(|last| id.to_hex() == last.id) {
                continue;
            }
            let since = match last {
                Some(last) if now.saturating_sub(last.since) < GROUP => last.since,
                _ => now,
            };
            if !local.has_object(id) && !self.objects.contains(&id) {
                let _ = std::fs::create_dir_all(self.objects.path());
                if self.objects.write_buf(gix::object::Kind::Blob, &data).is_err() {
                    continue;
                }
                self.written += std::fs::metadata(self.objects.object_path(&id)).map_or(0, |m| m.len());
            }
            let entry = Entry {
                path: path.clone(),
                id: id.to_string(),
                at: now,
                since,
                size: data.len() as u64,
            };
            if let Ok(line) = serde_json::to_string(&entry) {
                lines.push_str(&line);
                lines.push('\n');
                self.last.insert(path.clone(), entry);
                kept += 1;
            }
        }
        if !lines.is_empty() {
            let _ = std::fs::create_dir_all(&self.dir);
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.dir.join(format!("{}.jsonl", now / DAY)));
            if let Ok(mut file) = file {
                let _ = file.write_all(lines.as_bytes());
            }
        }
        if self.written > PRUNE_AFTER {
            let _ = self.prune(now);
        }
        kept
    }

    /// Let versions older than the days kept go, then the oldest until the blobs take at most
    /// [`MAX_BYTES`], and delete the blobs no version names. The bytes the kept blobs take.
    pub fn prune(&mut self, now: u64) -> Result<u64> {
        self.prune_within(now, MAX_BYTES)
    }

    /// [`LocalHistory::prune`] down to `max` bytes.
    pub fn prune_within(&mut self, now: u64, max: u64) -> Result<u64> {
        self.written = 0;
        let cutoff = now.saturating_sub(days_kept() * DAY);
        let mut entries = Vec::new();
        let mut days = Vec::new();
        for (day, file) in day_files(&self.dir) {
            if (day + 1) * DAY <= cutoff {
                std::fs::remove_file(&file).map_err(Error::git)?;
                continue;
            }
            let read = read_day(&file);
            days.push((day, file, read.len()));
            entries.extend(read);
        }
        let mut entries: Vec<Entry> = groups(entries).into_iter().filter(|e| e.at >= cutoff).collect();
        // The oldest versions go while the blobs they alone name take too much.
        let mut names: HashMap<&str, usize> = HashMap::new();
        for entry in &entries {
            *names.entry(entry.id.as_str()).or_default() += 1;
        }
        let size = |id: &str| {
            ObjectId::from_hex(id.as_bytes())
                .ok()
                .and_then(|id| std::fs::metadata(self.objects.object_path(&id)).ok())
                .map_or(0, |m| m.len())
        };
        let mut total: u64 = names.keys().map(|id| size(id)).sum();
        let mut drop = 0;
        let mut oldest: Vec<usize> = (0..entries.len()).collect();
        oldest.sort_by_key(|&at| entries[at].at);
        let mut dropped = HashSet::new();
        while total > max && drop < oldest.len() {
            let entry = &entries[oldest[drop]];
            dropped.insert(oldest[drop]);
            let left = names.get_mut(entry.id.as_str()).expect("named");
            *left -= 1;
            if *left == 0 {
                total -= size(&entry.id);
            }
            drop += 1;
        }
        let mut at = 0;
        entries.retain(|_| {
            at += 1;
            !dropped.contains(&(at - 1))
        });
        // Each day's list again, when it lost a line.
        for (day, file, count) in days {
            let left: Vec<&Entry> = entries.iter().filter(|e| e.at / DAY == day).collect();
            if left.len() == count {
                continue;
            }
            if left.is_empty() {
                std::fs::remove_file(&file).map_err(Error::git)?;
                continue;
            }
            let mut text = String::new();
            for entry in left {
                text.push_str(&serde_json::to_string(entry).map_err(Error::git)?);
                text.push('\n');
            }
            let fresh = file.with_extension("jsonl.new");
            std::fs::write(&fresh, text).map_err(Error::git)?;
            std::fs::rename(&fresh, &file).map_err(Error::git)?;
        }
        let named: HashSet<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        for id in self.objects.iter().filter_map(|id| id.ok()) {
            if !named.contains(id.to_string().as_str()) {
                let _ = std::fs::remove_file(self.objects.object_path(&id));
            }
        }
        self.last = entries.into_iter().map(|e| (e.path.clone(), e)).collect();
        Ok(total)
    }
}

fn days_kept() -> u64 {
    match keep_days() {
        0 => DEFAULT_DAYS as u64,
        days => days as u64,
    }
}

/// The day lists, oldest first: the day's number since 1970 and the file.
fn day_files(dir: &Path) -> Vec<(u64, PathBuf)> {
    let mut out: Vec<(u64, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            let day = path.file_name()?.to_str()?.strip_suffix(".jsonl")?.parse().ok()?;
            Some((day, path))
        })
        .collect();
    out.sort();
    out
}

/// A day's lines; one that can't be read, as from a write cut short, is left out.
fn read_day(file: &Path) -> Vec<Entry> {
    std::fs::read_to_string(file)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// The versions in order, each group of saves as its last one.
fn groups(entries: Vec<Entry>) -> Vec<Entry> {
    let mut last: HashMap<(String, u64), usize> = HashMap::new();
    let mut out: Vec<Option<Entry>> = Vec::with_capacity(entries.len());
    for entry in entries {
        let key = (entry.path.clone(), entry.since);
        if let Some(&at) = last.get(&key) {
            out[at] = None;
        }
        last.insert(key, out.len());
        out.push(Some(entry));
    }
    out.into_iter().flatten().collect()
}

/// A tree with only the file `path`, as blob `id`; empty without one.
fn one_file_tree(repo: &gix::Repository, path: &str, id: Option<ObjectId>, executable: bool) -> Result<String> {
    let mut editor = repo
        .edit_tree(ObjectId::empty_tree(repo.object_hash()))
        .map_err(Error::git)?;
    if let Some(id) = id {
        let kind = if executable {
            gix::object::tree::EntryKind::BlobExecutable
        } else {
            gix::object::tree::EntryKind::Blob
        };
        editor.upsert(path, kind, id).map_err(Error::git)?;
    }
    Ok(editor.write().map_err(Error::git)?.to_string())
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_: &Path) -> bool {
    false
}

impl Repo {
    /// The versions kept, oldest first, each group of saves as its last one: those of `path`, or
    /// of every file.
    fn local_entries(&self, path: Option<&str>) -> Vec<Entry> {
        let cutoff = now_ms().saturating_sub(days_kept() * DAY);
        let entries = day_files(&folder(self))
            .into_iter()
            .filter(|(day, _)| (day + 1) * DAY > cutoff)
            .flat_map(|(_, file)| read_day(&file))
            .filter(|e| path.is_none_or(|p| p == e.path))
            .collect();
        groups(entries).into_iter().filter(|e| e.at >= cutoff).collect()
    }

    /// The bytes of a version: from Local History's blobs, or git's.
    fn local_bytes(&self, local: &gix::Repository, objects: &gix::odb::loose::Store, id: ObjectId) -> Option<Vec<u8>> {
        let mut buf = Vec::new();
        if let Ok(Some(data)) = objects.try_find(&id, &mut buf) {
            return Some(data.data.to_vec());
        }
        local.find_object(id).ok().map(|object| object.detach().data)
    }

    /// The versions of `path` Local History kept, the newest first, with what each changed.
    pub fn local_history(&self, path: &str) -> Result<Vec<LocalVersion>> {
        let local = self.local();
        let objects = object_store(self, &folder(self));
        let entries = self.local_entries(Some(path));
        let head = local
            .head_tree()
            .ok()
            .and_then(|tree| tree.lookup_entry_by_path(path).ok().flatten())
            .filter(|entry| entry.mode().is_blob())
            .map(|entry| entry.object_id());
        let gone = !self.workdir().join(path).exists();
        let skip = entries.len().saturating_sub(MAX_LISTED);
        let mut before = match skip {
            0 => head,
            n => ObjectId::from_hex(entries[n - 1].id.as_bytes()).ok(),
        };
        let mut before_bytes = before.and_then(|id| self.local_bytes(&local, &objects, id));
        let mut out = Vec::new();
        for (at, entry) in entries.into_iter().enumerate().skip(skip) {
            let id = ObjectId::from_hex(entry.id.as_bytes()).map_err(Error::git)?;
            let bytes = self.local_bytes(&local, &objects, id);
            let binary = bytes.as_deref().is_some_and(is_binary) || before_bytes.as_deref().is_some_and(is_binary);
            let counts = match (&bytes, binary) {
                (Some(new), false) => {
                    let old = before_bytes.as_deref().map(String::from_utf8_lossy).unwrap_or_default();
                    Some(line_counts(&old, &String::from_utf8_lossy(new)))
                }
                _ => None,
            };
            out.push(LocalVersion {
                path: entry.path,
                id: entry.id,
                at: entry.at,
                size: entry.size,
                base: before.map(|id| id.to_string()),
                from_head: at == 0 && before.is_some(),
                additions: counts.map(|c| c.0),
                deletions: counts.map(|c| c.1),
                binary,
                kept: bytes.is_some(),
                gone,
            });
            before = Some(id);
            before_bytes = bytes;
        }
        out.reverse();
        Ok(out)
    }

    /// The versions of every file Local History kept, the newest first: when, how big, what each
    /// changed from, and whether the file is still on disk; no line counts.
    pub fn local_history_all(&self) -> Result<Vec<LocalVersion>> {
        let entries = self.local_entries(None);
        let mut before: HashMap<String, String> = HashMap::new();
        let bases: Vec<Option<String>> = entries
            .iter()
            .map(|e| before.insert(e.path.clone(), e.id.clone()))
            .collect();
        let local = self.local();
        let objects = object_store(self, &folder(self));
        let head = local.head_tree().ok();
        let mut gone: HashMap<String, bool> = HashMap::new();
        Ok(entries
            .into_iter()
            .zip(bases)
            .rev()
            .take(MAX_LISTED_ALL)
            .map(|(entry, base)| {
                let gone = *gone
                    .entry(entry.path.clone())
                    .or_insert_with(|| !self.workdir().join(&entry.path).exists());
                // The oldest version of a file changed it from the last commit's.
                let (base, from_head) = match base {
                    Some(base) => (Some(base), false),
                    None => {
                        let at_head = head
                            .as_ref()
                            .and_then(|tree| tree.lookup_entry_by_path(&entry.path).ok().flatten())
                            .filter(|e| e.mode().is_blob())
                            .map(|e| e.object_id().to_string());
                        let from_head = at_head.is_some();
                        (at_head, from_head)
                    }
                };
                let kept = ObjectId::from_hex(entry.id.as_bytes())
                    .is_ok_and(|id| objects.contains(&id) || local.has_object(id));
                LocalVersion {
                    path: entry.path,
                    id: entry.id,
                    at: entry.at,
                    size: entry.size,
                    base,
                    from_head,
                    additions: None,
                    deletions: None,
                    binary: false,
                    kept,
                    gone,
                }
            })
            .collect())
    }

    /// Put blob `id` of a version where git finds it, from Local History's blobs when git hasn't
    /// it: a diff and a restore read it from git.
    fn local_into_git(&self, local: &gix::Repository, id: &str) -> Result<ObjectId> {
        let id = ObjectId::from_hex(id.as_bytes()).map_err(|_| Error::Git(format!("no version {id}")))?;
        if local.has_object(id) {
            return Ok(id);
        }
        let mut buf = Vec::new();
        let objects = object_store(self, &folder(self));
        let data = match objects.try_find(&id, &mut buf) {
            Ok(Some(data)) => data.data,
            _ => {
                return Err(Error::Git(
                    "Local History no longer has this version: it was older than the days kept, or the space for it ran out"
                        .into(),
                ));
            }
        };
        local.write_blob(data).map_err(Error::git)?;
        Ok(id)
    }

    /// What version `id` of `path` changed since `base` (none: the file wasn't there), or with
    /// `against_disk`, how the file on disk now differs from it.
    pub fn local_version_diff_with(
        &self,
        path: &str,
        id: &str,
        base: Option<&str>,
        against_disk: bool,
        context: DiffContext,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<Vec<FileDiff>> {
        let local = self.local();
        let full = self.workdir().join(path);
        let executable = is_executable(&full);
        let version = Some(self.local_into_git(&local, id)?);
        let (old, new) = if against_disk {
            let disk = match std::fs::read(&full) {
                Ok(data) => Some(local.write_blob(data).map_err(Error::git)?.detach()),
                Err(_) => None,
            };
            (version, disk)
        } else {
            let base = base.map(|base| self.local_into_git(&local, base)).transpose()?;
            (base, version)
        };
        let old = one_file_tree(&local, path, old, executable)?;
        let new = one_file_tree(&local, path, new, executable)?;
        self.tree_diff_with(&old, &new, Some(path), context, cancel)
    }

    /// Save As: version `id` of a file written where the user chose.
    pub fn save_local_version(&self, id: &str, to: &Path) -> Result<()> {
        let local = self.local();
        let id = self.local_into_git(&local, id)?;
        let data = local.find_object(id).map_err(Error::git)?.detach().data;
        std::fs::write(to, data).map_err(|err| Error::Git(format!("Couldn’t write {}: {err}", to.display())))
    }

    /// Restore a version from Local History: `git restore` from a tree that has only the file, as
    /// it was saved, so only the file on disk changes, and Undo brings back what it replaced.
    pub(crate) fn plan_restore_local(&self, path: &str, id: &str) -> Result<Vec<GitCommand>> {
        let local = self.local();
        let id = self.local_into_git(&local, id)?;
        let tree = one_file_tree(&local, path, Some(id), is_executable(&self.workdir().join(path)))?;
        Ok(vec![
            GitCommand::new([
                "restore".to_owned(),
                format!("--source={tree}"),
                "--worktree".to_owned(),
                "--".to_owned(),
                literal(path),
            ])
            .comment(format!(
                "--source: a tree with only {path}, as Local History kept it; only the file on disk changes, nothing is staged"
            )),
        ])
    }
}

fn is_binary(data: &[u8]) -> bool {
    data[..data.len().min(8000)].contains(&0)
}

/// Lines added and removed from `old` to `new`.
fn line_counts(old: &str, new: &str) -> (u32, u32) {
    match crate::commit::diff_text_within(old, new, DiffContext::Lines(0), false, false, 0) {
        Ok(hunks) => {
            let mut counts = (0, 0);
            for line in hunks.iter().flat_map(|h| &h.lines) {
                match line.kind {
                    crate::commit::LineKind::Added => counts.0 += 1,
                    crate::commit::LineKind::Removed => counts.1 += 1,
                    crate::commit::LineKind::Context => {}
                }
            }
            counts
        }
        Err(over) => (over.additions, over.deletions),
    }
}

/// Keeps the versions the watches of one worktree tell, on a thread of its own, until the last
/// of those watches is dropped: two windows of one repository keep one list.
pub(crate) struct Keeper {
    tx: mpsc::Sender<Vec<String>>,
}

static KEEPERS: Mutex<Vec<(PathBuf, Weak<Keeper>)>> = Mutex::new(Vec::new());

impl Keeper {
    pub(crate) fn tell(&self, paths: Vec<String>) {
        let _ = self.tx.send(paths);
    }
}

/// The keeper of `repo`'s worktree, started when it has none.
pub(crate) fn keeper(repo: &Repo) -> Option<Arc<Keeper>> {
    // The Git folder, not Local History's: that one may not be there yet, and a later window
    // would name it otherwise.
    let git_dir = repo.local().git_dir().to_path_buf();
    let key = git_dir.canonicalize().unwrap_or(git_dir);
    let mut keepers = KEEPERS.lock().expect("keepers lock");
    keepers.retain(|(_, keeper)| keeper.strong_count() > 0);
    if let Some(keeper) = keepers.iter().find(|(at, _)| *at == key).and_then(|(_, k)| k.upgrade()) {
        return Some(keeper);
    }
    let (tx, rx) = mpsc::channel::<Vec<String>>();
    let repo = repo.clone();
    std::thread::Builder::new()
        .name("oxbow-local-history".into())
        .spawn(move || {
            let mut history = LocalHistory::open(&repo);
            let _ = history.prune(now_ms());
            let mut pruned = Instant::now();
            loop {
                match rx.recv_timeout(PRUNE_EVERY) {
                    Ok(paths) => {
                        history.keep(&repo, &paths, now_ms());
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
                if pruned.elapsed() >= PRUNE_EVERY {
                    let _ = history.prune(now_ms());
                    pruned = Instant::now();
                }
            }
        })
        .ok()?;
    let keeper = Arc::new(Keeper { tx });
    keepers.push((key, Arc::downgrade(&keeper)));
    Some(keeper)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, id: &str, at: u64, since: u64) -> Entry {
        Entry {
            path: path.into(),
            id: id.into(),
            at,
            since,
            size: 1,
        }
    }

    #[test]
    fn a_group_of_saves_is_its_last_one() {
        let out = groups(vec![
            entry("a", "1", 10, 10),
            entry("b", "2", 11, 11),
            entry("a", "3", 20, 10),
            entry("a", "4", 50, 50),
        ]);
        let ids: Vec<&str> = out.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["2", "3", "4"]);
    }

    #[test]
    fn turned_off_it_keeps_nothing_new() {
        let dir = tempfile::tempdir().unwrap();
        let status = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status();
        assert!(status.unwrap().success());
        std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
        let repo = Repo::open(dir.path()).unwrap();
        let mut history = LocalHistory::open(&repo);
        set_keep_days(0);
        let off = history.keep(&repo, &["a.txt".into()], now_ms());
        set_keep_days(DEFAULT_DAYS);
        assert_eq!(off, 0);
        assert_eq!(history.keep(&repo, &["a.txt".into()], now_ms()), 1);
    }

    #[test]
    fn editor_scratch_files_are_left_out() {
        for name in [
            ".a.rs.swp",
            "a.rs~",
            "a.rs___jb_tmp___",
            ".#a.rs",
            "#a.rs#",
            ".DS_Store",
        ] {
            assert!(scratch_name(name), "{name}");
        }
        for name in ["a.rs", "swp", ".swp.rs", "#note"] {
            assert!(!scratch_name(name), "{name}");
        }
    }
}
