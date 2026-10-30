// Hide the extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts;
mod actions;
mod conflicts;
mod console_window;
mod drafts;
mod git_config;
mod gitlab_accounts;
mod history;
mod menu;
mod open_in;
mod outside;
mod overview;
mod preferences;
mod pulls;
mod recent;
mod repo_files;
mod repos;
mod session;
mod settings;
mod setup;
mod trusted;
mod turns;
mod user_commands;
mod windows;
mod working;

use tauri::{Emitter, Manager};

use crate::preferences::{apply_settings, read_settings, show_settings};
use crate::session::Session;

type CommandResult<T> = Result<T, String>;

fn main() {
    let builder = tauri::Builder::default();
    // Other systems have no menu bar; the window's own ⌘, / Ctrl+, opens Settings there.
    #[cfg(target_os = "macos")]
    let builder = builder.menu(windows::app_menu);
    builder
        .plugin(tauri_plugin_dialog::init())
        // Restores the window's size and position from the last session.
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .manage(Session::default())
        .manage(overview::OverviewRuns::default())
        .manage(menu::MenuState::default())
        .manage(accounts::Accounts::default())
        .manage(gitlab_accounts::GitLabTokens::default())
        .on_window_event(|window, event| {
            let session = window.state::<Session>();
            let label = window.label().to_owned();
            match event {
                // Settings shows and changes the repository of the window used last.
                tauri::WindowEvent::Focused(true) if label != "settings" && label != "console" => {
                    let changed = session.focused.lock().expect("session lock").replace(label.clone()) != Some(label);
                    if changed {
                        let _ = window.emit_to("settings", "repo-changed", ());
                        let _ = window.emit_to("console", "repo-changed", ());
                    }
                }
                tauri::WindowEvent::Destroyed => {
                    window.state::<overview::OverviewRuns>().stop(&label, None);
                    // Its reads stop with it.
                    if let Some(mut slot) = session.windows.lock().expect("session lock").remove(&label) {
                        slot.stop_reads();
                    }
                    session.pending.lock().expect("session lock").remove(&label);
                    let mut focused = session.focused.lock().expect("session lock");
                    if focused.as_deref() == Some(label.as_str()) {
                        *focused = None;
                    }
                }
                _ => {}
            }
        })
        .on_menu_event(|app, event| {
            if event.id() == "settings" {
                let _ = show_settings(app);
            }
            let focused = app.state::<Session>().focused.lock().expect("session lock").clone();
            menu::picked(app, event.id().as_ref(), focused);
            #[cfg(target_os = "macos")]
            if event.id() == "quit" {
                windows::quit(app);
            }
        })
        .setup(|app| {
            // Before anything runs git: it must find git-lfs where Homebrew put it.
            oxbow_core::lfs::extend_path();
            let handle = app.handle();
            apply_settings(handle, &handle.state::<Session>(), &read_settings(handle));
            accounts::lend_to_git(handle);
            gitlab_accounts::lend_to_git(handle);
            console_window::follow(handle);
            menu::recent_changed(handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            preferences::display_options,
            repos::open_repo,
            repos::close_repo,
            repos::trust_repo,
            trusted::trusted_repos,
            trusted::untrust_repo,
            repos::initial_repo,
            recent::recent_repos,
            overview::repository_overview,
            overview::cancel_repository_overview,
            recent::forget_repo,
            repos::gone_folder,
            repos::reconnect_folder,
            recent::clear_recent,
            setup::welcome_info,
            setup::folder_state,
            setup::probe_remote,
            setup::clone_command,
            setup::clone_repo,
            setup::plan_new_repo,
            setup::create_repo,
            history::history,
            history::commit_detail,
            history::commit_diff,
            history::commit_diff_page,
            session::stop_reads,
            history::folder_states,
            history::settle_unpushed,
            history::compare,
            history::compare_with_working_copy,
            history::compare_diff,
            history::compare_diff_page,
            history::compare_counts,
            history::file_history,
            history::blame,
            history::line_history,
            history::file_pickaxe,
            history::local_history,
            history::local_history_all,
            history::local_history_problem,
            history::local_version_diff,
            history::save_local_version,
            history::files,
            history::tree_at,
            history::files_at,
            history::file_at,
            history::save_file_at,
            working::working_tree,
            working::working_counts,
            working::recent_messages,
            working::working_diff,
            working::working_diff_page,
            working::changelists,
            working::edit_changelists,
            working::source_bytes,
            working::source_lines,
            working::open_source,
            working::deletion_check,
            working::remote_deletion_check,
            working::merge_preview,
            working::patch_preview,
            working::patch_range,
            conflicts::conflict_file,
            conflicts::merge_tool,
            conflicts::open_merge_tool,
            conflicts::conflict_draft,
            conflicts::save_conflict_draft,
            conflicts::drop_conflict_draft,
            windows::cancel_quit,
            working::stash_check,
            history::stack,
            history::restack,
            history::save_restack,
            history::landed_on,
            history::stack_preview,
            history::split_files,
            actions::plan_action,
            actions::perform_action,
            session::stop_action,
            preferences::get_setting,
            preferences::all_settings,
            preferences::set_setting,
            preferences::open_settings,
            preferences::set_settings_key,
            menu::set_menu,
            git_config::git_settings,
            git_config::repo_settings,
            git_config::set_git_config,
            git_config::set_commit_checks,
            user_commands::run_user_command,
            repo_files::repo_files,
            repo_files::repo_file_text,
            repo_files::check_repo_file,
            repo_files::save_repo_file,
            actions::fetch_in_background,
            actions::stop_fetch,
            actions::public_key,
            actions::pull_setup,
            outside::open_in_editor,
            outside::open_in_terminal,
            outside::reveal_worktree,
            windows::open_window,
            working::submodule_move,
            working::submodules,
            working::worktrees,
            working::unpushed_submodules,
            windows::close_window_of,
            outside::reveal_file,
            outside::quick_look,
            working::lfs_status,
            working::lfs_prune_preview,
            outside::open_lfs_download,
            history::operation_log,
            history::series,
            history::last_rewrite,
            history::reflog_entries,
            history::fix_state,
            history::compare_snapshot,
            preferences::monospace_fonts,
            actions::refresh_remote_tags,
            history::search,
            history::resolve_commit,
            preferences::settings_text,
            preferences::save_settings_text,
            preferences::settings_location,
            preferences::restore_default_settings,
            preferences::restore_settings_backup,
            preferences::reveal_settings,
            console_window::open_console,
            console_window::console_repos,
            console_window::console_entries,
            console_window::console_clear,
            accounts::github_account,
            accounts::github_sign_in_setup,
            accounts::github_device_start,
            accounts::github_device_wait,
            accounts::github_device_stop,
            accounts::github_sign_in_token,
            accounts::github_sign_out,
            accounts::open_github,
            accounts::github_owners,
            accounts::github_publish,
            gitlab_accounts::gitlab_accounts,
            gitlab_accounts::gitlab_sign_in_setup,
            gitlab_accounts::gitlab_open_new_token,
            gitlab_accounts::gitlab_open_profile,
            gitlab_accounts::gitlab_sign_in,
            gitlab_accounts::gitlab_sign_in_stop,
            gitlab_accounts::gitlab_sign_out,
            pulls::github_repo,
            pulls::gitlab_projects,
            pulls::gitlab_merge_requests,
            pulls::gitlab_merge_request,
            pulls::gitlab_calls_preview,
            pulls::gitlab_run,
            pulls::gitlab_create_facts,
            pulls::gitlab_code_owners,
            pulls::gitlab_people,
            pulls::gitlab_statuses,
            pulls::gitlab_fetch,
            pulls::open_gitlab,
            pulls::github_pulls,
            pulls::github_pull,
            pulls::github_calls_preview,
            pulls::github_run,
            pulls::code_owners,
            pulls::github_pull_statuses,
            pulls::pull_fetch,
            pulls::github_viewing,
            pulls::github_send,
            pulls::review_draft,
            pulls::review_drafts,
            pulls::save_review_draft,
            pulls::gitlab_viewing,
            pulls::gitlab_set_viewed,
            gitlab_accounts::gitlab_namespaces,
            gitlab_accounts::gitlab_publish
        ])
        .run(tauri::generate_context!())
        .expect("error while running Oxbow");
}
