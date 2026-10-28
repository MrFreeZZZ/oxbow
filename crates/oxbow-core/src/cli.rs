//! Running the system `git` for commands that change a repository.
//!
//! Every write goes through the `git` command line so hooks, credentials, SSH and the user's
//! config behave exactly as in a terminal. A [`GitCommand`] is also what the confirmation sheet
//! shows, so the user sees the very command that will run.

use std::cell::RefCell;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::console;
use crate::error::{Error, Result};
use crate::repo::Repo;

/// A folder only this user can open, made once per run with a random name, for files that git
/// or another app reads. In a temp folder shared with other users, as Linux's `/tmp` is, a
/// fixed name could be made first by someone else, or be a link to a file of yours.
pub fn private_temp_dir() -> std::io::Result<std::path::PathBuf> {
    static DIR: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);
    let mut dir = DIR.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(made) = dir.as_ref().filter(|d| d.is_dir()) {
        return Ok(made.clone());
    }
    let made = private_temp_folder("oxbow-")?.keep();
    *dir = Some(made.clone());
    Ok(made)
}

/// A new folder with a random name in the temp folder, deleted when dropped. It is made closed
/// to other users, not closed after: a random name alone keeps no one out.
pub fn private_temp_folder(prefix: &str) -> std::io::Result<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(prefix);
    #[cfg(unix)]
    builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700));
    builder.tempdir()
}

/// The folder at `path`, and the ones it is in, made if missing so that only this user can open
/// them.
pub fn create_private_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(path)
}

/// Write `data` to the file at `path`, which only this user can read when it is new.
pub fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(data)
}

/// One `git` invocation, with an optional explanation for the confirmation sheet.
/// It serializes with its `display` form too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommand {
    /// Arguments after `git`.
    pub args: Vec<String>,
    /// What the command does, shown as a `# comment` next to it.
    pub comment: Option<String>,
    /// Ask git for progress output even though it does not write to a terminal. Not shown, since
    /// nobody types it.
    pub progress: bool,
    /// What Oxbow feeds the command on standard input, shown so it can be checked, e.g. the
    /// patch of a few lines for `git apply`.
    pub input: Option<String>,
    /// A step Oxbow does itself before the command, as it would be typed in a shell, e.g.
    /// adding a line to `.gitignore`.
    pub before: Option<String>,
    /// Environment variables for the command, e.g. a scratch `GIT_INDEX_FILE`. Not shown.
    pub env: Vec<(String, String)>,
    /// The todo list Oxbow hands an interactive rebase instead of opening an editor; shown.
    pub todo: Option<String>,
    /// Another program than `git`, e.g. `ssh-add`; the arguments are its own.
    pub program: Option<String>,
    /// Run only when the first commit is an ancestor of the second at the moment the command's
    /// turn comes, after the commands before it ran; otherwise it is left out. For a
    /// fast-forward that depends on what a fetch before it brings.
    pub only_if_ancestor: Option<(String, String)>,
    /// Run only when the ref has moved from this commit (`None`: it did not exist) by the time
    /// the command's turn comes, e.g. a `stash pop` after a `stash push` that may save nothing.
    pub only_if_moved: Option<(String, Option<String>)>,
    /// Before running, stop when the secrets check finds something in the staged lines, as a
    /// commit does; for a commit that stages its files in an earlier command of the same action.
    pub secrets_check: bool,
    /// How long it may print nothing before it is taken to hang and stopped; `None` for one
    /// someone waits on, who can Stop it and may have a dialog to answer. Not shown.
    pub quiet_limit: Option<Duration>,
}

impl GitCommand {
    pub fn new<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        GitCommand {
            args: args.into_iter().map(Into::into).collect(),
            comment: None,
            progress: false,
            input: None,
            before: None,
            env: Vec::new(),
            todo: None,
            program: None,
            only_if_ancestor: None,
            only_if_moved: None,
            secrets_check: false,
            quiet_limit: Some(QUIET_LIMIT),
        }
    }

    pub fn program(mut self, program: impl Into<String>) -> Self {
        self.program = Some(program.into());
        self
    }

    pub fn comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = Some(comment.into());
        self
    }

    pub fn with_progress(mut self) -> Self {
        self.progress = true;
        self
    }

    pub fn input(mut self, input: impl Into<String>) -> Self {
        self.input = Some(input.into());
        self
    }

    pub fn before(mut self, step: impl Into<String>) -> Self {
        self.before = Some(step.into());
        self
    }

    pub fn todo(mut self, todo: impl Into<String>) -> Self {
        self.todo = Some(todo.into());
        self
    }

    pub fn only_if_ancestor(mut self, ancestor: impl Into<String>, of: impl Into<String>) -> Self {
        self.only_if_ancestor = Some((ancestor.into(), of.into()));
        self
    }

    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Stop it once it prints nothing for `limit`, sooner than [`QUIET_LIMIT`].
    pub fn quiet_limit(mut self, limit: Duration) -> Self {
        self.quiet_limit = Some(limit);
        self
    }

    /// Someone waits on it and can Stop it: it runs however long it is quiet, e.g. while a
    /// merge tool is open or a passphrase dialog waits.
    pub fn waits_for_user(mut self) -> Self {
        self.quiet_limit = None;
        self
    }

    /// Arguments as they are passed to git, `--progress` included.
    fn run_args(&self) -> Vec<&str> {
        let mut args: Vec<&str> = self.args.iter().map(String::as_str).collect();
        if self.progress && !args.is_empty() {
            // After the subcommand: `git submodule update --progress`, but `git push --progress`.
            let at = if args[0] == "submodule" { 2.min(args.len()) } else { 1 };
            args.insert(at, "--progress");
        }
        args
    }

    /// The command as it would be typed in a shell, with arguments quoted where needed.
    pub fn display(&self) -> String {
        std::iter::once(self.program.clone().unwrap_or_else(|| "git".to_owned()))
            .chain(self.args.iter().map(|a| shell_quote(a)))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl Serialize for GitCommand {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Shown<'a> {
            args: &'a [String],
            comment: &'a Option<String>,
            display: String,
            input: &'a Option<String>,
            before: &'a Option<String>,
            todo: &'a Option<String>,
        }
        Shown {
            args: &self.args,
            comment: &self.comment,
            display: self.display(),
            input: &self.input,
            before: &self.before,
            todo: &self.todo,
        }
        .serialize(serializer)
    }
}

