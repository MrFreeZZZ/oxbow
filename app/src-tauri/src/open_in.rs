//! Settings › Integrations › Open in: the editors and terminals installed on this computer,
//! and opening a file or the repository folder in them.

use std::ffi::{OsStr, OsString};
use std::io::{ErrorKind, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};

use oxbow_core::config::find_on_path;
use serde::Serialize;

mod editor_discovery;
use editor_discovery::EditorDiscovery;

/// An editor or terminal that is installed.
#[derive(Debug, Clone, Serialize)]
pub struct App {
    pub id: &'static str,
    pub name: &'static str,
}

const EDITORS: [App; 21] = [
    App {
        id: "vscode",
        name: "Visual Studio Code",
    },
    App { id: "zed", name: "Zed" },
    App {
        id: "sublime",
        name: "Sublime Text",
    },
    App {
        id: "xcode",
        name: "Xcode",
    },
    App {
        id: "pycharm",
        name: "PyCharm",
    },
    App {
        id: "idea",
        name: "IntelliJ IDEA",
    },
    App {
        id: "webstorm",
        name: "WebStorm",
    },
    App {
        id: "goland",
        name: "GoLand",
    },
    App {
        id: "phpstorm",
        name: "PhpStorm",
    },
    App {
        id: "rubymine",
        name: "RubyMine",
    },
    App {
        id: "rider",
        name: "Rider",
    },
    App {
        id: "clion",
        name: "CLion",
    },
    App {
        id: "rustrover",
        name: "RustRover",
    },
    App {
        id: "visualstudio",
        name: "Visual Studio",
    },
    App {
        id: "android-studio",
        name: "Android Studio",
    },
    App {
        id: "notepad++",
        name: "Notepad++",
    },
    App {
        id: "cursor",
        name: "Cursor",
    },
    App {
        id: "eclipse",
        name: "Eclipse",
    },
    App {
        id: "emacs",
        name: "Emacs",
    },
    App { id: "vim", name: "Vim" },
    App {
        id: "neovim",
        name: "Neovim",
    },
];

/// A JetBrains IDE: the names its app has on macOS (the installer's, then Toolbox's), and its
/// command line launchers elsewhere (Toolbox's script, the installer's, then the Snap's).
struct JetBrains {
    id: &'static str,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    bundles: &'static [&'static str],
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    launchers: &'static [&'static str],
}

const JETBRAINS: [JetBrains; 9] = [
    JetBrains {
        id: "pycharm",
        bundles: &[
            "PyCharm",
            "PyCharm Professional Edition",
            "PyCharm Professional",
            "PyCharm CE",
            "PyCharm Community Edition",
        ],
        launchers: &[
            "pycharm",
            "pycharm.cmd",
            "pycharm64",
            "pycharm.sh",
            "pycharm-professional",
            "pycharm-community",
        ],
    },
    JetBrains {
        id: "idea",
        bundles: &[
            "IntelliJ IDEA",
            "IntelliJ IDEA Ultimate",
            "IntelliJ IDEA CE",
            "IntelliJ IDEA Community Edition",
        ],
        launchers: &[
            "idea",
            "idea.cmd",
            "idea64",
            "idea.sh",
            "intellij-idea-ultimate",
            "intellij-idea-community",
        ],
    },
    JetBrains {
        id: "webstorm",
        bundles: &["WebStorm"],
        launchers: &["webstorm", "webstorm.cmd", "webstorm64", "webstorm.sh"],
    },
    JetBrains {
        id: "goland",
        bundles: &["GoLand"],
        launchers: &["goland", "goland.cmd", "goland64", "goland.sh"],
    },
    JetBrains {
        id: "phpstorm",
        bundles: &["PhpStorm"],
        launchers: &["phpstorm", "phpstorm.cmd", "phpstorm64", "phpstorm.sh"],
    },
    JetBrains {
        id: "rubymine",
        bundles: &["RubyMine"],
        launchers: &["rubymine", "rubymine.cmd", "rubymine64", "rubymine.sh"],
    },
    JetBrains {
        id: "rider",
        bundles: &["Rider"],
        launchers: &["rider", "rider.cmd", "rider64", "rider.sh"],
    },
    JetBrains {
        id: "clion",
        bundles: &["CLion"],
        launchers: &["clion", "clion.cmd", "clion64", "clion.sh"],
    },
    JetBrains {
        id: "rustrover",
        bundles: &["RustRover"],
        launchers: &["rustrover", "rustrover.cmd", "rustrover64", "rustrover.sh"],
    },
];

