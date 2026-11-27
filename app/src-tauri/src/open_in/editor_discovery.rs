use std::path::{Path, PathBuf};

#[cfg(any(windows, target_os = "linux", test))]
use super::jetbrains;

#[derive(Default)]
pub(super) struct EditorDiscovery {
    #[cfg(windows)]
    installs: std::cell::OnceCell<Vec<WindowsInstall>>,
}

impl EditorDiscovery {
    pub(super) fn find(&self, id: &str) -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        return registered_macos_app(id);
        #[cfg(target_os = "linux")]
        return linux_launcher(id);
        #[cfg(windows)]
        return match id {
            "visualstudio" => visual_studio_launcher(),
            "vim" | "neovim" => windows_default_launcher(id),
            _ => windows_launcher(id, self.installs.get_or_init(windows_installs))
                .or_else(|| windows_default_launcher(id))
                .or_else(|| windows_editor_from_shim(id)),
        };
        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        return None;
    }
}

#[cfg(target_os = "macos")]
fn registered_macos_app(id: &str) -> Option<PathBuf> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;

    let (bundle_ids, launcher): (&[&str], &str) = match id {
        "vscode" => (&["com.microsoft.VSCode"], "Contents/Resources/app/bin/code"),
        "zed" => (&["dev.zed.Zed"], "Contents/MacOS/cli"),
        "sublime" => (
            &["com.sublimetext.4", "com.sublimetext.3"],
            "Contents/SharedSupport/bin/subl",
        ),
        "xcode" => (&["com.apple.dt.Xcode"], "Contents/Developer/usr/bin/xed"),
        "android-studio" => (&["com.google.android.studio"], ""),
        "cursor" => (&["com.todesktop.230313mzl4w4u92"], "Contents/Resources/app/bin/cursor"),
        "eclipse" => (
            &[
                "org.eclipse.platform.ide",
                "org.eclipse.eclipse",
                "org.eclipse.epp.package.java",
                "org.eclipse.epp.package.jee",
                "org.eclipse.epp.package.cpp",
                "org.eclipse.epp.package.committers",
                "org.eclipse.epp.package.dsl",
                "org.eclipse.epp.package.embedcpp",
                "org.eclipse.epp.package.modeling",
                "org.eclipse.epp.package.php",
                "org.eclipse.epp.package.rcp",
                "org.eclipse.epp.package.scout",
            ],
            "Contents/MacOS/eclipse",
        ),
        "emacs" => (&["org.gnu.Emacs", "org.gnu.emacs"], "Contents/MacOS/Emacs"),
        "vim" => return launcher_in(Path::new("/usr/bin"), &["vim"]),
        "neovim" => {
            return ["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"]
                .into_iter()
                .find_map(|dir| launcher_in(Path::new(dir), &["nvim"]));
        }
        "pycharm" => (&["com.jetbrains.PyCharm", "com.jetbrains.pycharm.ce"], ""),
        "idea" => (&["com.jetbrains.intellij", "com.jetbrains.intellij.ce"], ""),
        "webstorm" => (&["com.jetbrains.WebStorm"], ""),
        "goland" => (&["com.jetbrains.goland"], ""),
        "phpstorm" => (&["com.jetbrains.PhpStorm"], ""),
        "rubymine" => (&["com.jetbrains.RubyMine"], ""),
        "rider" => (&["com.jetbrains.rider"], ""),
        "clion" => (&["com.jetbrains.CLion"], ""),
        "rustrover" => (&["com.jetbrains.RustRover"], ""),
        _ => return None,
    };
    let workspace = NSWorkspace::sharedWorkspace();
    bundle_ids.iter().find_map(|id| {
        let app = workspace
            .URLForApplicationWithBundleIdentifier(&NSString::from_str(id))
            .or_else(|| {
                id.starts_with("org.eclipse.epp.package.")
                    .then(|| {
                        workspace.URLForApplicationWithBundleIdentifier(&NSString::from_str(&format!("{id}.product")))
                    })
                    .flatten()
            })?
            .to_file_path()?;
        if launcher.is_empty() {
            return app.is_dir().then_some(app);
        }
        launcher_in(&app, &[launcher])
    })
}

#[cfg(any(windows, target_os = "linux"))]
fn absolute_env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

