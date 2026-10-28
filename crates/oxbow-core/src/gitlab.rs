//! Merge requests on GitLab, opened by the push itself: GitLab reads push options
//! (`git push -o merge_request.create …`) and opens one for the branch it receives, so it needs
//! no sign-in or token and works on a GitLab server of your own too. Nothing here talks to
//! GitLab's API (that is `gitlab_api.rs`, once signed in); when there is nothing to push, its New
//! merge request page opens in the browser.

use serde::{Deserialize, Serialize};

use crate::cli::GitCommand;
use crate::gitlab_api::Server;
use crate::repo::Repo;

/// A project on GitLab, the way a remote points at it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLabProject {
    /// The remote that points at it, e.g. `origin`.
    pub remote: String,
    pub host: String,
    /// `group/subgroup/project`, without `.git`.
    pub path: String,
    /// The project's page, e.g. `https://gitlab.com/group/project`.
    pub web_url: String,
}

/// What a push asks GitLab for: a merge request of the pushed branch into `target`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeRequest {
    pub target: String,
    pub title: String,
    /// Opened as a draft: the title starts with `Draft:`, which every GitLab version reads.
    pub draft: bool,
    /// GitLab deletes the pushed branch there once the merge request is merged.
    pub remove_source: bool,
}

impl MergeRequest {
    /// The push options, as `git push -o` takes them, each with what it does.
    pub fn push_options(&self) -> Vec<(String, String)> {
        // A push option can't hold a line break: git refuses the push.
        let title = self.title.split_whitespace().collect::<Vec<_>>().join(" ");
        let title = match self.draft {
            true if !title.to_lowercase().starts_with("draft:") => format!("Draft: {title}"),
            _ => title,
        };
        let mut options = vec![
            (
                "merge_request.create".to_owned(),
                format!("GitLab opens a merge request into {}", self.target),
            ),
            (format!("merge_request.target={}", self.target), String::new()),
        ];
        if !title.is_empty() {
            options.push((format!("merge_request.title={title}"), String::new()));
        }
        if self.remove_source {
            options.push((
                "merge_request.remove_source_branch".to_owned(),
                "and deletes the branch there once it is merged".to_owned(),
            ));
        }
        options
    }
}

/// Host and project path from a remote's address in any form git takes: `https://host/g/p.git`,
/// `git@host:g/p.git`, `ssh://git@host:2222/g/p`. Only GitLab's own host and hosts named for it
/// (`gitlab.example.com`, `gitlab-ce.corp`) count: a server of another name looks like any other,
/// unless the repository or a sign-in says otherwise (see [`Repo::gitlab_projects`]).
pub fn parse_gitlab_url(url: &str) -> Option<(String, String)> {
    project_url(url).filter(|(host, _)| named_gitlab(host))
}

/// Host and project path of any server, the way [`parse_gitlab_url`] reads them.
fn project_url(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let (host, path) = if let Some(rest) = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")) {
        let after_user = rest
            .split_once('@')
            .filter(|(user, _)| !user.contains('/'))
            .map_or(rest, |(_, r)| r);
        let (host, path) = after_user.split_once('/')?;
        // A port goes with the page: a server on :8443 serves its pages there.
        (host.to_owned(), path.to_owned())
    } else {
        let target = crate::ssh::ssh_target(url)?;
        (target.host, target.path)
    };
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path).trim_end_matches('/');
    // A project lives in a group or a user: at least two parts.
    if path.split('/').filter(|p| !p.is_empty()).count() < 2 {
        return None;
    }
    Some((host.to_lowercase(), path.to_owned()))
}

fn bare_host(host: &str) -> &str {
    host.split(':').next().unwrap_or(host)
}

fn named_gitlab(host: &str) -> bool {
    bare_host(host)
        .split('.')
        .next()
        .is_some_and(|label| label.starts_with("gitlab"))
}

/// Hosts that are never GitLab, whatever the repository holds: a mirror there keeps its
/// `.gitlab-ci.yml` too.
const OTHER_HOSTS: &[&str] = &[
    "github.com",
    "bitbucket.org",
    "codeberg.org",
    "dev.azure.com",
    "ssh.dev.azure.com",
];

