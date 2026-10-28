//! GitLab: signing in against a local server that answers like GitLab, and which remotes are
//! GitLab projects.

mod support;

use oxbow_core::Error;
use oxbow_core::Repo;
use oxbow_core::gitlab_api::{Client, Server};
use support::{Fixture, stand_in};

const USER: &str = r#"{"id":7,"username":"anna","name":"Anna Berg","avatar_url":"https://gitlab.com/uploads/anna.png","web_url":"https://gitlab.com/anna"}"#;

fn client(address: &str) -> Client {
    Client::new(Server::parse(address).unwrap()).with_token("glpat-secret")
}

#[test]
fn signing_in_reads_the_user_the_token_and_the_version() {
    let (address, seen) = stand_in(vec![
        ("/api/v4/user", USER),
        ("/api/v4/version", r#"{"version":"17.5.1-ee","revision":"abc"}"#),
        (
            "/api/v4/personal_access_tokens/self",
            r#"{"id":1,"name":"Oxbow","scopes":["api"],"expires_at":"2026-11-05","active":true}"#,
        ),
    ]);
    let me = client(&address).me().unwrap();
    assert_eq!(me.user.username, "anna");
    assert_eq!(me.user.name.as_deref(), Some("Anna Berg"));
    assert_eq!(me.scopes, Some(vec!["api".to_owned()]));
    assert_eq!(me.expires_at.as_deref(), Some("2026-11-05"));
    assert_eq!(me.version, "17.5.1-ee");
    assert_eq!(me.problem(), None);
    let paths: Vec<String> = seen.lock().unwrap().iter().map(|(path, _)| path.clone()).collect();
    assert_eq!(
        paths,
        ["/api/v4/user", "/api/v4/version", "/api/v4/personal_access_tokens/self"]
    );
}

#[test]
fn a_token_of_another_kind_signs_in_without_scopes() {
    // A server of your own on a port; a group's token has no page of its own.
    let (address, _) = stand_in(vec![
        ("/api/v4/user", USER),
        ("/api/v4/version", r#"{"version":"16.0.0"}"#),
    ]);
    assert!(address.contains(':'));
    let me = client(&address).me().unwrap();
    assert_eq!((&me.scopes, &me.expires_at), (&None, &None));
    assert_eq!(me.problem(), None);
}

#[test]
fn a_wrong_token_and_a_token_without_api_say_so() {
    let (address, _) = stand_in(vec![("/api/v4/user", r#"401 {"message":"401 Unauthorized"}"#)]);
    match client(&address).me() {
        Err(Error::GitLab { status, message }) => {
            assert_eq!(status, Some(401));
            assert!(message.contains("wrong, expired or revoked"), "{message}");
        }
        other => panic!("{other:?}"),
    }

    let (address, _) = stand_in(vec![(
        "/api/v4/user",
        r#"403 {"error":"insufficient_scope","error_description":"The request requires higher privileges than provided by the access token."}"#,
    )]);
    let message = client(&address).me().unwrap_err().to_string();
    assert!(message.contains("needs the api scope"), "{message}");

    let (address, _) = stand_in(vec![
        ("/api/v4/user", USER),
        ("/api/v4/version", r#"{"version":"17.0.0"}"#),
        ("/api/v4/personal_access_tokens/self", r#"{"scopes":["read_api"]}"#),
    ]);
    let problem = client(&address).me().unwrap().problem().unwrap();
    assert!(
        problem.contains("needs the api scope, and it has read_api"),
        "{problem}"
    );
}

#[test]
fn something_that_isnt_gitlab_says_so() {
    let (address, _) = stand_in(vec![("/api/v4/user", "<html>Welcome</html>")]);
    let message = client(&address).me().unwrap_err().to_string();
    assert!(message.contains("doesn’t answer like GitLab"), "{message}");
}

#[test]
fn a_signed_in_server_is_gitlab_whatever_its_name() {
    let mut fixture = Fixture::new();
    fixture.commit("a.txt", "a", "first");
    fixture.git(&["remote", "add", "origin", "git@git.corp.example:team/app.git"]);
    fixture.git(&["remote", "add", "mirror", "https://gitlab.com/team/app.git"]);
    let repo = Repo::open(fixture.path()).unwrap();

    let names = |projects: Vec<oxbow_core::GitLabProject>| {
        projects.into_iter().map(|p| (p.remote, p.web_url)).collect::<Vec<_>>()
    };
    assert_eq!(
        names(repo.gitlab_projects(&[])),
        [("mirror".to_owned(), "https://gitlab.com/team/app".to_owned())]
    );
    // Signed in to the company's server, whose pages are on another port.
    let server = Server::parse("https://git.corp.example:8443").unwrap();
    assert_eq!(
        names(repo.gitlab_projects(&[server])),
        [
            ("origin".to_owned(), "https://git.corp.example:8443/team/app".to_owned()),
            ("mirror".to_owned(), "https://gitlab.com/team/app".to_owned()),
        ]
    );
}

#[test]
fn merge_requests_come_as_pull_requests() {
    let (address, seen) = stand_in(vec![(
        "/api/v4/projects/team%2Fapp/merge_requests",
        r#"[
          {"id":9101,"iid":12,"title":"Export CSV","state":"opened","draft":true,"source_branch":"csv","target_branch":"main",
           "sha":"abc123","source_project_id":5,"target_project_id":5,"web_url":"https://git.corp/team/app/-/merge_requests/12",
           "description":null,"author":{"username":"anna"},"created_at":"2026-10-20T10:00:00.000Z","updated_at":"2026-10-28T09:30:15.512Z",
           "reviewers":[{"username":"boris"}]},
          {"id":9100,"iid":11,"title":"Fix login","state":"merged","draft":false,"source_branch":"login","target_branch":"main",
           "sha":"def456","source_project_id":8,"target_project_id":5,"web_url":"https://git.corp/team/app/-/merge_requests/11",
           "description":"Closes #3","author":{"username":"boris"},"created_at":"2026-10-01T10:00:00Z","updated_at":"2026-10-02T10:00:00Z",
           "reviewers":[]}
        ]"#,
    )]);
    let project = oxbow_core::GitLabProject {
        remote: "origin".into(),
        host: "git.corp".into(),
        path: "team/app".into(),
        web_url: "https://git.corp/team/app".into(),
    };
    let list = client(&address).merge_requests(&project).unwrap();
    assert_eq!(
        seen.lock().unwrap()[0].0,
        "/api/v4/projects/team%2Fapp/merge_requests?state=all&order_by=updated_at&sort=desc&per_page=100"
    );
    let first = &list[0];
    assert_eq!(
        (first.number, first.title.as_str(), first.draft),
        (12, "Export CSV", true)
    );
    assert_eq!(
        (first.head.as_str(), first.base.as_str(), first.head_sha.as_str()),
        ("csv", "main", "abc123")
    );
    assert_eq!((first.head_owner.as_deref(), first.body.as_str()), (None, ""));
    assert_eq!(
        (first.author.login.as_str(), first.requested.clone()),
        ("anna", vec!["boris".to_owned()])
    );
    assert_eq!(first.updated, 1_793_179_815);
    assert_eq!(first.node_id, "9101");
    assert_eq!(
        serde_json::to_value(list[1].state).unwrap(),
        serde_json::json!("merged")
    );
    assert_eq!(
        list[1].head_owner.as_deref(),
        Some("project 8"),
        "a fork's merge request"
    );

    let (address, _) = stand_in(vec![("/api/v4/projects", r#"404 {"message":"404 Project Not Found"}"#)]);
    let message = client(&address).merge_requests(&project).unwrap_err().to_string();
    assert!(message.contains("doesn't know this project"), "{message}");
}

#[test]
fn a_merge_request_in_full_comes_as_a_pull_request() {
    let mut fx = Fixture::new();
    let base = fx.commit("app.py", "a\nb\nc\nd\n", "Start");
    fx.git(&["checkout", "-q", "-b", "csv"]);
    let first = fx.commit("app.py", "a\nb\nc\nd\nexport()\n", "Export");
    let head = fx.commit("csv.py", "rows\n", "Rows");
    let leak = |text: String| -> &'static str { Box::leak(text.into_boxed_str()) };
    let detail = leak(format!(
        r#"{{"id":9101,"iid":12,"title":"Export CSV","state":"opened","draft":false,"source_branch":"csv","target_branch":"main",
           "sha":"{head}","source_project_id":5,"target_project_id":5,"web_url":"https://git.corp/team/app/-/merge_requests/12",
           "description":"Adds CSV","author":{{"username":"anna"}},"created_at":"2026-10-20T10:00:00Z","updated_at":"2026-10-28T09:00:00Z",
           "reviewers":[{{"username":"boris"}},{{"username":"vera"}}],"detailed_merge_status":"discussions_not_resolved",
           "has_conflicts":false,"changes_count":"2","merge_when_pipeline_succeeds":false,"squash":true,
           "diverged_commits_count":3,"user":{{"can_merge":true}},
           "diff_refs":{{"base_sha":"{base}","start_sha":"{base}","head_sha":"{head}"}},
           "head_pipeline":{{"id":77,"status":"failed","web_url":"https://git.corp/team/app/-/pipelines/77"}}}}"#
    ));
    let commits = leak(format!(
        r#"[{{"id":"{head}","title":"Rows","message":"Rows\n","author_name":"Anna","committed_date":"2026-10-21T10:00:00Z"}},
            {{"id":"{first}","title":"Export","message":"Export\n","author_name":"Anna","committed_date":"2026-10-20T10:00:00Z"}}]"#
    ));
    let (address, _) = stand_in(vec![
        ("/api/v4/projects/team%2Fapp/merge_requests/12/commits", commits),
        (
            "/api/v4/projects/team%2Fapp/merge_requests/12/discussions",
            r#"[
              {"id":"d1","individual_note":true,"notes":[{"id":1,"system":true,"body":"added 2 commits\n\n<ul><li>…</li></ul>","author":{"username":"anna"},"created_at":"2026-10-21T10:00:00Z"}]},
              {"id":"d2","individual_note":true,"notes":[{"id":2,"system":false,"body":"Looks close","author":{"username":"vera"},"created_at":"2026-10-21T11:00:00Z"}]},
              {"id":"d3","individual_note":false,"notes":[
                {"id":3,"type":"DiffNote","system":false,"body":"Name it export_csv?","author":{"username":"boris"},"created_at":"2026-10-21T12:00:00Z",
                 "resolvable":true,"resolved":false,"position":{"new_path":"app.py","old_path":"app.py","new_line":5,"old_line":null}},
                {"id":4,"type":"DiffNote","system":false,"body":"Sure","author":{"username":"anna"},"created_at":"2026-10-21T13:00:00Z","resolvable":true,"resolved":false}]},
              {"id":"d4","individual_note":true,"notes":[{"id":5,"system":true,"body":"approved this merge request","author":{"username":"vera"},"created_at":"2026-10-22T10:00:00Z"}]}
            ]"#,
        ),
        (
            "/api/v4/projects/team%2Fapp/merge_requests/12/approvals",
            r#"{"approvals_required":2,"approvals_left":1,"approved_by":[{"user":{"username":"vera"}}]}"#,
        ),
        (
            "/api/v4/projects/team%2Fapp/merge_requests/12/reviewers",
            r#"[{"user":{"username":"boris"},"state":"requested_changes","created_at":"2026-10-21T12:00:00Z"},{"user":{"username":"vera"},"state":"approved"}]"#,
        ),
        ("/api/v4/projects/team%2Fapp/merge_requests/12", detail),
        (
            "/api/v4/projects/team%2Fapp/pipelines/77/jobs",
            r#"[{"id":902,"name":"lint","stage":"test","status":"failed","allow_failure":true,"duration":12.4,"web_url":"https://git.corp/team/app/-/jobs/902"},
                {"id":901,"name":"build","stage":"build","status":"success","duration":61.0,"web_url":"https://git.corp/team/app/-/jobs/901"},
                {"id":903,"name":"rspec","stage":"test","status":"running","duration":null}]"#,
        ),
        (
            "/api/v4/projects/team%2Fapp",
            r#"{"default_branch":"main","merge_method":"ff","squash_option":"default_on"}"#,
        ),
    ]);
    let project = oxbow_core::GitLabProject {
        remote: "origin".into(),
        host: "git.corp".into(),
        path: "team/app".into(),
        web_url: "https://git.corp/team/app".into(),
    };
    let mut mr = client(&address).merge_request(&project, 12).unwrap();
    assert_eq!((mr.summary.number, mr.summary.title.as_str()), (12, "Export CSV"));
    assert_eq!(
        (mr.mergeable, mr.mergeable_state.as_str(), mr.changed_files),
        (Some(true), "blocked", 2)
    );
    let facts = mr.gitlab.clone().unwrap();
    assert_eq!(
        (
            facts.merge_status.as_str(),
            facts.merge_method.as_str(),
            facts.squash,
            facts.behind
        ),
        ("discussions_not_resolved", "ff", true, Some(3))
    );
    assert_eq!(facts.pipeline.unwrap().id, 77);
    assert_eq!(
        mr.commits.iter().map(|c| c.summary.as_str()).collect::<Vec<_>>(),
        ["Export", "Rows"],
        "oldest first"
    );
    assert_eq!(
        mr.checks
            .iter()
            .map(|c| (c.name.as_str(), c.state.as_str(), c.seconds))
            .collect::<Vec<_>>(),
        [
            ("build", "ok", Some(61)),
            ("lint", "warn", Some(12)),
            ("rspec", "run", None)
        ]
    );
    assert_eq!(mr.checks[1].detail, "test, allowed to fail");
    assert_eq!(
        mr.reviews
            .iter()
            .map(|r| (r.author.login.as_str(), r.state.as_str()))
            .collect::<Vec<_>>(),
        [("vera", "approved"), ("boris", "changesRequested")]
    );
    assert_eq!(mr.required_approvals, Some(2));
    assert!(mr.settings.can_push && mr.settings.squash && mr.settings.merge_commit);
    let kinds: Vec<String> = mr
        .timeline
        .iter()
        .map(|e| serde_json::to_value(e).unwrap()["kind"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(kinds, ["event", "comment", "thread", "event"]);
    let thread = serde_json::to_value(&mr.timeline[2]).unwrap();
    assert_eq!(
        (
            thread["path"].as_str(),
            thread["line"].as_u64(),
            thread["resolved"].as_bool()
        ),
        (Some("app.py"), Some(5), Some(false))
    );
    assert_eq!(thread["comments"].as_array().unwrap().len(), 2);
    assert_eq!(thread["node"], "d3");

    // What the commits here tell: the lines, and the code under the comment.
    let repo = Repo::open(fx.path()).unwrap();
    repo.merge_request_here(&mut mr);
    assert_eq!((mr.additions, mr.deletions), (2, 0));
    assert!(mr.gitlab.as_ref().unwrap().line_counts);
    assert_eq!(mr.commits[0].additions, Some(1));
    let thread = serde_json::to_value(&mr.timeline[2]).unwrap();
    let hunk: Vec<(Option<u64>, &str, bool)> = thread["hunk"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            (
                l["number"].as_u64(),
                l["text"].as_str().unwrap(),
                l["mark"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        hunk,
        [
            (Some(2), "b", false),
            (Some(3), "c", false),
            (Some(4), "d", false),
            (Some(5), "export()", true)
        ]
    );
}
