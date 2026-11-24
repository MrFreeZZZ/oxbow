use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use oxbow_core::{GitCommand, RefKind, RemoteInfo, Repo};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::CommandResult;
use crate::session::{RepoRequest, Session, current, expected};

#[derive(Default)]
pub(crate) struct SummaryWindows {
    opening: tauri::async_runtime::Mutex<()>,
    snapshots: Mutex<HashMap<String, Snapshot>>,
}

#[derive(Clone, Serialize)]
pub(crate) struct Snapshot {
    summary: Value,
    context: Context,
}

#[derive(Clone, Serialize)]
struct Context {
    repository: String,
    path: String,
    branches: String,
    remotes: Vec<RemoteInfo>,
    publication: &'static str,
    url: Option<String>,
}

fn web_commit(address: &str, id: &str) -> Option<String> {
    if address.contains(['?', '#', '\\']) || address.chars().any(char::is_control) {
        return None;
    }
    if address.contains("://")
        && !["https://", "http://", "ssh://", "git://"]
            .iter()
            .any(|scheme| address.starts_with(scheme))
    {
        return None;
    }
    let (mut url, path, route) = if let Some((owner, name)) = oxbow_core::pulls::parse_github_url(address) {
        (
            Url::parse("https://github.com/").ok()?,
            format!("{owner}/{name}"),
            "commit",
        )
    } else {
        let (host, path) = oxbow_core::gitlab::parse_gitlab_url(address)?;
        let scheme = if address.starts_with("http://") {
            "http"
        } else {
            "https"
        };
        let url = Url::parse(&format!("{scheme}://{host}/")).ok()?;
        if !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return None;
        }
        (url, path, "-/commit")
    };
    {
        let mut segments = url.path_segments_mut().ok()?;
        segments.pop_if_empty();
        for part in path.split('/').chain(route.split('/')) {
            if part.is_empty() || part == "." || part == ".." {
                return None;
            }
            segments.push(part);
        }
        segments.push(id);
    }
    Some(url.into())
}

fn display_remote_url(address: &str) -> String {
    if !address.contains("://") {
        return address.to_owned();
    }
    let Ok(mut url) = Url::parse(address) else {
        return "Address unavailable".into();
    };
    if matches!(url.scheme(), "http" | "https") || url.password().is_some() {
        let _ = url.set_password(None);
        let _ = url.set_username("");
    }
    url.set_query(None);
    url.set_fragment(None);
    url.into()
}

// Git leaves credentials out of HTTP fetch records and can omit the .git suffix.
fn fetch_identity(address: &str) -> String {
    let address = address.trim();
    if let Ok(mut url) = Url::parse(address)
        && matches!(url.scheme(), "http" | "https")
    {
        let _ = url.set_password(None);
        let _ = url.set_username("");
        let path = url.path().trim_end_matches('/');
        let path = path.strip_suffix(".git").unwrap_or(path).to_owned();
        url.set_path(&path);
        return url.into();
    }
    let address = address.trim_end_matches('/');
    address.strip_suffix(".git").unwrap_or(address).to_owned()
}

/// A slow ancestry check leaves publication unverified.
const ANCESTRY_QUIET_LIMIT: Duration = Duration::from_secs(2);

