//! Settings › Integrations › Open in: the editors and terminals installed on this computer,
//! and opening a file or the repository folder in them.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(not(target_os = "macos"))]
use oxbow_core::config::find_on_path;
use serde::Serialize;

/// An editor or terminal that is installed.
#[derive(Debug, Clone, Serialize)]
pub struct App {
    pub id: &'static str,
    pub name: &'static str,
}

const EDITORS: [App; 13] = [
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
    EDITORS
        .into_iter()
        .filter(|app| editor_command(app.id).is_some())
        .collect()
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
fn editor_command(id: &str) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let inside = |app: &str, tool: &str| bundle(app).map(|path| path.join(tool)).filter(|path| path.exists());
        match id {
            "vscode" => inside("Visual Studio Code", "Contents/Resources/app/bin/code"),
            "zed" => inside("Zed", "Contents/MacOS/cli"),
            "sublime" => inside("Sublime Text", "Contents/SharedSupport/bin/subl"),
            "xcode" => bundle("Xcode").map(|_| PathBuf::from("/usr/bin/xed")),
            _ => jetbrains(id).and_then(|ide| ide.bundles.iter().find_map(|name| bundle(name))),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        match id {
            "vscode" => find_on_path("code").or_else(|| find_on_path("code.cmd")),
            "zed" => find_on_path("zed").or_else(|| find_on_path("zeditor")),
            "sublime" => find_on_path("subl"),
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

/// Open `path` in the editor, at `line` when the editor can.
pub fn open_file(editor: &str, path: &Path, line: Option<u32>) -> Result<(), String> {
    let tool = editor_command(editor).ok_or_else(|| format!("{editor} is not installed"))?;
    #[cfg(target_os = "macos")]
    let mut command = if jetbrains(editor).is_some() {
        // The IDE hands the file to its window when it is running already.
        let mut command = Command::new("open");
        command.arg("-na").arg(&tool).arg("--args");
        command
    } else {
        Command::new(&tool)
    };
    #[cfg(not(target_os = "macos"))]
    let mut command = Command::new(&tool);
    command.args(file_args(editor, path, line));
    launch(command)
}

/// What the editor's command line tool is given to open `path` at `line`.
fn file_args(editor: &str, path: &Path, line: Option<u32>) -> Vec<OsString> {
    let at = || match line {
        Some(line) => OsString::from(format!("{}:{line}", path.display())),
        None => path.as_os_str().to_owned(),
    };
    let flag_then_path = |flag: &str| match line {
        Some(line) => vec![flag.into(), line.to_string().into(), path.as_os_str().to_owned()],
        None => vec![path.as_os_str().to_owned()],
    };
    match editor {
        "vscode" => vec!["-g".into(), at()],
        "xcode" => flag_then_path("-l"),
        _ if jetbrains(editor).is_some() => flag_then_path("--line"),
        _ => vec![at()],
    }
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
        open_url(&path.display().to_string())
    }
}

/// Open a web page in the default browser.
pub fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let command = {
        let mut command = Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(target_os = "windows")]
    let command = {
        let mut command = Command::new("rundll32");
        command.args(["url.dll,FileProtocolHandler", url]);
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    launch(command)
}

/// Start `command` and let it run on its own.
fn launch(mut command: Command) -> Result<(), String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| err.to_string())?;
    // Reap it when it exits, so no zombie is left behind.
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(editor: &str, line: Option<u32>) -> Vec<String> {
        file_args(editor, Path::new("/repo/src/main.py"), line)
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
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
