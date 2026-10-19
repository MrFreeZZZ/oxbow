//! The Console: a log of every process Oxbow runs, with what it printed, its exit code and how
//! long it took, kept per repository in `.git/oxbow/console.jsonl`.
//!
//! What runs is grouped into entries: an action the user started, a fetch of the background
//! timer, or a read of the repository. A [`scope`] opens an entry for the work on this thread;
//! every process run inside it lands there, the action's own commands as they are and the ones
//! Oxbow runs for it (checks, the secrets check, the snapshot for Undo) under a [`role`]. A
//! process run outside any scope gets an entry of its own, so nothing goes unlogged.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde::{Deserialize, Serialize};

/// How many entries a repository's log keeps.
pub const KEEP: usize = 1000;
/// Of those, how many reads at most: a read runs on every focus of the window and would push
/// the actions out.
const KEEP_READS: usize = 300;
/// The log is rewritten once it has this many more lines than it keeps.
const SLACK: usize = 100;
const FILE: &str = "console.jsonl";

/// Lines and bytes of output kept of an action's own command, and of any other.
const MAIN_OUTPUT: (usize, usize) = (400, 64 * 1024);
const OTHER_OUTPUT: (usize, usize) = (40, 6 * 1024);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    /// Started by the user: an action of a sheet, a Fetch, a clone, a setting.
    Action,
    /// Started by Oxbow on its own, e.g. the fetch of the timer.
    Background,
    /// A read of the repository, e.g. the reload when the window comes to the front.
    Read,
}

/// One entry of the Console.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: u64,
    /// Milliseconds since the epoch.
    pub time: i64,
    pub kind: EntryKind,
    pub title: String,
    pub millis: u64,
    #[serde(default)]
    pub failed: bool,
    /// Stopped with Stop before it finished.
    #[serde(default)]
    pub stopped: bool,
    /// Read with gitoxide inside Oxbow, which starts no git process.
    #[serde(default)]
    pub gitoxide: bool,
    /// What a read found, e.g. "refs 41 · commits 1 240 · status 0 changes".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub commands: Vec<Ran>,
    /// Still running: only in the live updates, never in the log.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub running: bool,
}

/// One process of an entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ran {
    /// As typed in a shell, with passwords and tokens masked.
    pub display: String,
    /// What Oxbow ran it for, e.g. "secrets check", when it is not one of the action's own
    /// commands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub time: i64,
    pub millis: u64,
    /// The exit code; none when it was stopped or did not start.
    #[serde(default)]
    pub code: Option<i32>,
    #[serde(default)]
    pub stopped: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output: Vec<Line>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub running: bool,
}

/// A line a process printed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    #[serde(rename = "t")]
    pub text: String,
    /// Printed to standard error.
    #[serde(rename = "e", default, skip_serializing_if = "std::ops::Not::not")]
    pub stderr: bool,
}