fn fetched_commit_url(repo: &Repo, id: &str, remotes: &[RemoteInfo]) -> Option<String> {
    let path = repo
        .run(&GitCommand::new([
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "FETCH_HEAD",
        ]))
        .ok()?;
    let fetch_head = std::fs::read_to_string(Path::new(path.stdout.trim_end_matches(['\r', '\n']))).ok()?;
    let upstream = repo.head().ok().and_then(|head| head.branch).and_then(|branch| {
        repo.run(&GitCommand::new([
            "for-each-ref",
            "--format=%(upstream:remotename)",
            &format!("refs/heads/{branch}"),
        ]))
        .ok()
        .map(|output| output.stdout.trim().to_owned())
    });
    let default = repo.default_remote();
    let mut remotes: Vec<_> = remotes.iter().collect();
    remotes.sort_by_key(|remote| {
        if Some(&remote.name) == upstream.as_ref() {
            0
        } else if Some(&remote.name) == default.as_ref() {
            1
        } else {
            2
        }
    });
    for remote in remotes {
        let Some(url) = web_commit(&remote.fetch_url, id) else {
            continue;
        };
        let Ok(effective) = repo.run(&GitCommand::new(["remote", "get-url", "--", &remote.name])) else {
            continue;
        };
        let effective = effective.stdout.trim();
        let identity = fetch_identity(effective);
        if fetch_identity(&remote.fetch_url) != identity {
            continue;
        }
        // Only a recorded fetch ties a specific object to the address that served it.
        let mut tips: Vec<&str> = fetch_head
            .lines()
            .filter_map(|line| {
                let mut fields = line.splitn(3, '\t');
                let (Some(tip), Some(_), Some(source)) = (fields.next(), fields.next(), fields.next()) else {
                    return None;
                };
                let (_, address) = source.rsplit_once(" of ")?;
                (tip.len() == id.len()
                    && tip.bytes().all(|byte| byte.is_ascii_hexdigit())
                    && fetch_identity(address) == identity)
                    .then_some(tip)
            })
            .collect();
        if tips.contains(&id) {
            return Some(url);
        }
        if tips.is_empty() {
            continue;
        }
        tips.sort_unstable();
        tips.dedup();
        // The best common base of the commit and all the tips together is the commit itself
        // exactly when one of the tips contains it.
        let mut args = vec!["--no-replace-objects", "merge-base", id];
        args.extend(tips);
        if repo
            .run(&GitCommand::new(args).quiet_limit(ANCESTRY_QUIET_LIMIT))
            .is_ok_and(|output| output.stdout.trim() == id)
        {
            return Some(url);
        }
    }
    None
}

fn commit_context(repo: &Repo, id: &str) -> Context {
    let remotes = repo.remotes_info().unwrap_or_default();
    let branches = repo.refs().map_or_else(
        |_| "Branch counts unavailable".into(),
        |refs| {
            let local = refs.iter().filter(|r| r.kind == RefKind::Local).count();
            let remote = refs.iter().filter(|r| r.kind == RefKind::Remote).count();
            format!("{local} local · {remote} remote")
        },
    );
    let mut context = Context {
        repository: repo.name(),
        path: repo.workdir().display().to_string(),
        branches,
        remotes: remotes
            .iter()
            .map(|remote| RemoteInfo {
                name: remote.name.clone(),
                fetch_url: display_remote_url(&remote.fetch_url),
                push_url: display_remote_url(&remote.push_url),
            })
            .collect(),
        publication: "Publication status unknown",
        url: None,
    };
    context.url = fetched_commit_url(repo, id, &remotes);
    if context.url.is_some() {
        context.publication = "Confirmed by a recorded fetch";
    } else if repo
        .run(&GitCommand::new([
            "for-each-ref",
            "--contains",
            id,
            "--format=%(refname)%09%(symref)",
            "refs/remotes",
        ]))
        .is_ok_and(|output| output.stdout.lines().any(|line| line.ends_with('\t')))
    {
        context.publication = "Publication unverified · Found in local remote-tracking refs";
    }
    context
}

pub(crate) fn is_summary_window(label: &str) -> bool {
    label.starts_with("commit-summary-")
}

impl SummaryWindows {
    pub(crate) fn remove(&self, label: &str) {
        self.snapshots.lock().expect("summary windows lock").remove(label);
    }
}

#[tauri::command]
pub(crate) async fn commit_link(
    window: RepoRequest,
    session: State<'_, Session>,
    repository: String,
    id: String,
) -> CommandResult<Option<String>> {
    let repo = expected(current(&session, &window)?, Some(&repository))?;
    if !matches!(id.len(), 40 | 64) || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Invalid commit SHA.".into());
    }
    crate::session::blocking(move || {
        let remotes = repo.remotes_info().unwrap_or_default();
        Ok(fetched_commit_url(&repo, &id, &remotes))
    })
    .await
}