impl Repo {
    /// The GitLab projects the remotes point at, the default remote's first. A server of any
    /// name counts when the repository has GitLab's pipeline file, `.gitlab-ci.yml`, on any
    /// branch (the one checked out may be older than it), or when it is in `signed_in`; then
    /// its pages are where the sign-in says.
    pub fn gitlab_projects(&self, signed_in: &[Server]) -> Vec<GitLabProject> {
        let Ok(remotes) = self.remotes_info() else {
            return Vec::new();
        };
        let unnamed = |url: &str| {
            project_url(url).is_some_and(|(host, _)| {
                !named_gitlab(&host) && !OTHER_HOSTS.contains(&bare_host(&host).trim_start_matches("www."))
            })
        };
        // Only a server of another name needs the look at the branches.
        let pipeline =
            remotes.iter().any(|r| unnamed(&r.push_url) || unnamed(&r.fetch_url)) && self.has_pipeline_file();
        let gitlab = |url: &str| project_url(url).filter(|(host, _)| named_gitlab(host) || (pipeline && unnamed(url)));
        let preferred = self.default_remote();
        let mut projects: Vec<GitLabProject> = remotes
            .iter()
            .filter_map(|r| {
                let known = [&r.push_url, &r.fetch_url].into_iter().find_map(|url| {
                    let (host, path) = project_url(url)?;
                    let server = signed_in.iter().find(|s| bare_host(&s.host) == bare_host(&host))?;
                    Some((server, path))
                });
                if let Some((server, path)) = known {
                    return Some(GitLabProject {
                        remote: r.name.clone(),
                        web_url: format!("{}/{path}", server.web),
                        host: server.host.clone(),
                        path,
                    });
                }
                let (host, path) = gitlab(&r.push_url).or_else(|| gitlab(&r.fetch_url))?;
                // An address over ssh has the ssh port, never the page's; one over plain http
                // has its pages there too.
                let web = [&r.push_url, &r.fetch_url].into_iter().find(|u| u.starts_with("http"));
                let page = if web.is_some() {
                    host.clone()
                } else {
                    bare_host(&host).to_owned()
                };
                let scheme = if web.is_some_and(|u| u.starts_with("http://")) {
                    "http"
                } else {
                    "https"
                };
                Some(GitLabProject {
                    remote: r.name.clone(),
                    web_url: format!("{scheme}://{page}/{path}"),
                    host: page,
                    path,
                })
            })
            .collect();
        projects.sort_by_key(|p| Some(&p.remote) != preferred.as_ref());
        projects
    }
}

impl Repo {
    /// Whether the working folder or a branch here, local or on a remote, has `.gitlab-ci.yml`.
    fn has_pipeline_file(&self) -> bool {
        if self.workdir().join(".gitlab-ci.yml").is_file() {
            return true;
        }
        let Ok(refs) = self.run(&GitCommand::new([
            "for-each-ref",
            "--format=%(refname):.gitlab-ci.yml",
            "refs/heads",
            "refs/remotes",
        ])) else {
            return false;
        };
        self.run_with_input(
            &GitCommand::new(["cat-file", "--batch-check"]),
            Some(refs.stdout.as_bytes()),
        )
        .is_ok_and(|out| out.stdout.lines().any(|line| line.contains(" blob ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitlab_addresses_in_every_form() {
        let project = |url| parse_gitlab_url(url);
        assert_eq!(
            project("git@gitlab.com:acme/api.git"),
            Some(("gitlab.com".into(), "acme/api".into()))
        );
        assert_eq!(
            project("https://gitlab.com/acme/backend/api.git"),
            Some(("gitlab.com".into(), "acme/backend/api".into()))
        );
        assert_eq!(
            project("https://anna@gitlab.example.com:8443/team/app"),
            Some(("gitlab.example.com:8443".into(), "team/app".into()))
        );
        assert_eq!(
            project("ssh://git@gitlab.example.com:2222/team/app.git"),
            Some(("gitlab.example.com".into(), "team/app".into()))
        );
        assert_eq!(
            project("git@GitLab-CE.corp:a/b"),
            Some(("gitlab-ce.corp".into(), "a/b".into()))
        );
        assert_eq!(project("https://github.com/acme/api.git"), None);
        assert_eq!(project("git@git.example.com:acme/api.git"), None);
        assert_eq!(project("https://example.com/gitlab/acme/api"), None);
        assert_eq!(project("https://gitlab.com/acme"), None);
    }

    #[test]
    fn a_merge_request_as_push_options() {
        let request = MergeRequest {
            target: "main".into(),
            title: "Export CSV\nwith headers".into(),
            draft: true,
            remove_source: true,
        };
        let options: Vec<String> = request.push_options().into_iter().map(|(o, _)| o).collect();
        assert_eq!(
            options,
            [
                "merge_request.create",
                "merge_request.target=main",
                "merge_request.title=Draft: Export CSV with headers",
                "merge_request.remove_source_branch",
            ]
        );
        let ready = MergeRequest {
            draft: false,
            remove_source: false,
            title: "  ".into(),
            ..request
        };
        assert_eq!(ready.push_options().len(), 2);
    }
}
