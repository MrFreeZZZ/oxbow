mod support;

use oxbow_core::{Repo, SearchMode, SearchQuery};
use support::Fixture;

fn find(repo: &Repo, mode: SearchMode, text: &str) -> Vec<(String, Vec<String>)> {
    let query = SearchQuery {
        mode,
        text: text.into(),
        branch: None,
        since: None,
        author: None,
    };
    let result = repo.search(&query).unwrap();
    result.hits.into_iter().map(|h| (h.id, h.files)).collect()
}

#[test]
fn every_mode_finds_what_git_log_finds() {
    let mut fx = Fixture::new();
    let root = fx.commit("src/app.rs", "fn main() {}\n", "Root");
    let limit = fx.commit(
        "src/rate_limit.rs",
        "struct RateLimit;\n",
        "Add a limiter\n\nKeeps login attempts in check.",
    );
    fx.git(&["switch", "-q", "-c", "docs"]);
    let docs = fx.commit("docs/API.md", "RateLimit headers\n", "Document the limiter");

    let repo = Repo::open(fx.path()).unwrap();
    // Message: any case, the description counts too, every branch.
    let ids: Vec<_> = find(&repo, SearchMode::Message, "LOGIN ATTEMPTS")
        .into_iter()
        .map(|h| h.0)
        .collect();
    assert_eq!(ids, std::slice::from_ref(&limit));
    let ids: Vec<_> = find(&repo, SearchMode::Message, "limiter")
        .into_iter()
        .map(|h| h.0)
        .collect();
    assert_eq!(ids, [docs.clone(), limit.clone()]);
    // Code: commits that add or remove the text, with their files.
    assert_eq!(
        find(&repo, SearchMode::Code, "RateLimit"),
        [
            (docs.clone(), vec!["docs/API.md".to_owned()]),
            (limit.clone(), vec!["src/rate_limit.rs".to_owned()])
        ]
    );
    // File: any case, only the files that matched.
    assert_eq!(
        find(&repo, SearchMode::File, "api"),
        [(docs, vec!["docs/API.md".to_owned()])]
    );
    assert_eq!(find(&repo, SearchMode::Author, "alexander").len(), 3);
    assert!(find(&repo, SearchMode::Author, "maria").is_empty());
    assert_eq!(
        find(&repo, SearchMode::File, "app.rs"),
        [(root, vec!["src/app.rs".to_owned()])]
    );
}

#[test]
fn branch_filter_limits_the_search() {
    let mut fx = Fixture::new();
    let main = fx.commit("a.txt", "one\n", "Fix login");
    fx.git(&["switch", "-q", "-c", "topic"]);
    fx.commit("a.txt", "two\n", "Fix login again");
    let repo = Repo::open(fx.path()).unwrap();
    let query = SearchQuery {
        mode: SearchMode::Message,
        text: "login".into(),
        branch: Some("main".into()),
        since: None,
        author: None,
    };
    let result = repo.search(&query).unwrap();
    assert_eq!(result.hits.iter().map(|h| h.id.clone()).collect::<Vec<_>>(), [main]);
    assert_eq!(result.command, "git log main --oneline -i -F --grep=login");
}

#[test]
fn a_sha_prefix_resolves_anywhere_in_the_repository() {
    let mut fx = Fixture::new();
    let root = fx.commit("a.txt", "1\n", "Root");
    fx.commit("a.txt", "2\n", "Second");
    let repo = Repo::open(fx.path()).unwrap();
    assert_eq!(repo.resolve_commit(&root[..7]).as_deref(), Some(root.as_str()));
    assert_eq!(
        repo.resolve_commit(&root.to_uppercase()[..10]).as_deref(),
        Some(root.as_str())
    );
    assert_eq!(repo.resolve_commit("main"), None);
    assert_eq!(repo.resolve_commit("abc"), None);
    assert_eq!(repo.resolve_commit("0000000"), None);
}

#[test]
fn file_search_finds_names_with_tabs_and_spaces() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    let id = fx.commit("hello\tworld.txt", "x\n", "Add a tab");
    fx.commit(" spaced .txt", "y\n", "Add spaces");
    let repo = Repo::open(fx.path()).unwrap();
    assert_eq!(
        find(&repo, SearchMode::File, "hello\tworld"),
        [(id, vec!["hello\tworld.txt".to_owned()])]
    );
    let spaced = find(&repo, SearchMode::File, "spaced");
    assert_eq!(spaced[0].1, [" spaced .txt"]);
}

#[test]
fn file_search_finds_names_with_control_bytes() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    let id = fx.commit("pre\u{1}post.txt", "x\n", "Add a control byte");
    // A name that looks like the start of the next commit.
    let header = format!("\u{1}{id}");
    let other = fx.commit(&header, "y\n", "Add a name like an id");
    let repo = Repo::open(fx.path()).unwrap();
    assert_eq!(
        find(&repo, SearchMode::File, "pre\u{1}post"),
        [(id.clone(), vec!["pre\u{1}post.txt".to_owned()])]
    );
    assert_eq!(find(&repo, SearchMode::File, &header), [(other, vec![header.clone()])]);
}

#[test]
fn a_file_search_takes_the_text_as_it_is_not_as_a_pattern() {
    let mut fx = Fixture::new();
    fx.commit("pagea.txt", "a\n", "Plain");
    let brackets = fx.commit("routes/page[ab].txt", "x\n", "Brackets");
    let unicode = fx.commit("docs/Straße.md", "s\n", "Unicode");
    let repo = Repo::open(fx.path()).unwrap();
    let files = |text: &str| -> Vec<String> {
        find(&repo, SearchMode::File, text)
            .into_iter()
            .flat_map(|(_, files)| files)
            .collect()
    };
    assert_eq!(files("page[ab]"), ["routes/page[ab].txt"]);
    assert_eq!(files("PAGE[AB]"), ["routes/page[ab].txt"]);
    assert_eq!(find(&repo, SearchMode::File, "[ab]")[0].0, brackets);
    assert_eq!(files("e[a"), ["routes/page[ab].txt"]);
    assert_eq!(files("straße"), ["docs/Straße.md"]);
    assert_eq!(find(&repo, SearchMode::File, "straße")[0].0, unicode);
    assert_eq!(files("pagea"), ["pagea.txt"]);
}

/// Names a Windows disk can't hold.
#[cfg(unix)]
#[test]
fn a_file_search_finds_stars_question_marks_and_backslashes_as_they_are() {
    let mut fx = Fixture::new();
    fx.commit("axb.txt", "a\n", "Plain");
    fx.commit("a*b.txt", "s\n", "Star");
    fx.commit("what?.txt", "q\n", "Question");
    fx.commit("back\\slash.txt", "b\n", "Backslash");
    let repo = Repo::open(fx.path()).unwrap();
    let files = |text: &str| -> Vec<String> {
        find(&repo, SearchMode::File, text)
            .into_iter()
            .flat_map(|(_, files)| files)
            .collect()
    };
    assert_eq!(files("a*b"), ["a*b.txt"]);
    assert_eq!(files("t?"), ["what?.txt"]);
    assert_eq!(files("k\\s"), ["back\\slash.txt"]);
    assert_eq!(files("*"), ["a*b.txt"]);
}