fn jetbrains(id: &str) -> Option<&'static JetBrains> {
    JETBRAINS.iter().find(|ide| ide.id == id)
}

const TERMINALS: [App; 7] = [
    App {
        id: "terminal",
        name: "Terminal",
    },
    App {
        id: "ghostty",
        name: "Ghostty",
    },
    App {
        id: "iterm",
        name: "iTerm",
    },
    App {
        id: "gnome-terminal",
        name: "GNOME Terminal",
    },
    App {
        id: "konsole",
        name: "Konsole",
    },
    App {
        id: "wt",
        name: "Windows Terminal",
    },
    App {
        id: "xterm",
        name: "XTerm",
    },
];

pub fn editors() -> Vec<App> {
    let discovery = EditorDiscovery::default();
    EDITORS
        .into_iter()
        .filter(|app| editor_command(app.id, &discovery).is_some())
        .collect()
}

pub fn default_editor_id(editors: &[App]) -> Option<&'static str> {
    editors
        .iter()
        .find(|app| !cfg!(target_os = "macos") || app.id != "vim")
        .map(|app| app.id)
}

pub fn terminals() -> Vec<App> {
    TERMINALS.into_iter().filter(|app| terminal_installed(app.id)).collect()
}

/// A macOS app bundle in /Applications or ~/Applications, or where JetBrains Toolbox used to put
/// its IDEs.
#[cfg(target_os = "macos")]
fn bundle(name: &str) -> Option<PathBuf> {
    let home = oxbow_core::config::home_dir().map(|home| home.join("Applications"));
    [
        Some(PathBuf::from("/Applications")),
        Some(PathBuf::from("/System/Applications/Utilities")),
        home.clone(),
        home.map(|home| home.join("JetBrains Toolbox")),
    ]
    .into_iter()
    .flatten()
    .map(|dir| dir.join(format!("{name}.app")))
    .find(|path| path.is_dir())
}

/// The command line tool that opens a file at a line. On macOS it lives inside the app,
/// since apps started from the Dock do not see the shell's PATH; a JetBrains IDE is its app,
/// which `open` starts.
fn editor_command(id: &str, discovery: &EditorDiscovery) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    if terminal_editor(id) && terminal_editor_host(None).is_none() {
        return None;
    }
    preferred_editor_command(id).or_else(|| discovery.find(id))
}

fn preferred_editor_command(id: &str) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let inside = |app: &str, tool: &str| bundle(app).map(|path| path.join(tool)).filter(|path| path.is_file());
        match id {
            "vscode" => inside("Visual Studio Code", "Contents/Resources/app/bin/code"),
            "zed" => inside("Zed", "Contents/MacOS/cli"),
            "sublime" => inside("Sublime Text", "Contents/SharedSupport/bin/subl"),
            "xcode" => bundle("Xcode").map(|_| PathBuf::from("/usr/bin/xed")),
            "android-studio" => bundle("Android Studio"),
            "cursor" => inside("Cursor", "Contents/Resources/app/bin/cursor"),
            "eclipse" => inside("Eclipse", "Contents/MacOS/eclipse"),
            "emacs" => inside("Emacs", "Contents/MacOS/Emacs"),
            "vim" => find_on_path("vim"),
            "neovim" => find_on_path("nvim"),
            _ => jetbrains(id).and_then(|ide| ide.bundles.iter().find_map(|name| bundle(name))),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        match id {
            #[cfg(windows)]
            "vscode" => find_on_path("code.exe"),
            #[cfg(not(windows))]
            "vscode" => find_on_path("code").or_else(|| find_on_path("code.cmd")),
            "zed" => find_on_path("zed").or_else(|| find_on_path("zeditor")),
            "sublime" => find_on_path("subl"),
            #[cfg(windows)]
            "cursor" => find_on_path("cursor.exe"),
            #[cfg(not(windows))]
            "cursor" => find_on_path("cursor"),
            "android-studio" => ["android-studio", "studio64", "studio", "studio.sh"]
                .into_iter()
                .find_map(find_on_path),
            "eclipse" => find_on_path("eclipse"),
            "vim" => find_on_path("vim"),
            "neovim" => find_on_path("nvim"),
            #[cfg(windows)]
            "visualstudio" => find_on_path("devenv"),
            #[cfg(windows)]
            "notepad++" => find_on_path("notepad++"),
            #[cfg(windows)]
            "emacs" => find_on_path("runemacs"),
            #[cfg(target_os = "linux")]
            "emacs" => ["emacs-gtk", "emacs-pgtk", "emacs-lucid", "emacs"]
                .into_iter()
                .find_map(|name| find_on_path(name).filter(|path| editor_discovery::graphical_emacs(path))),
            _ => jetbrains(id).and_then(|ide| ide.launchers.iter().find_map(|name| find_on_path(name))),
        }
    }
}