#[cfg(target_os = "linux")]
fn linux_launcher(id: &str) -> Option<PathBuf> {
    use oxbow_core::config::home_dir;

    let home = home_dir().filter(|path| path.is_absolute());
    let local_bin = home.as_ref().map(|home| home.join(".local/bin"));
    let data = absolute_env_path("XDG_DATA_HOME").or_else(|| home.as_ref().map(|home| home.join(".local/share")));
    let toolbox = data.map(|data| data.join("JetBrains/Toolbox"));
    let names: &[&str] = match id {
        "vscode" => &["code"],
        "zed" => &["zed", "zeditor"],
        "sublime" => &["subl"],
        "android-studio" => &["android-studio", "studio", "studio.sh"],
        "cursor" => &["cursor"],
        "eclipse" => &["eclipse"],
        "emacs" => &["emacs-gtk", "emacs-pgtk", "emacs-lucid", "emacs"],
        "vim" => &["vim"],
        "neovim" => &["nvim"],
        _ => jetbrains(id)?.launchers,
    };
    let toolbox_scripts = jetbrains(id).and(toolbox.as_ref()).map(|dir| dir.join("scripts"));
    [
        toolbox_scripts,
        local_bin,
        Some(PathBuf::from("/usr/local/bin")),
        Some(PathBuf::from("/usr/bin")),
        Some(PathBuf::from("/snap/bin")),
    ]
    .into_iter()
    .flatten()
    .find_map(|dir| {
        names
            .iter()
            .find_map(|name| launcher_in(&dir, &[name]).filter(|path| id != "emacs" || graphical_emacs(path)))
    })
    .or_else(|| match id {
        "vscode" => launcher_in(Path::new("/usr/share/code"), &["bin/code"]),
        "zed" => launcher_in(&home?.join(".local/zed.app"), &["bin/zed"]),
        "sublime" => launcher_in(Path::new("/opt/sublime_text"), &["sublime_text"]),
        "android-studio" => installed_launcher(&["android-studio/bin/studio", "android-studio/bin/studio.sh"]),
        "eclipse" => installed_launcher(&["eclipse/eclipse"])
            .or_else(|| launcher_in_installations(&home?.join("eclipse"), "eclipse/eclipse")),
        "neovim" => installed_launcher(&[
            "nvim/bin/nvim",
            "nvim-linux-x86_64/bin/nvim",
            "nvim-linux-arm64/bin/nvim",
        ]),
        _ if jetbrains(id).is_some() => {
            let script = format!("bin/{id}.sh");
            toolbox
                .and_then(|dir| toolbox_launcher(&dir.join("apps"), &script))
                .or_else(|| {
                    ["/opt", "/usr/local"]
                        .into_iter()
                        .find_map(|dir| launcher_in_installations(Path::new(dir), &script))
                })
        }
        _ => None,
    })
}

#[cfg(target_os = "linux")]
fn installed_launcher(names: &[&str]) -> Option<PathBuf> {
    ["/opt", "/usr/local"]
        .into_iter()
        .find_map(|dir| launcher_in(Path::new(dir), names))
}

#[cfg(target_os = "linux")]
pub(super) fn graphical_emacs(path: &Path) -> bool {
    path.canonicalize().ok().is_some_and(|resolved| {
        resolved
            .file_name()
            .is_some_and(|name| !name.to_string_lossy().contains("nox"))
    })
}

#[cfg(any(windows, target_os = "linux", all(test, unix)))]
fn launcher_in_installations(root: &Path, script: &str) -> Option<PathBuf> {
    installation_entries(root)
        .iter()
        .find_map(|dir| launcher_in(dir, &[script]))
}

#[cfg(any(windows, target_os = "linux", all(test, unix)))]
fn installation_entries(root: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .collect();
    entries.sort_unstable_by(|a, b| b.cmp(a));
    entries
}

#[cfg(any(target_os = "linux", all(test, unix)))]
fn toolbox_launcher(root: &Path, script: &str) -> Option<PathBuf> {
    launcher_in_installations(root, script).or_else(|| {
        installation_entries(root).iter().find_map(|product| {
            launcher_in_installations(&product.join("ch-0"), script).or_else(|| {
                installation_entries(product)
                    .iter()
                    .filter(|channel| {
                        channel
                            .file_name()
                            .is_some_and(|name| name != "ch-0" && name.to_string_lossy().starts_with("ch-"))
                    })
                    .find_map(|channel| launcher_in_installations(channel, script))
            })
        })
    })
}

