//! Merge requests on GitLab, opened by the push itself: GitLab reads push options
//! (`git push -o merge_request.create …`) and opens one for the branch it receives, so it needs
//! no sign-in or token and works on a GitLab server of your own too. Nothing here talks to
//! GitLab's API; when there is nothing to push, its New merge request page opens in the browser.

use serde::{Deserialize, Serialize};

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
/// (`gitlab.example.com`, `gitlab-ce.corp`) count: a server of another name looks like any other.
pub fn parse_gitlab_url(url: &str) -> Option<(String, String)> {
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
    let name = host.split(':').next().unwrap_or(&host).to_lowercase();
    if !name.split('.').next().is_some_and(|label| label.starts_with("gitlab")) {
        return None;
    }
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path).trim_end_matches('/');
    // A project lives in a group or a user: at least two parts.
    if path.split('/').filter(|p| !p.is_empty()).count() < 2 {
        return None;
    }
    Some((host.to_lowercase(), path.to_owned()))
}

impl Repo {
    /// The GitLab projects the remotes point at, the default remote's first.
    pub fn gitlab_projects(&self) -> Vec<GitLabProject> {
        let Ok(remotes) = self.remotes_info() else {
            return Vec::new();
        };
        let preferred = self.default_remote();
        let mut projects: Vec<GitLabProject> = remotes
            .iter()
            .filter_map(|r| {
                let (host, path) = parse_gitlab_url(&r.push_url).or_else(|| parse_gitlab_url(&r.fetch_url))?;
                // An address over ssh has the ssh port, never the page's.
                let page = if r.push_url.starts_with("http") || r.fetch_url.starts_with("http") {
                    host.clone()
                } else {
                    host.split(':').next().unwrap_or(&host).to_owned()
                };
                Some(GitLabProject {
                    remote: r.name.clone(),
                    web_url: format!("https://{page}/{path}"),
                    host: page,
                    path,
                })
            })
            .collect();
        projects.sort_by_key(|p| Some(&p.remote) != preferred.as_ref());
        projects
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