fn terminal_installed(id: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        match id {
            "terminal" => true,
            "ghostty" => bundle("Ghostty").is_some(),
            "iterm" => bundle("iTerm").is_some(),
            _ => false,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        match id {
            "ghostty" | "gnome-terminal" | "konsole" | "wt" | "xterm" => find_on_path(id).is_some(),
            _ => false,
        }
    }
}

/// The existing working file, with its original name for the system's file association.
pub fn working_file(root: &Path, path: &str) -> Result<PathBuf, String> {
    let relative = Path::new(path);
    let invalid = || format!("{path} is not a file inside this repository.");
    if path.is_empty() || !relative.components().all(|part| matches!(part, Component::Normal(_))) {
        return Err(invalid());
    }
    let file = root.join(relative);
    let real = file.canonicalize().map_err(|err| {
        if err.kind() == ErrorKind::NotFound {
            format!("The working copy of {path} is missing from disk.")
        } else {
            format!("Could not access the working copy of {path}: {err}")
        }
    })?;
    let root = root.canonicalize().map_err(|err| err.to_string())?;
    if !real.starts_with(root) || !real.is_file() {
        return Err(invalid());
    }
    Ok(file)
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum WorkingFileStatus {
    Text,
    Binary,
    Unavailable { reason: String },
}

pub(crate) fn inspect_working_file(root: &Path, path: &str) -> Result<(PathBuf, WorkingFileStatus), String> {
    let file = working_file(root, path)?;
    let mut head = Vec::new();
    // Match the diff's binary test, but read the current working file.
    std::fs::File::open(&file)
        .and_then(|f| f.take(8000).read_to_end(&mut head))
        .map_err(|err| match err.kind() {
            ErrorKind::NotFound => format!("The working copy of {path} is missing from disk."),
            _ => format!("Could not read the working copy of {path}: {err}"),
        })?;
    let status = if head.contains(&0) {
        WorkingFileStatus::Binary
    } else {
        WorkingFileStatus::Text
    };
    Ok((file, status))
}

/// Open `path` in the editor, at `line` when the editor can.
pub fn open_file(editor: &str, path: &Path, line: Option<u32>, terminal: Option<&str>) -> Result<(), String> {
    let tool =
        editor_command(editor, &EditorDiscovery::default()).ok_or_else(|| format!("{editor} is not installed"))?;
    let args = file_args(editor, path, line);
    if terminal_editor(editor) {
        return open_terminal_editor(&tool, &args, terminal);
    }
    #[cfg(target_os = "macos")]
    let mut command = if jetbrains(editor).is_some() || editor == "android-studio" {
        // The IDE hands the file to its window when it is running already.
        let mut command = Command::new("open");
        command.arg("-na").arg(&tool).arg("--args");
        command
    } else {
        Command::new(&tool)
    };
    #[cfg(not(target_os = "macos"))]
    let mut command = Command::new(&tool);
    command.args(args);
    launch(command)
}

/// What the editor's command line tool is given to open `path` at `line`.
fn file_args(editor: &str, path: &Path, line: Option<u32>) -> Vec<OsString> {
    let at = || match line {
        Some(line) => {
            let mut target = path.as_os_str().to_owned();
            target.push(format!(":{line}"));
            target
        }
        None => path.as_os_str().to_owned(),
    };
    let flag_then_path = |flag: &str| match line {
        Some(line) => vec![flag.into(), line.to_string().into(), path.as_os_str().to_owned()],
        None => vec![path.as_os_str().to_owned()],
    };
    match editor {
        "vscode" | "cursor" => vec!["-g".into(), at()],
        "android-studio" => flag_then_path("--line"),
        "notepad++" | "vim" | "neovim" | "emacs" => {
            let mut args = Vec::new();
            if let Some(line) = line {
                let flag = if editor == "notepad++" { "-n" } else { "+" };
                args.push(format!("{flag}{line}").into());
            }
            args.push(path.as_os_str().to_owned());
            args
        }
        "visualstudio" => vec!["/Edit".into(), path.as_os_str().to_owned()],
        "eclipse" => vec!["--launcher.openFile".into(), path.as_os_str().to_owned()],
        "xcode" => flag_then_path("-l"),
        _ if jetbrains(editor).is_some() => flag_then_path("--line"),
        _ => vec![at()],
    }
}

fn terminal_editor(id: &str) -> bool {
    matches!(id, "vim" | "neovim")
}

#[cfg(target_os = "linux")]
fn terminal_editor_host(preferred: Option<&str>) -> Option<&str> {
    preferred
        .into_iter()
        .chain(["ghostty", "gnome-terminal", "konsole", "xterm"])
        .find(|id| matches!(*id, "ghostty" | "gnome-terminal" | "konsole" | "xterm") && terminal_installed(id))
}

fn open_terminal_editor(tool: &Path, args: &[OsString], terminal: Option<&str>) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        if terminal.is_none() || terminal == Some("wt") {
            if let (Some(wt), Some(terminal_args)) = (find_on_path("wt.exe"), windows_terminal_args(tool, args)) {
                let mut command = Command::new(wt);
                command.args(terminal_args);
                return launch(command);
            }
        }
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        let mut child = Command::new(tool)
            .args(args)
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn()
            .map_err(|err| err.to_string())?;
        std::thread::spawn(move || child.wait());
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        let terminal = terminal_editor_host(terminal).ok_or("No terminal found for this editor.")?;
        let mut command = Command::new(terminal);
        command.arg(if terminal == "gnome-terminal" { "--" } else { "-e" });
        command.arg(tool).args(args);
        launch(command)
    }
    #[cfg(target_os = "macos")]
    {
        if terminal == Some("ghostty") && terminal_installed("ghostty") {
            let mut command = Command::new("/usr/bin/open");
            command.args(["-na", "Ghostty", "--args", "-e"]).arg(tool).args(args);
            return launch(command);
        }

        let (host, script) = if terminal == Some("iterm") && terminal_installed("iterm") {
            (
                "iTerm",
                r#"on run argv
    set shellCommand to "/bin/sh -c " & quoted form of (item 1 of argv)
    tell application id "com.googlecode.iterm2"
        activate
        create window with default profile command shellCommand
    end tell
end run"#,
            )
        } else {
            (
                "Terminal",
                r#"on run argv
    tell application id "com.apple.Terminal"
        activate
        do script ("exec /bin/sh -c " & quoted form of (item 1 of argv))
    end tell
end run"#,
            )
        };
        let output = Command::new("/usr/bin/osascript")
            .args(["-e", script, "--"])
            .arg(terminal_shell_script(tool, args))
            .output()
            .map_err(|err| err.to_string())?;
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Could not open the editor in {host}. {}", error.trim()));
        }
        Ok(())
    }
}