#[cfg(any(windows, test))]
struct WindowsInstall {
    name: String,
    publisher: String,
    root: PathBuf,
    version: Option<String>,
}

#[cfg(any(windows, test))]
fn windows_launcher(id: &str, installs: &[WindowsInstall]) -> Option<PathBuf> {
    let (product, publisher, launchers): (&str, &str, &[&str]) = match id {
        "vscode" => ("Microsoft Visual Studio Code", "Microsoft Corporation", &["Code.exe"]),
        "zed" => ("Zed", "Zed Industries", &["bin/zed.exe"]),
        "sublime" => ("Sublime Text", "Sublime HQ Pty Ltd", &["subl.exe"]),
        "android-studio" => ("Android Studio", "Google LLC", &["bin/studio64.exe"]),
        "cursor" => ("Cursor", "Cursor AI, Inc.", &["Cursor.exe"]),
        "notepad++" => ("Notepad++", "Notepad++ Team", &["notepad++.exe"]),
        _ => (jetbrains(id)?.bundles[0], "JetBrains s.r.o.", &[]),
    };
    let ide_launcher = format!("bin/{id}64.exe");
    let launchers = if launchers.is_empty() {
        &[ide_launcher.as_str()][..]
    } else {
        launchers
    };
    let matching = installs
        .iter()
        .filter(|install| {
            if install.publisher != publisher
                && !(id == "cursor" && install.publisher == "Anysphere")
                && !(id == "android-studio" && install.publisher == "Google Inc.")
            {
                return false;
            }
            let name = install
                .name
                .strip_prefix("JetBrains Toolbox (")
                .or_else(|| install.name.strip_prefix("JetBrains "))
                .unwrap_or(&install.name);
            name.strip_prefix(product).is_some_and(|suffix| match id {
                "vscode" => matches!(suffix, "" | " (User)" | " (System)"),
                "zed" => suffix.is_empty() || suffix.trim_start().starts_with(|c: char| c.is_ascii_digit()),
                _ => suffix.is_empty() || suffix.starts_with([' ', '(', ')']),
            })
        })
        .filter_map(|install| launcher_in(&install.root, launchers).map(|path| (install, path)));
    if jetbrains(id).is_some() {
        matching
            .min_by_key(|(install, path)| {
                (
                    install.name.contains("Community") || install.name.split_whitespace().any(|word| word == "CE"),
                    std::cmp::Reverse(jetbrains_version(install)),
                    path.clone(),
                )
            })
            .map(|(_, path)| path)
    } else {
        matching.map(|(_, path)| path).next()
    }
}