/// What the processes run now are for.
#[derive(Debug, Clone, Copy)]
enum Role {
    /// The action's own commands, the ones its sheet shows.
    Main,
    /// Run by Oxbow for the action, e.g. "check".
    Helper(&'static str),
}

/// An entry being filled on this thread.
struct Open {
    /// The repository's working tree; none outside any repository.
    repo: Option<PathBuf>,
    /// The log it goes to, see [`log_of`].
    log: Option<PathBuf>,
    entry: Entry,
    started: Instant,
}

thread_local! {
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
    static ROLE: Cell<Option<Role>> = const { Cell::new(None) };
    static UNKEPT: Cell<bool> = const { Cell::new(false) };
}

type Listener = Box<dyn Fn(Option<&Path>, &Entry) + Send + Sync>;

static LISTENER: RwLock<Option<Listener>> = RwLock::new(None);
/// Where the log of what runs outside any repository goes, e.g. `git config --global`.
static OUTSIDE: RwLock<Option<PathBuf>> = RwLock::new(None);
/// An entry still running, with the log it goes to.
type Running = (Option<PathBuf>, Entry);
/// The entries still running: a window that opens meanwhile shows them too.
static RUNNING: LazyLock<Mutex<HashMap<u64, Running>>> = LazyLock::new(Default::default);
/// Lines in each log file, once counted; also what keeps two threads from writing one at once.
static LINES: LazyLock<Mutex<HashMap<PathBuf, usize>>> = LazyLock::new(Default::default);
static NEXT_ID: LazyLock<AtomicU64> = LazyLock::new(|| AtomicU64::new(now_ms().max(0) as u64 * 1000));

/// Tell `listener` of every entry as it is filled and once it is done, with the working tree of
/// its repository: the Console window follows them.
pub fn set_listener(listener: impl Fn(Option<&Path>, &Entry) + Send + Sync + 'static) {
    *LISTENER.write().expect("console listener lock") = Some(Box::new(listener));
}

/// The folder for the log of what runs outside any repository: the app's settings folder.
pub fn set_outside_dir(dir: impl Into<PathBuf>) {
    *OUTSIDE.write().expect("console folder lock") = Some(dir.into());
}

/// Run `f` as one entry of the Console of `repo` (none: outside any repository). Inside another
/// scope it is part of that one. The entry fails when `f` does; one without a process and
/// without a read of its own is left out.
pub fn scope<T, E>(
    repo: Option<&Path>,
    kind: EntryKind,
    title: impl Into<String>,
    f: impl FnOnce() -> std::result::Result<T, E>,
) -> std::result::Result<T, E> {
    if OPEN.with(|open| open.borrow().is_some()) {
        return f();
    }
    OPEN.with(|open| {
        *open.borrow_mut() = Some(Open {
            repo: repo.map(Path::to_path_buf),
            log: log_of(repo),
            entry: Entry {
                id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
                time: now_ms(),
                kind,
                title: title.into(),
                millis: 0,
                failed: false,
                stopped: false,
                gitoxide: false,
                note: None,
                commands: Vec::new(),
                running: true,
            },
            started: Instant::now(),
        })
    });
    // A panic in `f` must not leave the entry open for whatever runs on this thread next.
    let reset = Reset(ROLE.with(|role| role.replace(None)));
    let result = f();
    let open = OPEN.with(|open| open.borrow_mut().take());
    drop(reset);
    let Some(Open {
        repo,
        mut entry,
        started,
        ..
    }) = open
    else {
        return result;
    };
    running_done(entry.id);
    entry.millis = elapsed_ms(started);
    entry.running = false;
    entry.stopped = entry.commands.iter().any(|c| c.stopped);
    entry.failed = result.is_err() && !entry.stopped;
    if !entry.commands.is_empty() || entry.gitoxide {
        finish(repo.as_deref(), &entry);
    }
    result
}

/// Run `f` as the action's own commands: what its sheet shows, with their output in full.
pub fn main<T>(f: impl FnOnce() -> T) -> T {
    with(Role::Main, f)
}

/// Run `f` as commands Oxbow runs for the action, shown under it as `label`, e.g. "secrets
/// check". Inside [`main`] or another role it keeps that one.
pub fn role<T>(label: &'static str, f: impl FnOnce() -> T) -> T {
    if ROLE.with(Cell::get).is_some() {
        return f();
    }
    with(Role::Helper(label), f)
}

/// Run `f` keeping only how many lines its processes print, not the lines: what they print may
/// hold secrets, e.g. the staged lines the secrets check reads.
pub(crate) fn without_output<T>(f: impl FnOnce() -> T) -> T {
    struct Back(bool);
    impl Drop for Back {
        fn drop(&mut self) {
            UNKEPT.with(|unkept| unkept.set(self.0));
        }
    }
    let _back = Back(UNKEPT.with(|unkept| unkept.replace(true)));
    f()
}

fn with<T>(role: Role, f: impl FnOnce() -> T) -> T {
    let _reset = Reset(ROLE.with(|r| r.replace(Some(role))));
    f()
}

/// Puts the role back, and closes an entry left open, when dropped, also by a panic.
struct Reset(Option<Role>);

impl Drop for Reset {
    fn drop(&mut self) {
        ROLE.with(|role| role.set(self.0));
        if std::thread::panicking()
            && let Some(open) = OPEN.with(|open| open.borrow_mut().take())
        {
            running_done(open.entry.id);
        }
    }
}

/// Note what the open entry read with gitoxide, e.g. "refs 41 · commits 1 240": it is then kept
/// even without any process.
pub fn read_note(note: impl Into<String>) {
    OPEN.with(|open| {
        if let Some(open) = open.borrow_mut().as_mut() {
            open.entry.gitoxide = true;
            open.entry.note = Some(note.into());
        }
    });
}

/// A process that started, for [`Started::done`] to log once it is over.
pub(crate) struct Started {
    display: String,
    dir: PathBuf,
    time: i64,
    at: Instant,
    role: Option<Role>,
    unkept: bool,
}

/// Note that `display` starts in `dir`.
pub(crate) fn started(dir: &Path, display: &str) -> Started {
    let started = Started {
        display: mask(display),
        dir: dir.to_path_buf(),
        time: now_ms(),
        at: Instant::now(),
        role: ROLE.with(Cell::get),
        unkept: UNKEPT.with(Cell::get),
    };
    // Actions and fetches show what runs now; a read only once it is done.
    let live = OPEN.with(|open| {
        let mut open = open.borrow_mut();
        let open = open.as_mut().filter(|o| o.entry.kind != EntryKind::Read)?;
        let mut ran = started.ran(None, false, started.output());
        ran.running = true;
        open.entry.commands.push(ran);
        Some((open.repo.clone(), open.log.clone(), open.entry.clone()))
    });
    if let Some((repo, log, entry)) = live {
        tell_running(repo.as_deref(), log, &entry);
    }
    started
}

impl Started {
    /// It exited with `code` (none: killed by a signal), printing `stdout` and `stderr`.
    pub(crate) fn done(self, code: Option<i32>, stdout: &[u8], stderr: &[u8]) {
        let mut output = self.output();
        for line in pieces(&String::from_utf8_lossy(stdout)) {
            output.push(line, false);
        }
        for line in pieces(&String::from_utf8_lossy(stderr)) {
            output.push(line, true);
        }
        self.log(code, false, output);
    }