#[cfg(any(windows, test))]
fn windows_terminal_args(tool: &Path, args: &[OsString]) -> Option<Vec<String>> {
    let mut terminal_args = vec!["new-tab".into()];
    for arg in std::iter::once(tool.as_os_str()).chain(args.iter().map(OsString::as_os_str)) {
        let arg = arg.to_str()?;
        // Windows Terminal treats semicolons as commands even inside quoted arguments.
        terminal_args.push(arg.replace(';', r"\;"));
    }
    Some(terminal_args)
}

#[cfg(target_os = "macos")]
fn terminal_shell_script(tool: &Path, args: &[OsString]) -> String {
    use std::fmt::Write;
    use std::os::unix::ffi::OsStrExt;

    let mut script = String::from("set --; ");
    for argument in std::iter::once(tool.as_os_str()).chain(args.iter().map(OsString::as_os_str)) {
        // Encode control bytes before passing the command to Terminal's interactive shell.
        script.push_str("arg=$(/usr/bin/printf '%b.' '");
        for byte in argument.as_bytes() {
            write!(&mut script, "\\0{byte:03o}").unwrap();
        }
        script.push_str(r#"'); set -- "$@" "${arg%.}"; "#);
    }
    script.push_str(r#"exec "$@""#);
    script
}

/// Open a terminal in `dir`.
pub fn open_terminal(terminal: &str, dir: &Path) -> Result<(), String> {
    let mut command;
    #[cfg(target_os = "macos")]
    {
        command = Command::new("open");
        match terminal {
            "ghostty" => command
                .args(["-na", "Ghostty", "--args"])
                .arg(format!("--working-directory={}", dir.display())),
            "iterm" => command.args(["-a", "iTerm"]).arg(dir),
            _ => command.args(["-a", "Terminal"]).arg(dir),
        };
    }
    #[cfg(not(target_os = "macos"))]
    {
        command = Command::new(terminal);
        match terminal {
            "ghostty" => command.arg(format!("--working-directory={}", dir.display())),
            "gnome-terminal" => command.arg(format!("--working-directory={}", dir.display())),
            "konsole" => command.arg("--workdir").arg(dir),
            "wt" => command.arg("-d").arg(dir),
            _ => &mut command,
        };
        command.current_dir(dir);
    }
    launch(command)
}

/// Show `path` selected in the system's file manager; on Linux, open its folder.
pub fn reveal(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let command = {
        let mut command = Command::new("open");
        command.arg("-R").arg(path);
        command
    };
    #[cfg(target_os = "windows")]
    let command = {
        let mut command = Command::new("explorer");
        command.arg(format!("/select,{}", path.display()));
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let command = {
        let mut command = Command::new("xdg-open");
        command.arg(path.parent().unwrap_or(path));
        command
    };
    launch(command)
}

/// Preview a file with Quick Look on macOS; elsewhere open it in its default app.
pub fn quick_look(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("qlmanage");
        command.arg("-p").arg(path);
        launch(command)
    }
    #[cfg(not(target_os = "macos"))]
    {
        launch(system_opener(path.as_os_str()))
    }
}

/// Open a web page in the default browser.
pub fn open_url(url: &str) -> Result<(), String> {
    launch(system_opener(OsStr::new(url)))
}

/// Open the file in the app associated with its type by the system.
pub fn open_default(path: &Path, window: crate::session::RepoRequest) -> Result<(), String> {
    let command = system_opener(path.as_os_str());
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    let _ = window;
    #[cfg(target_os = "macos")]
    return check_open_status(command);
    #[cfg(target_os = "windows")]
    return launch(command);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    launch_checked(command, move |error| window.progress("open-file-error", error))
}

fn system_opener(target: &OsStr) -> Command {
    #[cfg(target_os = "macos")]
    let command = {
        let mut command = Command::new("open");
        command.arg(target);
        command
    };
    #[cfg(target_os = "windows")]
    let command = {
        let mut command = Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler").arg(target);
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let command = {
        let mut command = Command::new("xdg-open");
        command.arg(target);
        command
    };
    command
}

#[cfg(target_os = "macos")]
fn check_open_status(mut command: Command) -> Result<(), String> {
    let child = spawn_quietly(&mut command).map_err(default_open_error)?;
    wait_for_open(child, command.get_program())
}

// xdg-open can stay alive until the opened application exits.
#[cfg(any(not(any(target_os = "macos", target_os = "windows")), all(test, unix)))]
fn launch_checked(mut command: Command, report_error: impl FnOnce(String) + Send + 'static) -> Result<(), String> {
    let child = spawn_quietly(&mut command).map_err(default_open_error)?;
    std::thread::spawn(move || {
        if let Err(error) = wait_for_open(child, command.get_program()) {
            report_error(error);
        }
    });
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn default_open_error(error: std::io::Error) -> String {
    format!("Could not open in the default app. {error}")
}

#[cfg(not(target_os = "windows"))]
fn wait_for_open(mut child: Child, program: &OsStr) -> Result<(), String> {
    let status = child.wait().map_err(default_open_error)?;
    if status.success() {
        return Ok(());
    }
    Err(format!(
        "Could not open in the default app. {} returned {status}.",
        program.to_string_lossy()
    ))
}

/// Start `command` and let it run on its own.
fn launch(mut command: Command) -> Result<(), String> {
    let mut child = spawn_quietly(&mut command).map_err(|err| err.to_string())?;
    // Reap it when it exits, so no zombie is left behind.
    std::thread::spawn(move || child.wait());
    Ok(())
}

fn spawn_quietly(command: &mut Command) -> std::io::Result<Child> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn default_app_reports_an_unsuccessful_launcher() {
        let mut command = Command::new("sh");
        command.args(["-c", "exit 3"]);
        assert_eq!(
            check_open_status(command).unwrap_err(),
            "Could not open in the default app. sh returned exit status: 3."
        );
        let mut command = Command::new("sh");
        command.args(["-c", "exit 0"]);
        assert_eq!(check_open_status(command), Ok(()));
    }

    #[cfg(unix)]
    #[test]
    fn background_open_returns_before_exit_and_reports_failure() {
        use std::sync::mpsc::{RecvTimeoutError, channel};
        use std::time::Duration;

        let (send, receive) = channel();
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 2"]);
        assert_eq!(launch_checked(command, move |err| send.send(err).unwrap()), Ok(()));
        assert_eq!(receive.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty));
        assert_eq!(
            receive.recv_timeout(Duration::from_secs(10)),
            Err(RecvTimeoutError::Disconnected)
        );

        let (send, receive) = channel();
        let mut command = Command::new("sh");
        command.args(["-c", "exit 4"]);
        assert_eq!(launch_checked(command, move |err| send.send(err).unwrap()), Ok(()));
        assert_eq!(
            receive.recv_timeout(Duration::from_secs(5)).unwrap(),
            "Could not open in the default app. sh returned exit status: 4."
        );

        let command = Command::new("./missing-oxbow-test-launcher");
        assert!(launch_checked(command, |_| panic!("spawn errors return to the caller")).is_err());
    }

    fn args(editor: &str, line: Option<u32>) -> Vec<String> {
        file_args(editor, Path::new("/repo/src/main.py"), line)
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn opening_a_working_file_requires_a_file_on_disk_inside_the_repo() {
        let root = std::env::current_dir().unwrap();
        assert_eq!(working_file(&root, "Cargo.toml"), Ok(root.join("Cargo.toml")));
        assert_eq!(
            working_file(&root, "missing-working-copy.png").unwrap_err(),
            "The working copy of missing-working-copy.png is missing from disk."
        );
        assert!(working_file(&root, "../Cargo.toml").is_err());
        assert!(working_file(&root, "src").is_err());
    }

    #[test]
    fn opening_uses_the_live_file_after_replacement_or_removal() {
        let root = std::env::temp_dir().join(format!("oxbow-working-file-test-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let path = "same name.txt";
        let file = root.join(path);
        std::fs::write(&file, "text").unwrap();
        assert_eq!(
            inspect_working_file(&root, path),
            Ok((file.clone(), WorkingFileStatus::Text))
        );
        std::fs::write(&file, b"binary\0").unwrap();
        assert_eq!(
            inspect_working_file(&root, path),
            Ok((file.clone(), WorkingFileStatus::Binary))
        );
        std::fs::remove_file(&file).unwrap();
        assert_eq!(
            inspect_working_file(&root, path).unwrap_err(),
            "The working copy of same name.txt is missing from disk."
        );
        std::fs::write(&file, "back as text").unwrap();
        assert_eq!(inspect_working_file(&root, path), Ok((file, WorkingFileStatus::Text)));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn each_editor_opens_the_file_at_the_line_its_own_way() {
        assert_eq!(args("vscode", Some(12)), ["-g", "/repo/src/main.py:12"]);
        assert_eq!(args("zed", Some(12)), ["/repo/src/main.py:12"]);
        assert_eq!(args("sublime", Some(12)), ["/repo/src/main.py:12"]);
        assert_eq!(args("xcode", Some(12)), ["-l", "12", "/repo/src/main.py"]);
        for ide in JETBRAINS {
            assert_eq!(
                args(ide.id, Some(12)),
                ["--line", "12", "/repo/src/main.py"],
                "{}",
                ide.id
            );
        }
    }

    #[test]
    fn newly_supported_editors_receive_their_own_file_arguments() {
        let path = "/repo with spaces/main.py";
        for (id, expected) in [
            ("android-studio", &["--line", "12", path][..]),
            ("cursor", &["-g", "/repo with spaces/main.py:12"][..]),
            ("eclipse", &["--launcher.openFile", path][..]),
            ("emacs", &["+12", path][..]),
            ("neovim", &["+12", path][..]),
            ("notepad++", &["-n12", path][..]),
            ("vim", &["+12", path][..]),
            ("visualstudio", &["/Edit", path][..]),
        ] {
            assert_eq!(file_args(id, Path::new(path), Some(12)), expected, "{id}");
        }
        assert_eq!(args("visualstudio", None), ["/Edit", "/repo/src/main.py"]);
        assert_eq!(args("neovim", None), ["/repo/src/main.py"]);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn vim_requires_an_explicit_selection_on_macos() {
        let vim = App { id: "vim", name: "Vim" };
        let cursor = App {
            id: "cursor",
            name: "Cursor",
        };
        assert_eq!(default_editor_id(std::slice::from_ref(&vim)), None);
        assert_eq!(default_editor_id(&[vim, cursor]), Some("cursor"));
    }

    #[test]
    fn windows_terminal_gets_one_command_even_for_paths_with_semicolons() {
        let tool = Path::new(r"C:\Tools\Vim; stable\vim.exe");
        let path = r"C:\repo\;notes ; new-tab.txt";
        assert_eq!(
            windows_terminal_args(tool, &["+12".into(), path.into()]).unwrap(),
            [
                "new-tab",
                r"C:\Tools\Vim\; stable\vim.exe",
                "+12",
                r"C:\repo\\;notes \; new-tab.txt"
            ]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn terminal_shell_script_preserves_argument_bytes_without_exposing_them_to_the_terminal() {
        use std::os::unix::ffi::OsStringExt;

        let raw = b"/repo with spaces/'$(exit 5)\x03\xff\n";
        let script = terminal_shell_script(
            Path::new("/usr/bin/printf"),
            &["%s".into(), OsString::from_vec(raw.to_vec())],
        );
        assert!(script.bytes().all(|byte| (b' '..=b'~').contains(&byte)));
        let output = Command::new("/bin/sh").args(["-c", &script]).output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, raw);
    }

    #[test]
    fn without_a_line_only_the_file_is_given() {
        assert_eq!(args("vscode", None), ["-g", "/repo/src/main.py"]);
        assert_eq!(args("xcode", None), ["/repo/src/main.py"]);
        assert_eq!(args("pycharm", None), ["/repo/src/main.py"]);
    }

    #[test]
    fn every_jetbrains_ide_is_listed_as_an_editor() {
        for ide in JETBRAINS {
            assert!(EDITORS.iter().any(|app| app.id == ide.id), "{}", ide.id);
            assert!(!ide.bundles.is_empty() && !ide.launchers.is_empty(), "{}", ide.id);
        }
    }
}