#[cfg(any(windows, test))]
fn jetbrains_version(install: &WindowsInstall) -> ([u32; 3], Vec<u32>) {
    let info = std::fs::read(install.root.join("product-info.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let version = install.version.as_deref();
    let release = [
        info.as_ref().and_then(|info| info["version"].as_str()),
        version,
        Some(install.name.as_str()),
    ]
    .into_iter()
    .flatten()
    .flat_map(jetbrains_version_numbers)
    .find_map(|parts| match parts.as_slice() {
        [year @ 2000..=2099, release, rest @ ..] => Some([*year, *release, rest.first().copied().unwrap_or(0)]),
        _ => None,
    });
    let build = [info.as_ref().and_then(|info| info["buildNumber"].as_str()), version]
        .into_iter()
        .flatten()
        .flat_map(jetbrains_version_numbers)
        .find(|parts| matches!(parts[0], 100..=999))
        .unwrap_or_default();
    let release = release.unwrap_or_else(|| {
        // JetBrains encodes YYYY.R as YYR in the first build component.
        build
            .first()
            .map_or([0; 3], |branch| [2000 + branch / 10, branch % 10, 0])
    });
    (release, build)
}

#[cfg(any(windows, test))]
fn jetbrains_version_numbers(value: &str) -> impl Iterator<Item = Vec<u32>> + '_ {
    value
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .filter_map(|part| {
            let parts = part
                .trim_matches('.')
                .split('.')
                .map(str::parse)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            (parts.len() >= 2).then_some(parts)
        })
}

#[cfg(windows)]
fn windows_default_launcher(id: &str) -> Option<PathBuf> {
    let paths: &[&str] = match id {
        "vscode" => &["Microsoft VS Code/Code.exe"],
        "zed" => &["Zed/bin/zed.exe"],
        "sublime" => &["Sublime Text/subl.exe", "Sublime Text 3/subl.exe"],
        "android-studio" => &[
            "Android/Android Studio/bin/studio64.exe",
            "Android Studio/bin/studio64.exe",
        ],
        "cursor" => &["Cursor/Cursor.exe"],
        "notepad++" => &["Notepad++/notepad++.exe"],
        "neovim" => &["Neovim/bin/nvim.exe"],
        "eclipse" => {
            let root = oxbow_core::config::home_dir()?.join("eclipse");
            return launcher_in(&root, &["eclipse.exe"])
                .or_else(|| launcher_in_installations(&root, "eclipse/eclipse.exe"));
        }
        "vim" | "emacs" => {
            let (dir, launcher) = if id == "vim" {
                ("Vim", "vim.exe")
            } else {
                ("Emacs", "bin/runemacs.exe")
            };
            return ["ProgramFiles", "ProgramFiles(x86)"]
                .into_iter()
                .filter_map(absolute_env_path)
                .find_map(|root| launcher_in_installations(&root.join(dir), launcher));
        }
        _ => {
            let ide = jetbrains(id)?;
            let scripts = absolute_env_path("LOCALAPPDATA")?.join("JetBrains/Toolbox/scripts");
            return launcher_in(&scripts, &[&format!("{}.exe", ide.id), &format!("{}.cmd", ide.id)]);
        }
    };
    [
        absolute_env_path("LOCALAPPDATA").map(|local| local.join("Programs")),
        absolute_env_path("ProgramFiles"),
        absolute_env_path("ProgramFiles(x86)"),
    ]
    .into_iter()
    .flatten()
    .find_map(|dir| launcher_in(&dir, paths))
}

#[cfg(windows)]
fn windows_editor_from_shim(id: &str) -> Option<PathBuf> {
    let shim = match id {
        "vscode" => "code.cmd",
        "cursor" => "cursor.cmd",
        _ => return None,
    };
    let shim = oxbow_core::config::find_on_path(shim)?;
    editor_exe_next_to_install_shim(id, &shim)
}

#[cfg(any(windows, test))]
fn editor_exe_next_to_install_shim(id: &str, shim: &Path) -> Option<PathBuf> {
    let (levels, executable) = match id {
        "vscode" => (2, "Code.exe"),
        "cursor" => (4, "Cursor.exe"),
        _ => return None,
    };
    launcher_in(shim.ancestors().nth(levels)?, &[executable])
}

#[cfg(windows)]
fn visual_studio_launcher() -> Option<PathBuf> {
    use std::os::windows::process::CommandExt;

    let vswhere = ["ProgramFiles(x86)", "ProgramFiles"]
        .into_iter()
        .filter_map(absolute_env_path)
        .find_map(|root| launcher_in(&root, &["Microsoft Visual Studio/Installer/vswhere.exe"]))?;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new(vswhere)
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-prerelease", "-utf8", "-find", r"Common7\IDE\devenv.exe"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    visual_studio_path(std::str::from_utf8(&output.stdout).ok()?)
}

#[cfg(any(windows, test))]
fn visual_studio_path(output: &str) -> Option<PathBuf> {
    output
        .trim_start_matches('\u{feff}')
        .lines()
        .map(PathBuf::from)
        .find(|path| path.is_absolute() && path.is_file())
}

#[cfg(windows)]
fn windows_installs() -> Vec<WindowsInstall> {
    use std::ffi::OsString;
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};

    let mut installs = Vec::new();
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let hive = RegKey::predef(hive);
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            let Ok(uninstall) =
                hive.open_subkey_with_flags(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall", KEY_READ | view)
            else {
                continue;
            };
            for subkey in uninstall.enum_keys().flatten() {
                let Ok(key) = uninstall.open_subkey_with_flags(subkey, KEY_READ | view) else {
                    continue;
                };
                let (Ok(name), Ok(publisher)) = (key.get_value("DisplayName"), key.get_value("Publisher")) else {
                    continue;
                };
                let root = key
                    .get_value::<OsString, _>("InstallLocation")
                    .ok()
                    .map(PathBuf::from)
                    .filter(|dir| dir.is_absolute())
                    .or_else(|| {
                        // Toolbox can register only the launcher path.
                        let icon: String = key.get_value("DisplayIcon").ok()?;
                        let icon = icon.strip_suffix(",0").unwrap_or(&icon).trim_matches('"');
                        let exe = Path::new(icon);
                        let dir = exe.parent()?;
                        Some(if dir.ends_with("bin") { dir.parent()? } else { dir }.to_path_buf())
                    });
                if let Some(root) = root {
                    installs.push(WindowsInstall {
                        name,
                        publisher,
                        root,
                        version: key.get_value("DisplayVersion").ok(),
                    });
                }
            }
        }
    }
    installs
}