/// One line a running command printed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputLine {
    pub text: String,
    /// Printed to standard error, where git writes its messages and progress.
    pub stderr: bool,
    /// Ended with a carriage return: a progress line that the next one replaces.
    pub progress: bool,
}

/// What a finished command printed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
}

impl Repo {
    /// Run `command` in the repository's working directory and wait for it.
    pub fn run(&self, command: &GitCommand) -> Result<CommandOutput> {
        self.run_with_input(command, None)
    }

    /// Run `command`, feeding `input` to its standard input (a patch for `git apply`).
    pub fn run_with_input(&self, command: &GitCommand, input: Option<&[u8]>) -> Result<CommandOutput> {
        run_in(self.workdir(), command, input)
    }

    /// Run `command` and return its raw output whatever its exit code.
    pub(crate) fn spawn(&self, command: &GitCommand, input: Option<&[u8]>) -> Result<std::process::Output> {
        spawn_in(self.workdir(), command, input)
    }

    /// Run `command` and wait for it, unless `cancel` is set first: the command is then stopped
    /// and fails with [`Error::Cancelled`]. For reads whose answer is no longer wanted once a
    /// newer one is asked for, such as a search.
    pub fn run_cancellable(&self, command: &GitCommand, cancel: &AtomicBool) -> Result<CommandOutput> {
        run_cancellable_in(self.workdir(), command, cancel)
    }

    /// Run `command` and read at most `max` bytes of what it prints: past that it is stopped, and
    /// the output is cut short. The exit code is not checked. For reads that can print more than
    /// anything shows, such as the patch of a huge file. Setting `cancel` stops it too, failing
    /// with [`Error::Cancelled`].
    pub(crate) fn spawn_cancellable_within(
        &self,
        command: &GitCommand,
        max: usize,
        cancel: &AtomicBool,
    ) -> Result<Limited> {
        let log = console::started(self.workdir(), &command.display());
        let (mut child, log) = start(command_in(self.workdir(), command), false, log)?;
        let waited = wait_for(&mut child, command, log, None, Some(cancel), Keep::First(max))?;
        Ok(Limited {
            success: waited.status.success() && !waited.cut,
            code: waited.status.code(),
            cut: waited.cut,
            stdout: waited.stdout,
            stderr: waited.stderr,
        })
    }

    /// Run `command`, passing each line it prints to `on_line` as it comes. Setting `cancel`
    /// stops the command; it then fails with [`Error::Cancelled`].
    pub fn run_streaming(
        &self,
        command: &GitCommand,
        on_line: &mut dyn FnMut(OutputLine),
        cancel: &AtomicBool,
    ) -> Result<CommandOutput> {
        run_streaming_in(self.workdir(), command, on_line, cancel)
    }
}

/// What [`Repo::spawn_cancellable_within`] read.
pub(crate) struct Limited {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// The command exited with 0; never set when it was cut.
    pub success: bool,
    pub code: Option<i32>,
    /// It printed more than was read, and was stopped.
    pub cut: bool,
}