#[tauri::command]
pub(crate) async fn open_commit_summary(
    app: AppHandle,
    owner: RepoRequest,
    windows: State<'_, SummaryWindows>,
    session: State<'_, Session>,
    repository: String,
    summary: Value,
) -> CommandResult<()> {
    let label = owner.label();
    if label != "main" && !label.starts_with("repo-") {
        return Err("Open Commit Summary from a repository window.".into());
    }
    let label = format!("commit-summary-{}", owner.output_owner());
    let repo = expected(current(&session, &owner)?, Some(&repository))?;
    let id = summary
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| matches!(id.len(), 40 | 64) && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or("Invalid commit SHA.")?
        .to_owned();
    let _opening = windows.opening.lock().await;
    let context = crate::session::blocking(move || Ok(commit_context(&repo, &id))).await?;
    owner.ensure_open()?;
    let snapshot = Snapshot { summary, context };

    windows
        .snapshots
        .lock()
        .expect("summary windows lock")
        .insert(label.clone(), snapshot);
    if let Some(window) = app.get_webview_window(&label) {
        window
            .emit("commit-summary-changed", ())
            .map_err(|err| err.to_string())?;
        window.unminimize().map_err(|err| err.to_string())?;
        return window.set_focus().map_err(|err| err.to_string());
    }

    let builder = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("index.html#commit-summary".into()))
        .title("Commit Summary")
        .inner_size(760.0, 520.0)
        .min_inner_size(480.0, 320.0)
        .resizable(true)
        .theme(crate::preferences::appearance(&crate::preferences::read_settings(&app)));
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(22.0, 30.0));
    builder.build().map(|_| ()).map_err(|err| {
        windows.remove(&label);
        err.to_string()
    })
}

#[tauri::command]
pub(crate) fn commit_summary_state(
    window: WebviewWindow,
    windows: State<'_, SummaryWindows>,
) -> CommandResult<Snapshot> {
    windows
        .snapshots
        .lock()
        .expect("summary windows lock")
        .get(window.label())
        .cloned()
        .ok_or_else(|| "No commit in this window.".into())
}