    /// It exited with `code`, printing `output` as it came.
    pub(crate) fn done_lines(self, code: Option<i32>, output: Output) {
        self.log(code, false, output);
    }

    /// Stop stopped it.
    pub(crate) fn stopped(self, output: Output) {
        self.log(None, true, output);
    }

    /// Where to collect what it prints, line by line.
    pub(crate) fn output(&self) -> Output {
        Output::new(self.limit().0, self.unkept)
    }

    fn limit(&self) -> (usize, usize) {
        match self.role {
            Some(Role::Main) => MAIN_OUTPUT,
            _ => OTHER_OUTPUT,
        }
    }

    /// It could not start, e.g. git is not installed.
    pub(crate) fn not_started(self, error: &str) {
        let mut output = self.output();
        output.push(&format!("could not start: {error}"), true);
        self.log(None, false, output);
    }

    fn ran(&self, code: Option<i32>, stopped: bool, output: Output) -> Ran {
        let role = match self.role {
            Some(Role::Helper(label)) => Some(label.to_owned()),
            _ => None,
        };
        let (lines, skipped) = output.lines();
        Ran {
            display: self.display.clone(),
            role,
            time: self.time,
            millis: elapsed_ms(self.at),
            code,
            stopped,
            output: keep(lines, self.limit(), skipped),
            running: false,
        }
    }