thread_local! {
    /// The commands run on this thread while [`recording`] listens, as typed in a shell.
    static RAN: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
    /// The Stop of the read running on this thread: see [`stoppable`].
    static STOP: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

/// Run `f` so that setting `cancel` stops the git commands it runs on this thread, also those
/// deep inside it that take no Stop of their own: each then fails with [`Error::Cancelled`],
/// and none starts any more. For a read whose answer is no longer wanted, such as the history
/// of a repository the window has left.
pub fn stoppable<T>(cancel: &Arc<AtomicBool>, f: impl FnOnce() -> T) -> T {
    let outer = STOP.with(|stop| stop.borrow_mut().replace(cancel.clone()));
    // A panic in `f` must not leave the Stop in place for whatever runs on this thread next.
    struct Reset(Option<Arc<AtomicBool>>);
    impl Drop for Reset {
        fn drop(&mut self) {
            let outer = self.0.take();
            STOP.with(|stop| *stop.borrow_mut() = outer);
        }
    }
    let _reset = Reset(outer);
    f()
}

/// The Stop [`stoppable`] set on this thread, if any.
pub(crate) fn current_stop() -> Option<Arc<AtomicBool>> {
    STOP.with(|stop| stop.borrow().clone())
}

/// [`Error::Cancelled`] once the read running on this thread was stopped: for work between
/// git commands, such as walking the commits.
pub(crate) fn check_stop() -> Result<()> {
    match STOP.with(|stop| stop.borrow().as_ref().is_some_and(|s| s.load(Ordering::Relaxed))) {
        true => Err(Error::Cancelled),
        false => Ok(()),
    }
}

/// Run `f` and list the commands it ran on this thread, as typed in a shell: what a read such
/// as Reload shows it did.
pub(crate) fn recording<T>(f: impl FnOnce() -> T) -> (T, Vec<String>) {
    let outer = RAN.with(|ran| ran.borrow_mut().replace(Vec::new()));
    let value = f();
    let mine = RAN
        .with(|ran| std::mem::replace(&mut *ran.borrow_mut(), outer))
        .unwrap_or_default();
    // A recording around this one sees these commands too.
    RAN.with(|ran| {
        if let Some(outer) = ran.borrow_mut().as_mut() {
            outer.extend(mine.iter().cloned());
        }
    });
    (value, mine)
}

/// `git` set up to run `command` in `dir`.
fn command_in(dir: &Path, command: &GitCommand) -> Command {
    RAN.with(|ran| {
        if let Some(ran) = ran.borrow_mut().as_mut() {
            ran.push(command.display());
        }
    });
    let mut git = match &command.program {
        Some(program) => Command::new(program),
        None => crate::config::git(),
    };
    if command.program.is_none()
        && let Some(rest) = remote_args(&command.args)
    {
        crate::config::lend_github_token(&mut git, dir, rest);
    }
    git.args(command.run_args()).current_dir(dir);
    crate::trust::apply(&mut git, dir);
    git.envs(command.env.iter().map(|(k, v)| (k, v)));
    own_group(&mut git);
    git
}

/// Start the command as the head of a process group of its own, so that stopping it also stops
/// what it started: a hook, ssh, a credential helper.
fn own_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(not(unix))]
    let _ = command;
}

/// Stop `child` and every process it started. On Unix that is its process group (see
/// [`own_group`]), on Windows the tree of processes under it.
fn kill_tree(child: &mut Child) {
    #[cfg(unix)]
    if let Ok(pid) = i32::try_from(child.id()) {
        // SAFETY: kill(2) only takes two integers. The group is the child's own, and its id
        // can't be reused before the child is waited for.
        unsafe { libc::kill(-pid, libc::SIGKILL) };
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
}

/// The arguments after the subcommand of a git command that may need a password, one that
/// fetches from or pushes to a remote; `None` for any other command.
fn remote_args(args: &[String]) -> Option<&[String]> {
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        match arg.as_str() {
            // Options of git itself that take a value.
            "-c" | "-C" => i += 2,
            option if option.starts_with('-') => i += 1,
            command => {
                let remote = matches!(
                    command,
                    "push" | "pull" | "fetch" | "clone" | "ls-remote" | "remote" | "submodule" | "lfs"
                );
                let rest = &args[i + 1..];
                // `git lfs version` only asks git-lfs which version it is.
                let probe = command == "lfs" && rest.first().is_some_and(|a| a == "version");
                return (remote && !probe).then_some(rest);
            }
        }
    }
    None
}

/// Run `command` in `dir`, which need not be a repository (`clone`, `init`), and wait for it.
pub fn run_in(dir: &Path, command: &GitCommand, input: Option<&[u8]>) -> Result<CommandOutput> {
    let output = spawn_in(dir, command, input)?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.success() {
        Ok(CommandOutput { stdout, stderr })
    } else {
        Err(Error::Command {
            command: command.display(),
            code: output.status.code(),
            output: if stderr.trim().is_empty() { stdout } else { stderr },
        })
    }
}

fn spawn_in(dir: &Path, command: &GitCommand, input: Option<&[u8]>) -> Result<std::process::Output> {
    let stop = current_stop();
    if stop.as_ref().is_some_and(|s| s.load(Ordering::Relaxed)) {
        return Err(Error::Cancelled);
    }
    let log = console::started(dir, &command.display());
    let (mut child, log) = start(command_in(dir, command), input.is_some(), log)?;
    let keep = Keep::All(MAX_OUTPUT_BYTES);
    let waited = wait_for(&mut child, command, log, input, stop.as_deref(), keep)?;
    Ok(std::process::Output {
        status: waited.status,
        stdout: waited.stdout,
        stderr: waited.stderr,
    })
}

/// Run `program`, set up by the caller, the way a [`GitCommand`] runs: in the Console as
/// `shown`, stopped after [`QUIET_LIMIT`] without output or past [`MAX_OUTPUT_BYTES`]. For git
/// outside any repository and for ssh, which need no repository's settings.
pub(crate) fn output_of(mut program: Command, dir: &Path, shown: &GitCommand) -> Result<std::process::Output> {
    own_group(&mut program);
    let log = console::started(dir, &shown.display());
    let (mut child, log) = start(program, false, log)?;
    let waited = wait_for(&mut child, shown, log, None, None, Keep::All(MAX_OUTPUT_BYTES))?;
    Ok(std::process::Output {
        status: waited.status,
        stdout: waited.stdout,
        stderr: waited.stderr,
    })
}

