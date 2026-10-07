//! Settings › Integrations › Open in: the editors and terminals installed on this computer,
//! and opening a file or the repository folder in them.

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

const EDITORS: [App; 4] = [
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
];

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

/// A macOS app bundle in /Applications or ~/Applications.
#[cfg(target_os = "macos")]
fn bundle(name: &str) -> Option<PathBuf> {
    let home = oxbow_core::config::home_dir().map(|home| home.join("Applications"));
    [
        Some(PathBuf::from("/Applications")),
        Some(PathBuf::from("/System/Applications/Utilities")),
        home,
    ]
    .into_iter()
    .flatten()
    .map(|dir| dir.join(format!("{name}.app")))
    .find(|path| path.is_dir())
}

/// The command line tool that opens a file at a line. On macOS it lives inside the app,
/// since apps started from the Dock do not see the shell's PATH.
fn editor_command(id: &str) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let inside = |app: &str, tool: &str| bundle(app).map(|path| path.join(tool)).filter(|path| path.exists());
        match id {
            "vscode" => inside("Visual Studio Code", "Contents/Resources/app/bin/code"),
            "zed" => inside("Zed", "Contents/MacOS/cli"),
            "sublime" => inside("Sublime Text", "Contents/SharedSupport/bin/subl"),
            "xcode" => bundle("Xcode").map(|_| PathBuf::from("/usr/bin/xed")),
            _ => None,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        match id {
            "vscode" => find_on_path("code").or_else(|| find_on_path("code.cmd")),
            "zed" => find_on_path("zed").or_else(|| find_on_path("zeditor")),
            "sublime" => find_on_path("subl"),
            _ => None,
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
    let at = |path: &Path| match line {
        Some(line) => format!("{}:{line}", path.display()),
        None => path.display().to_string(),
    };
    let mut command = Command::new(tool);
    match editor {
        "vscode" => command.arg("-g").arg(at(path)),
        "xcode" => match line {
            Some(line) => command.arg("-l").arg(line.to_string()).arg(path),
            None => command.arg(path),
        },
        _ => command.arg(at(path)),
    };
    launch(command)
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