#[tauri::command]
pub(crate) fn open_summary_commit_link(window: WebviewWindow, windows: State<'_, SummaryWindows>) -> CommandResult<()> {
    let url = commit_summary_state(window, windows)?
        .context
        .url
        .ok_or("No link for this commit.")?;
    crate::open_in::open_url(&url)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{display_remote_url, web_commit};

    fn run_git(dir: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=QA",
                "-c",
                "user.email=qa@example.com",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    #[test]
    fn changing_remote_url_does_not_reuse_old_tracking_refs_or_fetch_evidence() {
        let dir = oxbow_core::cli::private_temp_folder("oxbow-summary-test-").unwrap();
        let git = |args: &[&str]| run_git(dir.path(), args);
        git(&["init", "-q", "-b", "main"]);
        git(&["remote", "add", "origin", "https://github.com/example/old.git"]);
        git(&["commit", "--allow-empty", "-qm", "Seen on old remote"]);
        let sha = git(&["rev-parse", "HEAD"]);
        git(&["update-ref", "refs/remotes/origin/main", &sha]);
        let fetch_head = git(&["rev-parse", "--path-format=absolute", "--git-path", "FETCH_HEAD"]);
        std::fs::write(
            fetch_head,
            format!("{sha}\t\tbranch 'main' of https://github.com/example/old\n"),
        )
        .unwrap();
        let repo = oxbow_core::Repo::open(dir.path()).unwrap();
        let before = super::commit_context(&repo, &sha);
        assert_eq!(before.publication, "Confirmed by a recorded fetch");
        assert_eq!(before.url, Some(format!("https://github.com/example/old/commit/{sha}")));

        git(&["remote", "set-url", "origin", "https://github.com/example/new.git"]);
        assert_eq!(git(&["rev-parse", "refs/remotes/origin/main"]), sha);
        let after = super::commit_context(&repo, &sha);
        assert_eq!(after.remotes[0].fetch_url, "https://github.com/example/new.git");
        assert_eq!(after.url, None);
        assert_eq!(
            after.publication,
            "Publication unverified · Found in local remote-tracking refs"
        );

        git(&["commit", "--allow-empty", "-qm", "No tracking ref"]);
        let unknown = super::commit_context(&repo, &git(&["rev-parse", "HEAD"]));
        assert_eq!(unknown.url, None);
        assert_eq!(unknown.publication, "Publication status unknown");
    }

    #[test]
    fn history_under_a_recorded_tip_is_confirmed_and_a_side_branch_is_not() {
        let dir = oxbow_core::cli::private_temp_folder("oxbow-summary-test-").unwrap();
        let git = |args: &[&str]| run_git(dir.path(), args);
        git(&["init", "-q", "-b", "main"]);
        git(&["remote", "add", "origin", "https://github.com/example/old.git"]);
        git(&["commit", "--allow-empty", "-qm", "Parent"]);
        let parent = git(&["rev-parse", "HEAD"]);
        git(&["commit", "--allow-empty", "-qm", "Child"]);
        let child = git(&["rev-parse", "HEAD"]);
        git(&["switch", "-q", "-c", "side", &parent]);
        git(&["commit", "--allow-empty", "-qm", "Side"]);
        let side = git(&["rev-parse", "HEAD"]);
        let fetch_head = git(&["rev-parse", "--path-format=absolute", "--git-path", "FETCH_HEAD"]);
        std::fs::write(
            fetch_head,
            format!("{child}\t\tbranch 'main' of https://github.com/example/old\n"),
        )
        .unwrap();
        let repo = oxbow_core::Repo::open(dir.path()).unwrap();

        let under = super::commit_context(&repo, &parent);
        assert_eq!(
            under.url,
            Some(format!("https://github.com/example/old/commit/{parent}"))
        );
        assert_eq!(under.publication, "Confirmed by a recorded fetch");

        let beside = super::commit_context(&repo, &side);
        assert_eq!(beside.url, None);
        assert_eq!(beside.publication, "Publication status unknown");
    }

    #[test]
    fn the_first_remote_whose_fetched_tips_contain_the_commit_gets_the_link() {
        let dir = oxbow_core::cli::private_temp_folder("oxbow-summary-test-").unwrap();
        let git = |args: &[&str]| run_git(dir.path(), args);
        git(&["init", "-q", "-b", "main"]);
        git(&["remote", "add", "origin", "https://github.com/example/old.git"]);
        git(&["remote", "add", "upstream", "https://github.com/example/upstream.git"]);
        git(&["commit", "--allow-empty", "-qm", "Parent"]);
        let parent = git(&["rev-parse", "HEAD"]);
        git(&["commit", "--allow-empty", "-qm", "Child"]);
        let child = git(&["rev-parse", "HEAD"]);
        git(&["switch", "-q", "-c", "other", &parent]);
        git(&["commit", "--allow-empty", "-qm", "Other"]);
        let other = git(&["rev-parse", "HEAD"]);
        git(&["switch", "-q", "-c", "side", &parent]);
        git(&["commit", "--allow-empty", "-qm", "Side"]);
        let side = git(&["rev-parse", "HEAD"]);
        let fetch_head = git(&["rev-parse", "--path-format=absolute", "--git-path", "FETCH_HEAD"]);
        std::fs::write(
            fetch_head,
            format!(
                "{child}\t\tbranch 'main' of https://github.com/example/old\n\
                 {other}\tnot-for-merge\tbranch 'other' of https://github.com/example/old\n\
                 {side}\t\tbranch 'main' of https://github.com/example/upstream\n"
            ),
        )
        .unwrap();
        let repo = oxbow_core::Repo::open(dir.path()).unwrap();

        let shared = super::commit_context(&repo, &parent);
        assert_eq!(
            shared.url,
            Some(format!("https://github.com/example/old/commit/{parent}"))
        );
        assert_eq!(shared.publication, "Confirmed by a recorded fetch");

        let upstream_only = super::commit_context(&repo, &side);
        assert_eq!(
            upstream_only.url,
            Some(format!("https://github.com/example/upstream/commit/{side}"))
        );
        assert_eq!(upstream_only.publication, "Confirmed by a recorded fetch");
    }

    #[test]
    fn confirmed_upstream_then_default_then_other_remote() {
        let dir = oxbow_core::cli::private_temp_folder("oxbow-link-test-").unwrap();
        let git = |args: &[&str]| run_git(dir.path(), args);
        git(&["init", "-q", "-b", "topic"]);
        git(&["commit", "--allow-empty", "-qm", "Published"]);
        let sha = git(&["rev-parse", "HEAD"]);
        for name in ["aaa", "origin", "upstream"] {
            git(&["remote", "add", name, &format!("https://gitlab.local/team/{name}.git")]);
            git(&["update-ref", &format!("refs/remotes/{name}/topic"), &sha]);
        }
        git(&["branch", "--set-upstream-to=upstream/topic"]);
        let fetch_head = git(&["rev-parse", "--path-format=absolute", "--git-path", "FETCH_HEAD"]);
        let repo = oxbow_core::Repo::open(dir.path()).unwrap();
        for names in [
            vec!["aaa", "origin", "upstream"],
            vec!["aaa", "origin"],
            vec!["aaa"],
            vec![],
        ] {
            std::fs::write(
                &fetch_head,
                names
                    .iter()
                    .map(|name| format!("{sha}\t\tbranch 'topic' of https://gitlab.local/team/{name}\n"))
                    .collect::<String>(),
            )
            .unwrap();
            let chosen = names
                .last()
                .map(|name| format!("https://gitlab.local/team/{name}/-/commit/{sha}"));
            assert_eq!(
                super::fetched_commit_url(&repo, &sha, &repo.remotes_info().unwrap()),
                chosen
            );
        }
        git(&["config", "--unset", "branch.topic.merge"]);
        std::fs::write(
            &fetch_head,
            ["aaa", "origin", "upstream"]
                .iter()
                .map(|name| format!("{sha}\t\tbranch 'topic' of https://gitlab.local/team/{name}\n"))
                .collect::<String>(),
        )
        .unwrap();
        assert_eq!(git(&["config", "branch.topic.remote"]), "upstream");
        assert_eq!(
            super::fetched_commit_url(&repo, &sha, &repo.remotes_info().unwrap()),
            Some(format!("https://gitlab.local/team/origin/-/commit/{sha}"))
        );
        git(&["switch", "--detach", "-q"]);
        std::fs::write(
            &fetch_head,
            ["aaa", "origin", "upstream"]
                .iter()
                .map(|name| format!("{sha}\t\tbranch 'topic' of https://gitlab.local/team/{name}\n"))
                .collect::<String>(),
        )
        .unwrap();
        assert_eq!(
            super::fetched_commit_url(&repo, &sha, &repo.remotes_info().unwrap()),
            Some(format!("https://gitlab.local/team/origin/-/commit/{sha}"))
        );
    }

    #[test]
    fn rewritten_fetch_url_is_not_publication_evidence_for_the_configured_address() {
        let dir = oxbow_core::cli::private_temp_folder("oxbow-link-test-").unwrap();
        let git = |args: &[&str]| run_git(dir.path(), args);
        git(&["init", "-q", "-b", "main"]);
        git(&["commit", "--allow-empty", "-qm", "Published elsewhere"]);
        let sha = git(&["rev-parse", "HEAD"]);
        git(&["remote", "add", "origin", "https://github.com/team/repo.git"]);
        git(&[
            "config",
            "url.https://github.com/other/.insteadOf",
            "https://github.com/team/",
        ]);
        let fetch_head = git(&["rev-parse", "--path-format=absolute", "--git-path", "FETCH_HEAD"]);
        std::fs::write(
            fetch_head,
            format!("{sha}\t\tbranch 'main' of https://github.com/other/repo\n"),
        )
        .unwrap();
        let repo = oxbow_core::Repo::open(dir.path()).unwrap();
        assert_eq!(
            super::fetched_commit_url(&repo, &sha, &repo.remotes_info().unwrap()),
            None
        );
    }

    #[test]
    fn remote_addresses_omit_credentials_in_summary() {
        assert_eq!(
            display_remote_url("https://token@example.com/team/repo.git?secret=value#fragment"),
            "https://example.com/team/repo.git"
        );
        assert_eq!(
            display_remote_url("git@example.com:team/repo.git"),
            "git@example.com:team/repo.git"
        );
        assert_eq!(
            display_remote_url("ssh://git@example.com/team/repo.git"),
            "ssh://git@example.com/team/repo.git"
        );
    }

    #[test]
    fn commit_links_use_the_forge_route_without_remote_credentials() {
        assert_eq!(
            web_commit("https://user:secret@github.com/team/repo.git", "abc").as_deref(),
            Some("https://github.com/team/repo/commit/abc")
        );
        assert_eq!(
            web_commit("http://gitlab.local:8080/group/subgroup/repo.git", "abc").as_deref(),
            Some("http://gitlab.local:8080/group/subgroup/repo/-/commit/abc")
        );
        assert_eq!(web_commit("git@github.com:team/../repo", "abc"), None);
        assert_eq!(web_commit("/local/repo.git", "abc"), None);
        for address in [
            "https://user:secret@gitlab.local/team/repo.git?token=secret",
            "https://github.com/team/repo.git#secret",
            "file://gitlab.local/team/repo.git",
            "https://gitlab.local/team/../repo.git",
        ] {
            assert_eq!(web_commit(address, "abc"), None);
        }
        assert_eq!(
            web_commit("ssh://git@gitlab.example.com:2222/team/repo.git", "abc").as_deref(),
            Some("https://gitlab.example.com/team/repo/-/commit/abc")
        );
    }
}