    fn log(self, code: Option<i32>, stopped: bool, output: Output) {
        let unscoped = OPEN.with(|open| open.borrow().is_none());
        let mut ran = self.ran(code, stopped, output);
        if unscoped {
            ran.role = None;
            let entry = Entry {
                id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
                time: self.time,
                kind: EntryKind::Read,
                title: "Read by Oxbow".into(),
                millis: ran.millis,
                failed: false,
                stopped,
                gitoxide: false,
                note: None,
                commands: vec![ran],
                running: false,
            };
            finish(Some(&self.dir), &entry);
            return;
        }
        let live = OPEN.with(|open| {
            let mut open = open.borrow_mut();
            let open = open.as_mut()?;
            // In an action, a process without a role is a check Oxbow makes for it.
            if open.entry.kind != EntryKind::Read && self.role.is_none() {
                ran.role = Some("check".into());
            }
            match open.entry.commands.last_mut() {
                Some(last) if last.running => *last = ran,
                _ => open.entry.commands.push(ran),
            }
            (open.entry.kind != EntryKind::Read).then(|| (open.repo.clone(), open.log.clone(), open.entry.clone()))
        });
        if let Some((repo, log, entry)) = live {
            tell_running(repo.as_deref(), log, &entry);
        }
    }
}

/// Write `entry` to its log and tell the listener.
fn finish(repo: Option<&Path>, entry: &Entry) {
    let file = repo.and_then(log_file);
    let (shown, file) = match file {
        Some(file) => (repo, Some(file)),
        None => (None, outside_file()),
    };
    if let Some(file) = file {
        // The log is a convenience: a folder that can't be written never stops the work.
        let _ = append(&file, entry);
    }
    tell(shown, entry);
}

/// Tell the listener of `entry`, still running, and keep it for [`running`].
fn tell_running(repo: Option<&Path>, log: Option<PathBuf>, entry: &Entry) {
    RUNNING
        .lock()
        .expect("console running lock")
        .insert(entry.id, (log, entry.clone()));
    tell(repo, entry);
}

fn running_done(id: u64) {
    RUNNING.lock().expect("console running lock").remove(&id);
}

/// The entries of the log of `dir` (see [`log_of`]) still running, oldest first: they are in
/// the log only once they are done.
pub fn running(dir: Option<&Path>) -> Vec<Entry> {
    let log = log_of(dir);
    let mut list: Vec<Entry> = RUNNING
        .lock()
        .expect("console running lock")
        .values()
        .filter(|(of, _)| *of == log)
        .map(|(_, entry)| entry.clone())
        .collect();
    list.sort_by_key(|e| e.time);
    list
}

fn tell(repo: Option<&Path>, entry: &Entry) {
    if let Some(listener) = LISTENER.read().expect("console listener lock").as_ref() {
        listener(repo, entry);
    }
}

fn outside_file() -> Option<PathBuf> {
    OUTSIDE
        .read()
        .expect("console folder lock")
        .as_ref()
        .map(|dir| dir.join(FILE))
}

/// The log what runs in `dir` goes to: of its repository, or of what runs outside any. Two
/// worktrees of one repository give the same one, so it tells their logs apart from others.
pub fn log_of(dir: Option<&Path>) -> Option<PathBuf> {
    dir.and_then(log_file).or_else(outside_file)
}

/// The log file of the repository whose working tree (or git folder) is `dir`: in its git
/// folder, shared by all its worktrees, as one path whichever of them it is reached from.
/// None when `dir` is no repository.
pub fn log_file(dir: &Path) -> Option<PathBuf> {
    let dot = dir.join(".git");
    let git_dir = if dot.is_dir() {
        dot
    } else if dot.is_file() {
        // A worktree or a submodule: `gitdir: <path>`.
        let text = std::fs::read_to_string(&dot).ok()?;
        let path = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
        dir.join(path)
    } else if dir.join("HEAD").is_file() && dir.join("objects").is_dir() {
        dir.to_path_buf()
    } else {
        return None;
    };
    let common = match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(path) => git_dir.join(path.trim()),
        Err(_) => git_dir,
    };
    // `.git/worktrees/x/../..` and `/tmp` against `/private/tmp` are one folder.
    let common = std::fs::canonicalize(&common).unwrap_or(common);
    Some(common.join("oxbow").join(FILE))
}