fn launcher_in(root: &Path, names: &[&str]) -> Option<PathBuf> {
    if !root.is_absolute() {
        return None;
    }
    names.iter().map(|name| root.join(name)).find(|path| {
        let Ok(metadata) = path.metadata() else {
            return false;
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
        }
        #[cfg(not(unix))]
        metadata.is_file()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_launcher(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, []).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn windows_uses_matching_installers_with_existing_launchers() {
        let temp = oxbow_core::cli::private_temp_folder("oxbow-editors-").unwrap();
        for (id, name, publisher, relative) in [
            ("sublime", "Sublime Text 3", "Sublime HQ Pty Ltd", "subl.exe"),
            (
                "vscode",
                "Microsoft Visual Studio Code (User)",
                "Microsoft Corporation",
                "Code.exe",
            ),
            ("zed", "Zed", "Zed Industries", "bin/zed.exe"),
            ("android-studio", "Android Studio", "Google LLC", "bin/studio64.exe"),
            ("cursor", "Cursor (User)", "Anysphere", "Cursor.exe"),
            ("notepad++", "Notepad++ (64-bit x64)", "Notepad++ Team", "notepad++.exe"),
            (
                "idea",
                "JetBrains Toolbox (IntelliJ IDEA Ultimate)",
                "JetBrains s.r.o.",
                "bin/idea64.exe",
            ),
        ] {
            let root = temp.path().join(id);
            let launcher = root.join(relative);
            write_launcher(&launcher);
            let installs = [
                WindowsInstall {
                    name: name.into(),
                    publisher: "Other publisher".into(),
                    root: root.clone(),
                    version: None,
                },
                WindowsInstall {
                    name: name.into(),
                    publisher: publisher.into(),
                    root: root.join("missing"),
                    version: None,
                },
                WindowsInstall {
                    name: name.into(),
                    publisher: publisher.into(),
                    root,
                    version: None,
                },
            ];
            assert_eq!(windows_launcher(id, &installs[..2]), None);
            assert_eq!(windows_launcher(id, &installs), Some(launcher));
        }
    }

    #[test]
    fn launchers_require_an_absolute_path_to_a_file() {
        let temp = oxbow_core::cli::private_temp_folder("oxbow-editors-").unwrap();
        std::fs::create_dir(temp.path().join("subl.exe")).unwrap();
        assert_eq!(launcher_in(temp.path(), &["subl.exe"]), None);
        assert_eq!(launcher_in(Path::new(""), &["subl.exe"]), None);
    }

    #[test]
    fn windows_jetbrains_prefers_the_full_edition_when_both_are_installed() {
        let temp = oxbow_core::cli::private_temp_folder("oxbow-editors-").unwrap();
        let installs = [
            "IntelliJ IDEA Community Edition 2026.1",
            "IntelliJ IDEA Ultimate 2025.1",
        ]
        .map(|name| {
            let root = temp.path().join(name);
            write_launcher(&root.join("bin/idea64.exe"));
            WindowsInstall {
                name: name.into(),
                publisher: "JetBrains s.r.o.".into(),
                root,
                version: None,
            }
        });
        let community = installs[0].root.join("bin/idea64.exe");
        let ultimate = installs[1].root.join("bin/idea64.exe");
        assert_eq!(windows_launcher("idea", &installs[..1]), Some(community));
        assert_eq!(windows_launcher("idea", &installs), Some(ultimate));
    }

    #[test]
    fn windows_jetbrains_compares_releases_and_builds_independently_of_the_path() {
        let temp = oxbow_core::cli::private_temp_folder("oxbow-editors-").unwrap();
        let installs = [
            ("A/2024.3", "IntelliJ IDEA Ultimate 2024.3", Some("2024.3")),
            ("D/251.200", "IntelliJ IDEA Ultimate", Some("IU-251.200.9")),
            ("Z/251.1100", "IntelliJ IDEA Ultimate", Some("IU-251.1100.1")),
        ]
        .map(|(folder, name, version)| {
            let root = temp.path().join(folder);
            write_launcher(&root.join("bin/idea64.exe"));
            WindowsInstall {
                name: name.into(),
                publisher: "JetBrains s.r.o.".into(),
                root,
                version: version.map(str::to_owned),
            }
        });
        assert_eq!(
            windows_launcher("idea", &installs),
            Some(installs[2].root.join("bin/idea64.exe"))
        );
    }

    #[test]
    fn windows_jetbrains_reads_product_version_when_toolbox_omits_it_from_the_registry() {
        let temp = oxbow_core::cli::private_temp_folder("oxbow-editors-").unwrap();
        let installs = [("A/2025.1.9", "2025.1.9"), ("Z/custom", "2025.1.10")].map(|(folder, version)| {
            let root = temp.path().join(folder);
            write_launcher(&root.join("bin/idea64.exe"));
            std::fs::write(
                root.join("product-info.json"),
                serde_json::json!({ "version": version }).to_string(),
            )
            .unwrap();
            WindowsInstall {
                name: "JetBrains Toolbox (IntelliJ IDEA Ultimate)".into(),
                publisher: "JetBrains s.r.o.".into(),
                root,
                version: None,
            }
        });
        assert_eq!(
            windows_launcher("idea", &installs),
            Some(installs[1].root.join("bin/idea64.exe"))
        );
    }

    #[test]
    fn visual_studio_skips_stale_executable_paths() {
        let temp = oxbow_core::cli::private_temp_folder("oxbow-editors-").unwrap();
        let stale = temp.path().join("Removed/Common7/IDE/devenv.exe");
        let ide = temp.path().join("Visual Studio/Common7/IDE/devenv.exe");
        write_launcher(&ide);
        let output = format!("\u{feff}{}\r\n{}\r\n", stale.display(), ide.display());
        assert_eq!(visual_studio_path(&output), Some(ide));
    }

    #[test]
    fn windows_editor_shims_resolve_to_native_executables() {
        let temp = oxbow_core::cli::private_temp_folder("oxbow-editors-").unwrap();
        for (id, shim, executable) in [
            ("vscode", "bin/code.cmd", "Code.exe"),
            ("cursor", "resources/app/bin/cursor.cmd", "Cursor.exe"),
        ] {
            let root = temp.path().join(id);
            let shim = root.join(shim);
            let native = root.join(executable);
            write_launcher(&shim);
            assert_eq!(editor_exe_next_to_install_shim(id, &shim), None);
            write_launcher(&native);
            assert_eq!(editor_exe_next_to_install_shim(id, &shim), Some(native));
        }
    }

    #[cfg(unix)]
    #[test]
    fn linux_toolbox_finds_legacy_channel_installations_without_scripts() {
        let temp = oxbow_core::cli::private_temp_folder("oxbow-editors-").unwrap();
        let launcher = temp.path().join("IDEA-U/ch-0/251.123/bin/idea.sh");
        std::fs::create_dir_all(temp.path().join("IDEA-U/ch-0/252.123/bin")).unwrap();
        write_launcher(&launcher);
        write_launcher(&temp.path().join("IDEA-U/ch-1/253.123/bin/idea.sh"));
        assert_eq!(toolbox_launcher(temp.path(), "bin/idea.sh"), Some(launcher));
    }

    #[cfg(unix)]
    #[test]
    fn linux_toolbox_skips_installations_with_missing_or_non_executable_scripts() {
        let temp = oxbow_core::cli::private_temp_folder("oxbow-editors-").unwrap();
        let launcher = temp.path().join("IDEA/bin/idea.sh");
        write_launcher(&launcher);
        let stale = temp.path().join("IDEA-stale/bin/idea.sh");
        std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
        std::fs::write(stale, []).unwrap();
        assert_eq!(launcher_in_installations(temp.path(), "bin/idea.sh"), Some(launcher));
    }
}