/// Run `command` in `dir` and wait for it, unless `cancel` is set first: the command is then
/// stopped and fails with [`Error::Cancelled`].
pub fn run_cancellable_in(dir: &Path, command: &GitCommand, cancel: &AtomicBool) -> Result<CommandOutput> {
    let log = console::started(dir, &command.display());
    let (mut child, log) = start(command_in(dir, command), false, log)?;
    let waited = wait_for(
        &mut child,
        command,
        log,
        None,
        Some(cancel),
        Keep::All(MAX_OUTPUT_BYTES),
    )?;
    let stdout = String::from_utf8_lossy(&waited.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&waited.stderr).into_owned();
    if waited.status.success() {
        Ok(CommandOutput { stdout, stderr })
    } else {
        Err(Error::Command {
            command: command.display(),
            code: waited.status.code(),
            output: if stderr.trim().is_empty() { stdout } else { stderr },
        })
    }
}

/// How long a command may print nothing before it is taken to hang and stopped, with all it
/// started. Whatever it prints starts the wait over, so a slow command that shows progress runs
/// on. Commands someone waits on and can Stop have none: see [`GitCommand::waits_for_user`].
pub const QUIET_LIMIT: Duration = Duration::from_secs(10 * 60);

/// The most of a command's standard output read whole: past it the command is stopped and
/// fails rather than fill memory. The status of a million files is a small part of it.
const MAX_OUTPUT_BYTES: usize = 256 * 1024 * 1024;

/// How long to wait for a command's output to end once it has exited: something it started and
/// left running, such as a hook's job in the background, may hold the pipes open.
const AFTER_EXIT: Duration = Duration::from_millis(500);

/// Start `program` with its output piped, and `input` piped in when there is one. `log` is
/// told when it can't start.
fn start(mut program: Command, input: bool, log: console::Started) -> Result<(Child, console::Started)> {
    match program
        .stdin(if input { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => Ok((child, log)),
        Err(err) => {
            log.not_started(&err.to_string());
            Err(Error::GitNotFound(err.to_string()))
        }
    }
}

/// What to keep of a command's standard output.
#[derive(Clone, Copy)]
enum Keep {
    /// All of it: past this many bytes the command is stopped and fails.
    All(usize),
    /// This many bytes from the start: past them the command is stopped and its output cut.
    First(usize),
}

/// How a command ended, and what it printed.
struct Waited {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    /// It printed more than [`Keep::First`] keeps, and was stopped.
    cut: bool,
}

/// Some of what a command printed, as it came.
enum Piece {
    Out(Vec<u8>),
    Err(Vec<u8>),
    /// One of the two pipes ended.
    Closed,
}

/// Feed `input` to `child`, read what it prints and wait for it. It is stopped with all it
/// started when `cancel` is set, when it prints nothing for the command's quiet limit, and when
/// it prints more than `keep` allows. `log` is told how it ended.
fn wait_for(
    child: &mut Child,
    command: &GitCommand,
    log: console::Started,
    input: Option<&[u8]>,
    cancel: Option<&AtomicBool>,
    keep: Keep,
) -> Result<Waited> {
    let (send, receive) = mpsc::sync_channel(16);
    pump(child.stdout.take(), false, send.clone());
    pump(child.stderr.take(), true, send);
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        let input = input.to_vec();
        // A command that exits early closes its end; its exit status tells what went wrong.
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    let mut stdout = Vec::new();
    let mut stderr = Tail::new(MAX_KEPT_BYTES);
    let mut open = 2;
    let mut cut = false;
    let mut heard = Instant::now();
    let mut exited: Option<(std::process::ExitStatus, Instant)> = None;
    let gave_up = loop {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            if exited.is_none() {
                kill_tree(child);
                let _ = child.wait();
            }
            let output = log.output();
            log.stopped(output);
            return Err(Error::Cancelled);
        }
        if exited.is_none() {
            match child.try_wait() {
                Ok(status) => exited = status.map(|status| (status, Instant::now())),
                Err(err) => {
                    log.not_started(&err.to_string());
                    return Err(Error::GitNotFound(err.to_string()));
                }
            }
        }
        match exited {
            Some(_) if open == 0 => break None,
            // Exited, but something it left running holds its output: what came is all.
            Some((_, at)) if at.elapsed() >= AFTER_EXIT => {
                while let Ok(piece) = receive.try_recv() {
                    match piece {
                        Piece::Out(bytes) => stdout.extend_from_slice(&bytes),
                        Piece::Err(bytes) => stderr.push(&bytes),
                        Piece::Closed => {}
                    }
                }
                break None;
            }
            None if command.quiet_limit.is_some_and(|limit| heard.elapsed() >= limit) => {
                break Some(quiet_note(command.quiet_limit.unwrap_or_default()));
            }
            _ => {}
        }
        // Both pipes closed a moment before it exits.
        if open == 0 {
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }
        match receive.recv_timeout(Duration::from_millis(20)) {
            Ok(Piece::Out(bytes)) => {
                heard = Instant::now();
                stdout.extend_from_slice(&bytes);
                match keep {
                    Keep::All(max) if stdout.len() > max => {
                        stdout.clear();
                        break Some(format!("it printed more than {} MB", max.div_ceil(1024 * 1024)));
                    }
                    Keep::First(max) if stdout.len() > max => {
                        stdout.truncate(max);
                        cut = true;
                        break None;
                    }
                    _ => {}
                }
            }
            Ok(Piece::Err(bytes)) => {
                heard = Instant::now();
                stderr.push(&bytes);
            }
            Ok(Piece::Closed) => open -= 1,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => open = 0,
        }
    };
    let stderr = stderr.finish();
    // Once waited for, its id may go to another process: only a running command is stopped.
    let status = match exited {
        Some((status, _)) => status,
        None => {
            if gave_up.is_some() || cut {
                kill_tree(child);
            }
            match child.wait() {
                Ok(status) => status,
                Err(err) => {
                    log.not_started(&err.to_string());
                    return Err(Error::GitNotFound(err.to_string()));
                }
            }
        }
    };
    if let Some(why) = gave_up {
        return Err(gave_up_on(command, log, status.code(), &stdout, stderr, &why));
    }
    log.done(status.code(), &stdout, &stderr);
    Ok(Waited {
        status,
        stdout,
        stderr,
        cut,
    })
}