fn append(file: &Path, entry: &Entry) -> std::io::Result<()> {
    let mut line = serde_json::to_string(entry).map_err(std::io::Error::other)?;
    line.push('\n');
    let mut counts = LINES.lock().expect("console lines lock");
    let count = counts
        .entry(file.to_path_buf())
        .or_insert_with(|| std::fs::read_to_string(file).map_or(0, |text| text.lines().count()));
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)?
        .write_all(line.as_bytes())?;
    *count += 1;
    if *count > KEEP + SLACK {
        let kept = trimmed(read_all(file));
        write_all(file, &kept)?;
        *count = kept.len();
    }
    Ok(())
}

fn read_all(file: &Path) -> Vec<Entry> {
    std::fs::read_to_string(file)
        .map(|text| {
            text.lines()
                .filter_map(|line| serde_json::from_str(line).ok())
                .collect()
        })
        .unwrap_or_default()
}

fn write_all(file: &Path, entries: &[Entry]) -> std::io::Result<()> {
    let mut text = String::new();
    for entry in entries {
        text.push_str(&serde_json::to_string(entry).map_err(std::io::Error::other)?);
        text.push('\n');
    }
    // Written aside first: a log cut in half would lose all of it.
    let aside = file.with_extension("jsonl.new");
    std::fs::write(&aside, text)?;
    std::fs::rename(aside, file)
}

/// The newest [`KEEP`] entries, of them at most [`KEEP_READS`] reads, oldest first.
fn trimmed(entries: Vec<Entry>) -> Vec<Entry> {
    let reads = entries.iter().filter(|e| e.kind == EntryKind::Read).count();
    let mut skip_reads = reads.saturating_sub(KEEP_READS);
    let mut kept: Vec<Entry> = entries
        .into_iter()
        .filter(|e| {
            if e.kind == EntryKind::Read && skip_reads > 0 {
                skip_reads -= 1;
                false
            } else {
                true
            }
        })
        .collect();
    if kept.len() > KEEP {
        kept.drain(..kept.len() - KEEP);
    }
    kept
}

/// The log of the repository at `dir` (none: of what ran outside any repository), oldest first.
pub fn entries(dir: Option<&Path>) -> Vec<Entry> {
    let file = match dir {
        Some(dir) => log_file(dir),
        None => outside_file(),
    };
    let Some(file) = file else {
        return Vec::new();
    };
    let _lock = LINES.lock().expect("console lines lock");
    trimmed(read_all(&file))
}

