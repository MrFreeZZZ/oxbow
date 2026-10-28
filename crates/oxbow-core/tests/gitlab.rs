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