/// Read `pipe` on a thread of its own, sending what comes as it comes, then [`Piece::Closed`].
fn pump(pipe: Option<impl Read + Send + 'static>, stderr: bool, send: mpsc::SyncSender<Piece>) {
    std::thread::spawn(move || {
        if let Some(mut pipe) = pipe {
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                let n = match pipe.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                };
                let bytes = buffer[..n].to_vec();
                // Nobody reads any more: the command was stopped, or left this pipe open.
                if send
                    .send(if stderr { Piece::Err(bytes) } else { Piece::Out(bytes) })
                    .is_err()
                {
                    return;
                }
            }
        }
        let _ = send.send(Piece::Closed);
    });
}

/// Why Oxbow stopped a command that printed nothing for `limit`, in its words.
fn quiet_note(limit: Duration) -> String {
    match limit.as_secs() {
        secs if secs >= 120 => format!("nothing came from it for {} minutes", secs / 60),
        secs => format!("nothing came from it for {secs} seconds"),
    }
}

/// The failure of `command`, stopped by Oxbow for `why`; `log` is told so.
fn gave_up_on(
    command: &GitCommand,
    log: console::Started,
    code: Option<i32>,
    stdout: &[u8],
    stderr: Vec<u8>,
    why: &str,
) -> Error {
    let mut said = String::from_utf8_lossy(&stderr).into_owned();
    if said.trim().is_empty() {
        said = String::from_utf8_lossy(stdout).into_owned();
    }
    if !said.is_empty() && !said.ends_with('\n') {
        said.push('\n');
    }
    said.push_str(&format!("Oxbow stopped it: {why}."));
    log.done(code, stdout, said.as_bytes());
    Error::Command {
        command: command.display(),
        code,
        output: said,
    }
}

/// The end of what a command printed to standard error, at most `max` bytes from a line
/// start: what a remote prints there has no end of its own.
struct Tail {
    all: Vec<u8>,
    max: usize,
    cut: bool,
}