/// Forget the log of the repository at `dir` (none: of what ran outside any repository).
pub fn clear(dir: Option<&Path>) -> std::io::Result<()> {
    let file = match dir {
        Some(dir) => log_file(dir),
        None => outside_file(),
    };
    let Some(file) = file else {
        return Ok(());
    };
    let mut counts = LINES.lock().expect("console lines lock");
    counts.remove(&file);
    match std::fs::remove_file(&file) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

/// What a process printed, as much of it as its log keeps: the first lines masked as they come,
/// the last ones held until it is over, the ones between only counted. A read of a long diff
/// prints thousands of lines, and masking them all would take longer than the read.
pub(crate) struct Output {
    max: usize,
    head: Vec<Line>,
    tail: VecDeque<(String, bool)>,
    skipped: usize,
    /// Only counted, see [`without_output`].
    unkept: bool,
    /// Between the first and the last line of a private key.
    in_key: bool,
}

impl Output {
    fn new(max: usize, unkept: bool) -> Self {
        Output {
            max,
            head: Vec::new(),
            tail: VecDeque::new(),
            skipped: 0,
            unkept,
            in_key: false,
        }
    }

    /// Add a line it printed.
    pub(crate) fn push(&mut self, text: &str, stderr: bool) {
        if self.unkept {
            self.skipped += 1;
            return;
        }
        // The lines of a private key hold no word a mask finds: they go as a whole.
        let key_body;
        let text = if self.in_key {
            if text.contains("-----END") {
                self.in_key = false;
                text
            } else {
                let marker = text.chars().next().filter(|c| matches!(c, '+' | '-' | ' '));
                key_body = format!("{}{MASK}", marker.map(String::from).unwrap_or_default());
                &key_body
            }
        } else {
            if text.contains("-----BEGIN") && text.contains("PRIVATE KEY") {
                self.in_key = true;
            }
            text
        };
        if self.head.len() < self.max {
            let line = line(text, stderr);
            if !line.text.trim().is_empty() {
                self.head.push(line);
            }
            return;
        }
        self.tail.push_back((text.to_owned(), stderr));
        if self.tail.len() > self.max {
            self.tail.pop_front();
            self.skipped += 1;
        }
    }

    /// The lines kept, and how many were left out between them.
    fn lines(self) -> (Vec<Line>, usize) {
        if self.unkept {
            let lines = (self.skipped > 0)
                .then(|| Line {
                    text: format!("⋯ {} lines not kept: they may hold secrets ⋯", self.skipped),
                    stderr: false,
                })
                .into_iter()
                .collect();
            return (lines, 0);
        }
        let mut lines = self.head;
        lines.extend(
            self.tail
                .into_iter()
                .map(|(text, stderr)| line(&text, stderr))
                .filter(|line| !line.text.trim().is_empty()),
        );
        (lines, self.skipped)
    }
}

/// `text` as lines, NULs of `-z` output as line breaks; of a progress line that `\r` rewrote,
/// what was there last.
fn pieces(text: &str) -> impl Iterator<Item = &str> {
    text.split(['\n', '\0'])
        .map(|line| line.rsplit('\r').find(|part| !part.trim().is_empty()).unwrap_or(""))
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
}

static ESCAPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07]*\x07").expect("valid regex"));

/// `text` without the terminal's escape codes, e.g. the `ESC[K` git prints to clear a line.
fn plain(text: &str) -> std::borrow::Cow<'_, str> {
    ESCAPE.replace_all(text, "")
}

/// A line printed by a running command, as the log keeps it.
pub(crate) fn line(text: &str, stderr: bool) -> Line {
    Line {
        text: mask(&plain(text)),
        stderr,
    }
}

/// At most `max_lines` lines and about `max_bytes` of `lines`: the first ones and the last few,
/// with a line saying how many were left out between them, `skipped` of them already.
fn keep(mut lines: Vec<Line>, (max_lines, max_bytes): (usize, usize), skipped: usize) -> Vec<Line> {
    let size = |lines: &[Line]| lines.iter().map(|l| l.text.len() + 1).sum::<usize>();
    if skipped == 0 && lines.len() <= max_lines && size(&lines) <= max_bytes {
        return lines;
    }
    for line in &mut lines {
        if line.text.len() > max_bytes / 8 {
            let mut end = max_bytes / 8;
            while !line.text.is_char_boundary(end) {
                end -= 1;
            }
            line.text.truncate(end);
            line.text.push('…');
        }
    }
    let tail = (max_lines / 8).max(2).min(lines.len());
    let mut head = max_lines.saturating_sub(tail + 1).min(lines.len() - tail);
    while head > 0 && size(&lines[..head]) + size(&lines[lines.len().saturating_sub(tail)..]) > max_bytes {
        head -= 1;
    }
    if skipped == 0 && head + tail >= lines.len() {
        return lines;
    }
    let left_out = lines.len() - head - tail + skipped;
    let last = lines.split_off(lines.len() - tail);
    lines.truncate(head);
    lines.push(Line {
        text: format!("⋯ {left_out} more lines ⋯"),
        stderr: false,
    });
    lines.extend(last);
    lines
}

const MASK: &str = "••••••";

static URL_USER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b([a-z][a-z0-9+.-]*://)([^/\s@'\x22]+)@").expect("valid regex"));
static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:gh[pousr]_[A-Za-z0-9]{16,}|github_pat_[A-Za-z0-9_]{20,}|glpat-[A-Za-z0-9_-]{16,}|xox[abprs]-[A-Za-z0-9-]{10,})")
        .expect("valid regex")
});
static SECRET_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(authorization:\s*(?:basic|bearer|token)?\s*|\bpassword\s*[=:]\s*['\x22]?|\baccess_token=)[^\s'\x22&]+",
    )
    .expect("valid regex")
});

