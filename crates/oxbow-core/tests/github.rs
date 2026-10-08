//! The GitHub client against a local server that answers like GitHub.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

use oxbow_core::Error;
use oxbow_core::github::{Client, DevicePoll};

/// A request the server got: its first line, headers and body.
struct Seen {
    line: String,
    headers: Vec<String>,
    body: String,
}

/// An answer: status, extra headers and the JSON body.
type Answer = (u16, Vec<(&'static str, &'static str)>, &'static str);

/// Serve `answers` in order, one per connection, and report each request.
fn server(answers: Vec<Answer>) -> (String, mpsc::Receiver<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let (send, receive) = mpsc::channel();
    thread::spawn(move || {
        for (status, headers, body) in answers {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let mut seen_headers = Vec::new();
            let mut length = 0;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                let header = header.trim_end().to_owned();
                if header.is_empty() {
                    break;
                }
                if let Some(value) = header.to_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
                seen_headers.push(header);
            }
            let mut request_body = vec![0; length];
            reader.read_exact(&mut request_body).unwrap();
            send.send(Seen {
                line: line.trim_end().to_owned(),
                headers: seen_headers,
                body: String::from_utf8(request_body).unwrap(),
            })
            .unwrap();
            let mut stream = stream;
            let extra: String = headers.iter().map(|(k, v)| format!("{k}: {v}\r\n")).collect();
            write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    (address, receive)
}

#[test]
fn device_flow_asks_for_a_code_then_waits_for_the_token() {
    let (address, seen) = server(vec![
        (
            200,
            vec![],
            r#"{"device_code":"dc1","user_code":"WDJB-MJHT","verification_uri":"https://github.com/login/device","expires_in":900,"interval":5}"#,
        ),
        (200, vec![], r#"{"error":"authorization_pending"}"#),
        (200, vec![], r#"{"error":"slow_down","interval":10}"#),
        (
            200,
            vec![],
            r#"{"access_token":"gho_abc","token_type":"bearer","scope":"repo"}"#,
        ),
    ]);
    let client = Client::new(&address, &address);

    let code = client.device_code("Iv1.id").unwrap();
    assert_eq!(code.user_code, "WDJB-MJHT");
    assert_eq!(code.interval, 5);
    let request = seen.recv().unwrap();
    assert_eq!(request.line, "POST /login/device/code HTTP/1.1");
    assert_eq!(request.body, "client_id=Iv1.id&scope=repo+workflow+read%3Aorg");
    assert!(
        request.headers.iter().any(|h| h == "accept: application/json"),
        "{:?}",
        request.headers
    );

    assert_eq!(client.poll_device("Iv1.id", "dc1").unwrap(), DevicePoll::Waiting);
    assert_eq!(client.poll_device("Iv1.id", "dc1").unwrap(), DevicePoll::SlowDown(10));
    assert_eq!(
        client.poll_device("Iv1.id", "dc1").unwrap(),
        DevicePoll::Approved("gho_abc".into())
    );
    let poll = seen.recv().unwrap();
    assert!(poll.body.contains("device_code=dc1"));
    assert!(
        poll.body
            .contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code")
    );
}

#[test]
fn me_reads_the_user_and_the_scopes() {
    let (address, seen) = server(vec![
        (
            200,
            vec![("X-OAuth-Scopes", "repo, workflow, read:org")],
            r#"{"login":"octocat","name":"The Octocat","avatar_url":"https://a/1","html_url":"https://github.com/octocat","id":1}"#,
        ),
        (
            200,
            vec![],
            r#"{"login":"robot","name":null,"avatar_url":"https://a/2","html_url":"https://github.com/robot"}"#,
        ),
        (401, vec![], r#"{"message":"Bad credentials"}"#),
    ]);
    let client = Client::new(&address, &address).with_token("gho_abc");

    let me = client.me().unwrap();
    assert_eq!(me.user.login, "octocat");
    assert_eq!(me.user.name.as_deref(), Some("The Octocat"));
    assert_eq!(
        me.scopes,
        Some(vec!["repo".into(), "workflow".into(), "read:org".into()])
    );
    let request = seen.recv().unwrap();
    assert_eq!(request.line, "GET /user HTTP/1.1");
    assert!(
        request.headers.iter().any(|h| h == "authorization: Bearer gho_abc"),
        "{:?}",
        request.headers
    );

    // A fine-grained token lists no scopes.
    assert_eq!(client.me().unwrap().scopes, None);

    match client.me() {
        Err(Error::GitHub { status, message }) => {
            assert_eq!(status, Some(401));
            assert!(message.contains("didn't accept the token"), "{message}");
        }
        other => panic!("expected a GitHub error, got {other:?}"),
    }
}

#[test]
fn a_disabled_device_flow_is_explained() {
    let (address, _seen) = server(vec![(200, vec![], r#"{"error":"device_flow_disabled"}"#)]);
    let err = Client::new(&address, &address).device_code("Iv1.id").unwrap_err();
    assert!(err.to_string().contains("Device flow is turned off"), "{err}");
}

#[test]
fn commits_are_counted_from_the_last_page() {
    let (api, seen) = server(vec![
        (
            200,
            vec![(
                "Link",
                "<https://api.github.com/repositories/1/commits?sha=main&per_page=1&page=2>; rel=\"next\", <https://api.github.com/repositories/1/commits?sha=main&per_page=1&page=1234>; rel=\"last\"",
            )],
            "[{}]",
        ),
        (200, vec![], "[{}]"),
    ]);
    let client = Client::new(&api, &api);
    assert_eq!(client.commit_count("acme", "api", "main").unwrap(), 1234);
    let first = seen.recv().unwrap();
    assert_eq!(first.line, "GET /repos/acme/api/commits?sha=main&per_page=1 HTTP/1.1");
    assert!(
        !first
            .headers
            .iter()
            .any(|h| h.to_lowercase().starts_with("authorization"))
    );
    // One commit: no other pages.
    assert_eq!(client.commit_count("acme", "api", "feature/x").unwrap(), 1);
    assert!(seen.recv().unwrap().line.contains("sha=feature%2Fx"));
}