impl Tail {
    fn new(max: usize) -> Self {
        Tail {
            all: Vec::new(),
            max,
            cut: false,
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        self.all.extend_from_slice(bytes);
        // Dropped in large steps, so a long output isn't moved for every piece.
        if self.all.len() > 2 * self.max {
            self.all.drain(..self.all.len() - self.max);
            self.cut = true;
        }
    }

    /// All of it, or its last `max` bytes from a line start, saying so.
    fn finish(self) -> Vec<u8> {
        let Tail { all, max, cut } = self;
        if !cut && all.len() <= max {
            return all;
        }
        let from = all.len().saturating_sub(max);
        let start = all[from..]
            .iter()
            .position(|&c| c == b'\n')
            .map_or(all.len(), |i| from + i + 1);
        let mut kept = b"\xe2\x80\xa6 (earlier output cut)\n".to_vec();
        kept.extend_from_slice(&all[start..]);
        kept
    }
}

/// A line of a command's output longer than this is cut; the rest of it is dropped.
const MAX_LINE_BYTES: usize = 16 * 1024;

/// Lines read but not yet handled, per command: a command that prints faster than its lines
/// are handled waits for them instead of filling memory.
const QUEUED_LINES: usize = 1024;

/// The output kept of a streamed command, per stream: past it, the oldest lines go.
const MAX_KEPT_BYTES: usize = 4 * 1024 * 1024;

/// Run `command` in `dir`, passing each line it prints to `on_line` as it comes. Setting
/// `cancel` stops the command; it then fails with [`Error::Cancelled`].
pub fn run_streaming_in(
    dir: &Path,
    command: &GitCommand,
    on_line: &mut dyn FnMut(OutputLine),
    cancel: &AtomicBool,
) -> Result<CommandOutput> {
    let log = console::started(dir, &command.display());
    let (mut child, log) = start(command_in(dir, command), false, log)?;
    let (send, receive) = mpsc::sync_channel(QUEUED_LINES);
    // What the Console shows: the lines in the order they came, without progress.
    let mut logged = log.output();
    read_lines(child.stdout.take().expect("stdout is piped"), false, send.clone());
    read_lines(child.stderr.take().expect("stderr is piped"), true, send);
    let mut output = CommandOutput::default();
    let mut heard = Instant::now();
    let mut exited: Option<(std::process::ExitStatus, Instant)> = None;
    // When both pipes closed. That doesn't mean it is done: it may have sent its output
    // elsewhere and go on, and Stop still stops it.
    let mut closed: Option<Instant> = None;
    let status = loop {
        // Before each line: a command that prints without a pause still stops at once.
        if cancel.load(Ordering::Relaxed) {
            if exited.is_none() {
                kill_tree(&mut child);
                let _ = child.wait();
            }
            log.stopped(logged);
            return Err(Error::Cancelled);
        }
        let next = if let Some(at) = closed {
            // Most often it exits a moment after closing them: look often at first, so each
            // git command doesn't take 20 ms more; one that went on working, less often.
            let pause = if at.elapsed() < Duration::from_millis(50) {
                1
            } else {
                20
            };
            std::thread::sleep(Duration::from_millis(pause));
            Err(mpsc::RecvTimeoutError::Timeout)
        } else {
            receive.recv_timeout(Duration::from_millis(50))
        };
        match next {
            Ok(line) => {
                heard = Instant::now();
                if !line.progress {
                    let all = if line.stderr {
                        &mut output.stderr
                    } else {
                        &mut output.stdout
                    };
                    all.push_str(&line.text);
                    all.push('\n');
                    keep_tail(all, MAX_KEPT_BYTES);
                    logged.push(&line.text, line.stderr);
                }
                on_line(line);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => match exited {
                // Exited, but something it left running holds its output: what came is all.
                Some((status, at)) if closed.is_some() || at.elapsed() >= AFTER_EXIT => break status,
                Some(_) => {}
                None => match child.try_wait() {
                    Ok(Some(status)) if closed.is_some() => break status,
                    Ok(Some(status)) => exited = Some((status, Instant::now())),
                    Ok(None) if command.quiet_limit.is_some_and(|limit| heard.elapsed() >= limit) => {
                        kill_tree(&mut child);
                        let code = child.wait().ok().and_then(|status| status.code());
                        let why = quiet_note(command.quiet_limit.unwrap_or_default());
                        let said = if output.stderr.trim().is_empty() {
                            output.stdout
                        } else {
                            output.stderr
                        };
                        let note = format!("Oxbow stopped it: {why}.");
                        logged.push(&note, true);
                        log.done_lines(code, logged);
                        return Err(Error::Command {
                            command: command.display(),
                            code,
                            output: format!("{said}{note}"),
                        });
                    }
                    Ok(None) => {}
                    Err(err) => {
                        log.not_started(&err.to_string());
                        return Err(Error::GitNotFound(err.to_string()));
                    }
                },
            },
            Err(mpsc::RecvTimeoutError::Disconnected) => match exited {
                Some((status, _)) => break status,
                None => closed = Some(Instant::now()),
            },
        }
    };
    log.done_lines(status.code(), logged);
    if status.success() {
        Ok(output)
    } else {
        Err(Error::Command {
            command: command.display(),
            code: status.code(),
            output: if output.stderr.trim().is_empty() {
                output.stdout
            } else {
                output.stderr
            },
        })
    }
}

/// Drop whole lines from the start of `text` until it is at most `max` bytes, saying so once.
fn keep_tail(text: &mut String, max: usize) {
    const CUT: &str = "… (earlier output cut)\n";
    if text.len() <= max {
        return;
    }
    let mut from = (text.len() + CUT.len()).saturating_sub(max).min(text.len());
    while !text.is_char_boundary(from) {
        from += 1;
    }
    let start = text[from..].find('\n').map_or(text.len(), |i| from + i + 1);
    text.replace_range(..start, CUT);
}

/// Read `pipe` on a thread of its own, sending each line split at `\n` or `\r`, at most
/// [`MAX_LINE_BYTES`] of it.
fn read_lines(
    mut pipe: impl Read + Send + 'static,
    stderr: bool,
    send: mpsc::SyncSender<OutputLine>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut pending: Vec<u8> = Vec::new();
        let mut cut = false;
        let mut buffer = [0u8; 4096];
        let emit = |bytes: &[u8], cut: bool, progress: bool| {
            let mut text = String::from_utf8_lossy(bytes).trim_end().to_owned();
            if cut {
                text.push_str(" …");
            }
            if !text.is_empty() {
                let _ = send.send(OutputLine { text, stderr, progress });
            }
        };
        loop {
            let n = match pipe.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            for &byte in &buffer[..n] {
                if byte == b'\n' || byte == b'\r' {
                    emit(&pending, cut, byte == b'\r');
                    pending.clear();
                    cut = false;
                } else if pending.len() < MAX_LINE_BYTES {
                    pending.push(byte);
                } else {
                    cut = true;
                }
            }
        }
        emit(&pending, cut, false);
    })
}

/// `path` as a pathspec that names just that file. git reads `*`, `?`, `[` and a leading `:`
/// in a pathspec as a pattern, so `a*.txt` alone would also pick `another.txt`; such paths get
/// the `:(literal)` prefix. Paths without them stay as they are, to keep the shown commands plain.
pub fn literal(path: &str) -> String {
    if path.starts_with(':') || path.contains(['*', '?', '[', '\\']) {
        format!(":(literal){path}")
    } else {
        path.to_owned()
    }
}