/// `text` with passwords and tokens hidden: in addresses (`https://user:token@host`), sign-in
/// headers, tokens of GitHub, GitLab and Slack, and whatever the secrets check would stop in a
/// commit, the user's own patterns too. A user name alone stays, as in `git@github.com` or
/// `ssh://git@host`, except in an http address, where it is often a token.
pub fn mask(text: &str) -> String {
    let mut text = mask_known(text);
    // A line may hold more than one; a pattern of the user's that matches the dots must not
    // keep it going.
    for _ in 0..8 {
        match crate::secrets::check_line(&text) {
            Some((_, (start, end))) if &text[start..end] != MASK => text.replace_range(start..end, MASK),
            _ => break,
        }
    }
    text
}

fn mask_known(text: &str) -> String {
    let text = URL_USER.replace_all(text, |caps: &regex::Captures| {
        let scheme = &caps[1];
        let user = &caps[2];
        let http = scheme.to_ascii_lowercase().starts_with("http");
        if http || user.contains(':') {
            format!("{scheme}{MASK}@")
        } else {
            caps[0].to_owned()
        }
    });
    let text = TOKEN.replace_all(&text, MASK);
    SECRET_VALUE
        .replace_all(&text, |caps: &regex::Captures| format!("{}{MASK}", &caps[1]))
        .into_owned()
}

