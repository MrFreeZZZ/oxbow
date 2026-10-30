//! Personal commands are resolved from settings at invocation, then bound to this repository.
use crate::{
    CommandResult, preferences,
    session::{Session, at, current, expected, logged},
    turns::Running,
};
use oxbow_core::{Error, console, user_commands::UserCommand};
use tauri::{AppHandle, State};

#[tauri::command]
pub(crate) async fn run_user_command(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    id: String,
    repository: String,
    branch: String,
    commit: String,
) -> CommandResult<()> {
    let repo = expected(current(&session, &window)?, Some(&repository))?;
    let commands: Vec<UserCommand> = serde_json::from_value(
        preferences::read_settings(&app)
            .remove("oxbow.commands")
            .unwrap_or(serde_json::json!([])),
    )
    .map_err(|e| e.to_string())?;
    let command = commands
        .into_iter()
        .find(|c| c.id == id)
        .ok_or("This command no longer exists.")?;
    let stop = session.new_stop(&window);
    let turns = session.turns.clone();
    let change = Running::new(window.label(), repository, command.name.clone());
    logged(
        Some(at(&repo)),
        console::EntryKind::Action,
        format!("Custom: {}", console::mask(&command.name)),
        move || {
            let _turn = turns
                .take(&repo.common_dir(), change, &stop.cancel, |_| {})
                .ok_or(Error::Cancelled)?;
            repo.run_user_command(&command, &branch, &commit, &stop.cancel)
                .map_err(|err| match err {
                    Error::Command { code, .. } => Error::Git(format!(
                        "Command exited with {}. See Console for output.",
                        code.map_or("no exit code".into(), |c| format!("code {c}"))
                    )),
                    other => other,
                })
        },
    )
    .await
    .map_err(|e| console::mask(&e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{session::Slot, settings};
    use std::sync::atomic::AtomicBool;

    #[test]
    fn a_captured_command_stays_with_its_repository_after_switch_or_close() {
        let dir = std::env::temp_dir().join(format!("oxbow-command-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for name in ["a", "b"] {
            let path = dir.join(name);
            std::fs::create_dir_all(&path).unwrap();
            assert!(
                std::process::Command::new("git")
                    .args(["init", "-q"])
                    .current_dir(&path)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let a = oxbow_core::Repo::open(dir.join("a")).unwrap();
        let b = oxbow_core::Repo::open(dir.join("b")).unwrap();
        let mut slot = Slot::default();
        slot.open(a);
        let captured = slot.repo.clone().unwrap();
        slot.open(b);
        slot.close();
        let command = UserCommand {
            id: "test".into(),
            name: "Test".into(),
            script: "echo ran > result.txt".into(),
            repository: String::new(),
        };
        captured
            .run_user_command(&command, "", "", &AtomicBool::new(false))
            .unwrap();
        assert!(dir.join("a/result.txt").exists());
        assert!(!dir.join("b/result.txt").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn personal_commands_survive_settings_reload_with_other_preferences() {
        let dir = std::env::temp_dir().join(format!("oxbow-command-settings-{}", std::process::id()));
        let file = dir.join("settings.json");
        let mut values = serde_json::Map::new();
        values.insert("oxbow.diff.wrap".into(), serde_json::json!(false));
        values.insert(
            "oxbow.commands".into(),
            serde_json::json!([{
                "id": "test", "name": "Tests", "script": "cargo test", "repository": "/a path"
            }]),
        );
        settings::write(&file, &values).unwrap();
        let loaded = settings::read(&file).unwrap();
        let commands: Vec<UserCommand> = serde_json::from_value(loaded["oxbow.commands"].clone()).unwrap();
        assert_eq!(commands[0].repository, "/a path");
        assert_eq!(loaded["oxbow.diff.wrap"], false);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