/// Whether `text` is a full object id (SHA-1 or SHA-256), as git prints it: an id read back
/// from git's output that is anything else came from a commit message, not from git.
pub(crate) fn is_oid(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Quote an argument for display the way a POSIX shell would need it.
pub(crate) fn shell_quote(arg: &str) -> String {
    let plain = !arg.is_empty() && arg.chars().all(|c| c.is_alphanumeric() || "-_./:=@+,%^~{}".contains(c));
    if plain {
        arg.to_owned()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kept_output_loses_its_oldest_lines() {
        let mut text = "one\ntwo\nthree\nfour\n".to_owned();
        keep_tail(&mut text, 100);
        assert_eq!(text, "one\ntwo\nthree\nfour\n");
        let mut text = format!("{}\nlast line\n", "x".repeat(200));
        keep_tail(&mut text, 60);
        assert_eq!(text, "… (earlier output cut)\nlast line\n");
        assert!(text.len() <= 60);
    }

    #[test]
    fn endless_stderr_keeps_its_end() {
        let tail = |pieces: Vec<Vec<u8>>| {
            let mut tail = Tail::new(1000);
            for piece in pieces {
                tail.push(&piece);
            }
            tail.finish()
        };
        let small = b"one\ntwo\n".to_vec();
        assert_eq!(tail(vec![small.clone()]), small);
        let lines = (0..10_000)
            .map(|i| format!("remote: line {i}\n").into_bytes())
            .collect();
        let kept = String::from_utf8(tail(lines)).unwrap();
        assert!(kept.starts_with("… (earlier output cut)\nremote: line "));
        assert!(kept.ends_with("remote: line 9999\n"));
        assert!(kept.len() <= 1000 + "… (earlier output cut)\n".len());
    }

    #[test]
    fn a_very_long_line_is_cut() {
        let (send, receive) = mpsc::sync_channel(4);
        let data = format!("{}\nnext\n", "y".repeat(MAX_LINE_BYTES * 3));
        read_lines(std::io::Cursor::new(data.into_bytes()), false, send)
            .join()
            .unwrap();
        let lines: Vec<OutputLine> = receive.iter().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text.len(), MAX_LINE_BYTES + " …".len());
        assert_eq!(lines[1].text, "next");
    }

    #[cfg(unix)]
    #[test]
    fn stop_stops_a_command_that_prints_without_a_pause() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let mut lines = 0;
        let started = std::time::Instant::now();
        // `yes` prints for ever, faster than the lines are taken: there is always one waiting.
        let result = run_streaming_in(
            dir.path(),
            &GitCommand::new(["y"]).program("yes"),
            &mut |_| {
                lines += 1;
                if lines == 200 {
                    cancel.store(true, Ordering::Relaxed);
                }
                std::thread::sleep(Duration::from_micros(200));
            },
            &cancel,
        );
        assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
        assert!(lines < 300, "{lines} lines after Stop");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[cfg(unix)]
    #[test]
    fn stop_stops_what_the_command_started_too() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        // Like a hook or ssh that git starts: a process of its own under the command.
        let script = format!("sleep 30 & echo $! > '{}'; wait", pid_file.display());
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let file = pid_file.clone();
        let later = std::thread::spawn(move || {
            while !file.exists() {
                std::thread::sleep(Duration::from_millis(10));
            }
            stop.store(true, Ordering::Relaxed);
        });
        let result = run_cancellable_in(dir.path(), &GitCommand::new(["-c", &script]).program("sh"), &cancel);
        later.join().unwrap();
        assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
        let pid = std::fs::read_to_string(&pid_file).unwrap();
        let alive = || {
            std::process::Command::new("kill")
                .args(["-0", pid.trim()])
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        };
        let started = std::time::Instant::now();
        while alive() && started.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!alive(), "sleep {} still runs", pid.trim());
    }

    #[cfg(unix)]
    #[test]
    fn output_past_the_limit_is_cut_and_the_command_stopped() {
        let dir = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        let repo = Repo::open(dir.path()).unwrap();
        // `yes` would print for ever.
        let never = AtomicBool::new(false);
        let out = repo
            .spawn_cancellable_within(&GitCommand::new(["y"]).program("yes"), 1000, &never)
            .unwrap();
        assert!(out.cut && !out.success);
        assert_eq!(out.stdout.len(), 1000);
        let out = repo
            .spawn_cancellable_within(&GitCommand::new(["--version"]), 1000, &never)
            .unwrap();
        assert!(!out.cut && out.success);
        assert!(out.stdout.starts_with(b"git version"));
    }

    #[cfg(unix)]
    #[test]
    fn a_bounded_read_waits_for_a_command_that_closes_its_output_early() {
        let dir = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        let repo = Repo::open(dir.path()).unwrap();
        let never = AtomicBool::new(false);
        let sh = |script: &str| GitCommand::new(["-c", script]).program("sh");
        // Prints its output, closes it, and exits a moment later.
        let early = sh("printf 0123456789abcdef; exec 1>&-; sleep 0.2; exit 0");
        for max in [100, 16] {
            let out = repo.spawn_cancellable_within(&early, max, &never).unwrap();
            assert!(out.success && !out.cut, "max {max}");
            assert_eq!(out.stdout, b"0123456789abcdef");
        }
        let out = repo.spawn_cancellable_within(&early, 15, &never).unwrap();
        assert!(out.cut && !out.success);
        assert_eq!(out.stdout, b"0123456789abcde");
        // A command that fails is not taken for one cut short.
        let out = repo
            .spawn_cancellable_within(&sh("printf x; exit 3"), 100, &never)
            .unwrap();
        assert!(!out.success && !out.cut && out.code == Some(3));
        // Stopped while it still prints.
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let later = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            stop.store(true, Ordering::Relaxed);
        });
        let started = std::time::Instant::now();
        let out = repo.spawn_cancellable_within(&sh("printf x; sleep 5"), 100, &cancel);
        later.join().unwrap();
        assert!(matches!(out, Err(Error::Cancelled)));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn a_command_quiet_for_too_long_is_stopped() {
        let dir = tempfile::tempdir().unwrap();
        let sh = |script: &str| {
            GitCommand::new(["-c", script])
                .program("sh")
                .quiet_limit(Duration::from_millis(400))
        };
        let started = Instant::now();
        let result = run_in(dir.path(), &sh("echo working >&2; sleep 30"), None);
        let Err(Error::Command { output, .. }) = result else {
            panic!("{result:?}");
        };
        assert_eq!(output, "working\nOxbow stopped it: nothing came from it for 0 seconds.");
        assert!(started.elapsed() < Duration::from_secs(5));
        let never = AtomicBool::new(false);
        let result = run_streaming_in(dir.path(), &sh("sleep 30"), &mut |_| {}, &never);
        assert!(matches!(result, Err(Error::Command { code: None, .. })), "{result:?}");
        assert!(started.elapsed() < Duration::from_secs(10));
        // Slow, but printing all along.
        let busy = sh("for i in 1 2 3 4 5 6; do echo $i; sleep 0.2; done");
        assert_eq!(run_in(dir.path(), &busy, None).unwrap().stdout, "1\n2\n3\n4\n5\n6\n");
        assert!(run_streaming_in(dir.path(), &busy, &mut |_| {}, &never).is_ok());
        // Someone waits on it: no limit.
        let waiting = sh("sleep 1; echo done").waits_for_user();
        assert_eq!(run_in(dir.path(), &waiting, None).unwrap().stdout, "done\n");
    }

    #[cfg(unix)]
    #[test]
    fn output_past_what_is_read_whole_fails() {
        let dir = tempfile::tempdir().unwrap();
        let command = GitCommand::new(["y"]).program("yes");
        let log = console::started(dir.path(), &command.display());
        let (mut child, log) = start(command_in(dir.path(), &command), false, log).unwrap();
        let result = wait_for(&mut child, &command, log, None, None, Keep::All(100_000));
        let Err(Error::Command { output, .. }) = result else {
            panic!("expected a failure");
        };
        assert_eq!(output, "Oxbow stopped it: it printed more than 1 MB.");
    }

    #[cfg(unix)]
    #[test]
    fn a_command_is_done_when_it_exits_even_if_what_it_started_runs_on() {
        let dir = tempfile::tempdir().unwrap();
        // Like a hook that starts a job in the background, which keeps the output open.
        let command = GitCommand::new(["-c", "sleep 30 & echo $!"]).program("sh");
        let kill = |pid: &str| {
            let _ = std::process::Command::new("kill").arg(pid.trim()).status();
        };
        let started = Instant::now();
        let out = run_in(dir.path(), &command, None).unwrap();
        kill(&out.stdout);
        assert!(out.stdout.trim().parse::<u32>().is_ok(), "{out:?}");
        let never = AtomicBool::new(false);
        let mut lines = Vec::new();
        run_streaming_in(dir.path(), &command, &mut |line| lines.push(line.text), &never).unwrap();
        kill(&lines.concat());
        assert_eq!(lines.len(), 1);
        assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    }

    #[cfg(unix)]
    #[test]
    fn stop_ends_a_streamed_command_that_closed_its_output() {
        let dir = tempfile::tempdir().unwrap();
        // Like a hook that sends its output elsewhere and goes on working.
        let late = dir.path().join("late");
        let script = format!("exec >/dev/null 2>&1; sleep 2; touch '{}'", late.display());
        let command = GitCommand::new(["-c", &script]).program("sh");
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            stop.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        let result = run_streaming_in(dir.path(), &command, &mut |_| {}, &cancel);
        assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
        assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
        std::thread::sleep(Duration::from_millis(2500));
        assert!(!late.exists(), "it went on after Stop");
        // Not stopped, it is waited for to the end.
        let never = AtomicBool::new(false);
        run_streaming_in(dir.path(), &command, &mut |_| {}, &never).unwrap();
        assert!(late.exists());
    }

    #[test]
    fn commands_that_reach_a_remote_are_told_apart() {
        let talks = |args: &[&str]| remote_args(&args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>()).is_some();
        assert!(talks(&["push", "-u", "origin", "main"]));
        assert!(talks(&["-c", "core.hooksPath=x", "fetch", "--prune"]));
        assert!(talks(&["-C", "/tmp/a", "clone", "https://github.com/a/b"]));
        assert!(!talks(&["commit", "-m", "push the button"]));
        assert!(!talks(&["-c", "push.default=simple", "status"]));
        assert!(!talks(&["lfs", "version"]));
    }

    #[test]
    fn display_quotes_only_what_a_shell_needs() {
        let cmd = GitCommand::new(["commit", "-m", "Fix the user's login", "--", "src/a b.rs"]);
        assert_eq!(
            cmd.display(),
            r"git commit -m 'Fix the user'\''s login' -- 'src/a b.rs'"
        );
        assert_eq!(
            GitCommand::new(["switch", "auth/3-ui"]).display(),
            "git switch auth/3-ui"
        );
        assert_eq!(GitCommand::new(["reset", "HEAD~1"]).display(), "git reset HEAD~1");
        assert_eq!(
            GitCommand::new(["stash", "apply", "stash@{0}"]).display(),
            "git stash apply stash@{0}"
        );
    }
}