/// `n` with its thousands apart, as in "1 240".
pub fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('\u{202f}');
        }
        out.push(c);
    }
    out
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn elapsed_ms(since: Instant) -> u64 {
    since.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_and_tokens_are_masked() {
        assert_eq!(
            mask("To https://x-access-token:gho_abcdefghijklmnopqrstuv@github.com/a/b.git"),
            "To https://••••••@github.com/a/b.git"
        );
        assert_eq!(
            mask("https://anna@example.com/r.git"),
            "https://••••••@example.com/r.git"
        );
        assert_eq!(mask("git@github.com:acme/api.git"), "git@github.com:acme/api.git");
        assert_eq!(mask("ssh://git@host/r.git"), "ssh://git@host/r.git");
        assert_eq!(mask("ssh://git:secret@host/r.git"), "ssh://••••••@host/r.git");
        assert_eq!(mask("token ghp_0123456789abcdefABCDEF here"), "token •••••• here");
        assert_eq!(
            mask("-c http.extraHeader='Authorization: Bearer abc.def'"),
            "-c http.extraHeader='Authorization: Bearer ••••••'"
        );
        assert_eq!(mask("password=hunter2"), "password=••••••");
        assert_eq!(mask(r#"PASSWORD = "audit-password-123""#), r#"PASSWORD = "••••••""#);
        assert_eq!(mask(r#"+password="audit-password-123""#), r#"+password="••••••""#);
        assert_eq!(
            mask("git commit -m 'Add the console window'"),
            "git commit -m 'Add the console window'"
        );
    }

    #[test]
    fn output_is_split_into_lines() {
        let texts: Vec<&str> = pieces("a\0b\nCounting 10%\rCounting 100%\n\n").collect();
        assert_eq!(texts, ["a", "b", "Counting 100%"]);
        let mut output = Output::new(40, false);
        for line in pieces("\x1b[KSuccessfully rebased\n\x1b[K\n") {
            output.push(line, true);
        }
        let (lines, _) = output.lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "Successfully rebased");
    }

    #[test]
    fn long_output_keeps_its_start_and_end() {
        let lines: Vec<Line> = (0..100)
            .map(|n| Line {
                text: format!("line {n}"),
                stderr: false,
            })
            .collect();
        let kept = keep(lines, (40, 1 << 20), 0);
        assert_eq!(kept.len(), 40);
        assert_eq!(kept[0].text, "line 0");
        assert_eq!(kept[34].text, "⋯ 61 more lines ⋯");
        assert_eq!(kept[39].text, "line 99");
        // Of a very long one, only what is kept is masked: the lines between are counted.
        let mut output = Output::new(40, false);
        for n in 0..10_000 {
            output.push(&format!("line {n}"), false);
        }
        let (lines, skipped) = output.lines();
        assert_eq!((lines.len(), skipped), (80, 9_920));
        let kept = keep(lines, (40, 1 << 20), skipped);
        assert_eq!(kept.len(), 40);
        assert_eq!(kept[34].text, "⋯ 9961 more lines ⋯");
        assert_eq!(kept[39].text, "line 9999");
    }

    fn entry(kind: EntryKind, n: u64) -> Entry {
        Entry {
            id: n,
            time: 0,
            kind,
            title: String::new(),
            millis: 0,
            failed: false,
            stopped: false,
            gitoxide: false,
            note: None,
            commands: Vec::new(),
            running: false,
        }
    }

    #[test]
    fn reads_never_push_the_actions_out() {
        let mut all: Vec<Entry> = (0..50).map(|n| entry(EntryKind::Action, n)).collect();
        all.extend((50..1050).map(|n| entry(EntryKind::Read, n)));
        let kept = trimmed(all);
        assert_eq!(kept.len(), 350);
        assert_eq!(kept[0].id, 0);
        assert_eq!(kept[50].id, 750);
        let kept = trimmed((0..1200).map(|n| entry(EntryKind::Action, n)).collect());
        assert_eq!(kept.len(), KEEP);
        assert_eq!(kept[0].id, 200);
    }

    #[test]
    fn worktrees_of_a_repository_share_one_log() {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir.path().join("main"))
                .output()
                .unwrap()
        };
        std::fs::create_dir(dir.path().join("main")).unwrap();
        git(&["init", "-q"]);
        git(&["-c", "user.name=A", "-c", "user.email=a@b", "commit", "-q", "--allow-empty", "-m", "One"]);
        git(&["worktree", "add", "-q", "../linked"]);
        let main = log_file(&dir.path().join("main")).unwrap();
        assert_eq!(log_file(&dir.path().join("linked")), Some(main.clone()));
        assert!(!main.components().any(|c| c == std::path::Component::ParentDir));
    }

    #[test]
    fn a_scope_groups_its_processes_and_roles() {
        let dir = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        let repo = crate::Repo::open(dir.path()).unwrap();
        let result: Result<(), ()> = scope(Some(dir.path()), EntryKind::Action, "Commit", || {
            role("snapshot for Undo", || {
                let _ = repo.run(&crate::GitCommand::new(["write-tree"]));
            });
            let _ = repo.run(&crate::GitCommand::new(["rev-parse", "--git-dir"]));
            main(|| {
                let _ = repo.run(&crate::GitCommand::new(["status", "--short"]));
                let _ = repo.run(&crate::GitCommand::new(["no-such-command"]));
            });
            Err(())
        });
        assert!(result.is_err());
        let log = entries(Some(dir.path()));
        assert_eq!(log.len(), 1);
        let e = &log[0];
        assert!(e.failed && e.title == "Commit" && e.kind == EntryKind::Action);
        let roles: Vec<Option<&str>> = e.commands.iter().map(|c| c.role.as_deref()).collect();
        assert_eq!(roles, [Some("snapshot for Undo"), Some("check"), None, None]);
        assert_eq!(e.commands[1].output[0].text, ".git");
        assert_eq!(e.commands[2].code, Some(0));
        assert_eq!(e.commands[3].code, Some(1));
        // Without a scope a process is an entry of its own.
        let _ = repo.run(&crate::GitCommand::new(["rev-parse", "HEAD"]));
        let log = entries(Some(dir.path()));
        assert_eq!(log.len(), 2);
        assert_eq!(log[1].commands.len(), 1);
        assert_eq!(log[1].kind, EntryKind::Read);
        clear(Some(dir.path())).unwrap();
        assert!(entries(Some(dir.path())).is_empty());
    }
}
