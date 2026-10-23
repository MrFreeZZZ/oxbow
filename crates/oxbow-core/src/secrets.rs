//! The secrets check: before a commit, the staged lines are searched for what looks like a
//! password, token or private key, so it can be stopped before it is in history. A built-in set
//! of rules (well-known key formats, and names like `API_KEY` or `PASSWORD` that get a value)
//! works together with the user's own patterns from Settings. A line holding `oxbow:allow` (or
//! gitleaks' `gitleaks:allow`) is never flagged.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, RwLock};

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::cli::GitCommand;
use crate::error::Result;
use crate::repo::Repo;

static CHECK: AtomicBool = AtomicBool::new(true);
static OWN: RwLock<Vec<OwnRule>> = RwLock::new(Vec::new());

/// More findings than this are not listed: the sheet would be no clearer.
const MAX_FINDINGS: usize = 50;
/// Longer lines (minified code, data) are not searched.
const MAX_LINE: usize = 4000;
/// The masked line is cut to about this many characters.
const SHOWN: usize = 120;

/// An added line that looks like it holds a secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretFinding {
    pub path: String,
    /// 1-based, in the new file.
    pub line: u32,
    /// What it looks like, e.g. "GitHub token".
    pub kind: String,
    /// The line with the secret masked, e.g. `TOKEN = "ghp_••••••5Gt"`.
    pub masked: String,
}

/// A signature of the user's own, from Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretPattern {
    pub kind: PatternKind,
    pub pattern: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PatternKind {
    /// A variable or key name, `*` for any characters, matched without regard to case; a line
    /// that gives it a value is flagged.
    Name,
    /// A regular expression for the secret itself.
    Text,
}

enum OwnRule {
    Name { name: Regex, label: String },
    Text { text: Regex, label: String },
}

/// Whether to check commits, and the user's own patterns. A pattern that doesn't compile is
/// left out; Settings refuses those anyway.
pub fn set_secrets_check(on: bool, patterns: &[SecretPattern]) {
    CHECK.store(on, Ordering::Relaxed);
    let rules = patterns.iter().filter_map(compile).collect();
    *OWN.write().expect("secret patterns lock") = rules;
}

pub(crate) fn check_secrets() -> bool {
    CHECK.load(Ordering::Relaxed)
}

fn compile(pattern: &SecretPattern) -> Option<OwnRule> {
    let text = pattern.pattern.trim();
    if text.is_empty() {
        return None;
    }
    let label = format!("Matches {text}");
    match pattern.kind {
        PatternKind::Name => {
            let glob = text.split('*').map(regex::escape).collect::<Vec<_>>().join(".*");
            let name = RegexBuilder::new(&format!("^{glob}$"))
                .case_insensitive(true)
                .build()
                .ok()?;
            Some(OwnRule::Name { name, label })
        }
        PatternKind::Text => Some(OwnRule::Text {
            text: Regex::new(text).ok()?,
            label,
        }),
    }
}

/// Well-known key formats: what they are called, and the expression.
static FORMATS: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    [
        ("Private key", r"-----BEGIN (?:[A-Z]+ )*PRIVATE KEY(?: BLOCK)?-----"),
        ("AWS access key", r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b"),
        (
            "GitHub token",
            r"\bgh[pousr]_[A-Za-z0-9]{36,}\b|\bgithub_pat_[A-Za-z0-9_]{40,}",
        ),
        ("GitLab token", r"\bglpat-[A-Za-z0-9_-]{20,}"),
        ("Slack token", r"\bxox[abposr]-[A-Za-z0-9-]{10,}"),
        (
            "Slack webhook",
            r"hooks\.slack\.com/services/T[A-Za-z0-9]+/B[A-Za-z0-9]+/[A-Za-z0-9]+",
        ),
        ("Stripe key", r"\b[sr]k_(?:live|test)_[0-9A-Za-z]{16,}"),
        ("Google API key", r"\bAIza[0-9A-Za-z_-]{35}\b"),
        ("Anthropic API key", r"\bsk-ant-[A-Za-z0-9_-]{32,}"),
        ("OpenAI API key", r"\bsk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{32,}"),
        (
            "Password in URL",
            r"\b[a-zA-Z][a-zA-Z0-9+.-]*://[^\s:/@]+:([^\s:/@]{3,})@",
        ),
    ]
    .into_iter()
    .map(|(kind, re)| (kind, Regex::new(re).expect("built-in secret rule")))
    .collect()
});

/// `NAME = value`, `NAME: value`, `"NAME": "value"`, `NAME := value`, `NAME => value`; the
/// name may be quoted, and `export`, `const` and the like before it don't matter.
static ASSIGNMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:^|[^A-Za-z0-9_$])["']?([A-Za-z_$][A-Za-z0-9_.$-]*)["']?\s*(?::=|=>|=|:)\s*"#)
        .expect("assignment rule")
});

/// Names that hold a secret: API keys, secrets, tokens, passwords and credentials.
static SECRET_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)api_?key|access_?key|private_?key|secret|token|passw(?:or)?d|\bpwd\b|_pwd\b|credential")
        .expect("secret name rule")
});

/// What a secret name holds, for the sheet.
fn name_kind(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.contains("pass") || lower.contains("pwd") {
        "Password"
    } else if lower.contains("token") {
        "Token"
    } else if lower.contains("secret") || !lower.contains("key") {
        "Secret"
    } else {
        "API key"
    }
}

/// The value right after an assignment: its span in `line`, and whether it was quoted.
fn value_at(line: &str, start: usize) -> Option<(usize, usize, bool)> {
    let rest = &line[start..];
    let first = rest.chars().next()?;
    if matches!(first, '"' | '\'' | '`') {
        let body = &rest[1..];
        let end = body.find(first)?;
        return Some((start + 1, start + 1 + end, true));
    }
    let end = rest
        .find(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '#' | ')' | ']' | '}'))
        .unwrap_or(rest.len());
    (end > 0).then_some((start, start + end, false))
}

/// A value that is clearly not a secret: empty, a placeholder like `***`, `<token>` or
/// `${TOKEN}`, or code that reads it from somewhere else, like `os.getenv(...)`.
fn placeholder(value: &str, quoted: bool) -> bool {
    let v = value.trim();
    if v.is_empty()
        || v.chars()
            .all(|c| matches!(c, '*' | 'x' | 'X' | '.' | '-' | '_' | '•' | '?' | '#' | '0'))
    {
        return true;
    }
    let lower = v.to_ascii_lowercase();
    if [
        "null",
        "none",
        "nil",
        "true",
        "false",
        "undefined",
        "changeme",
        "example",
        "placeholder",
        "your_token_here",
    ]
    .contains(&lower.as_str())
        || lower.starts_with("your")
    {
        return true;
    }
    if v.contains("${") || v.contains("{{") || v.starts_with('$') || v.starts_with('<') || v.starts_with('%') {
        return true;
    }
    // Unquoted, an identifier or a path to one is code: `settings.API_KEY`, `getToken(`, `String`.
    !quoted
        && (v.contains(['(', '['])
            || v.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == ':')
                && v.contains(['.', ':'])
            || v.chars().all(|c| c.is_ascii_alphabetic() || c == '_') && v.len() < 12)
}

/// A value worth flagging under a built-in secret name: long enough, one word, and with a
/// digit or long enough to be random rather than a word or a sentence.
fn looks_random(value: &str) -> bool {
    let v = value.trim();
    v.chars().count() >= 8
        && !v.contains(char::is_whitespace)
        && (v.chars().any(|c| c.is_ascii_digit()) || v.len() >= 20)
}

/// What on `line` looks like a secret: what it is, and the byte span to mask.
pub(crate) fn check_line(line: &str) -> Option<(String, (usize, usize))> {
    check_with(line, &OWN.read().expect("secret patterns lock"))
}

fn check_with(line: &str, own: &[OwnRule]) -> Option<(String, (usize, usize))> {
    if line.len() > MAX_LINE || line.contains("oxbow:allow") || line.contains("gitleaks:allow") {
        return None;
    }
    scan(line, own, false).into_iter().next()
}

/// The spans of everything on `line` that looks like a secret, for the Console to hide: unlike
/// the check before a commit, a long line or one marked `oxbow:allow` is no reason to keep one.
pub(crate) fn secret_spans(line: &str) -> Vec<(usize, usize)> {
    scan(line, &OWN.read().expect("secret patterns lock"), true)
        .into_iter()
        .map(|(_, span)| span)
        .collect()
}

/// What on `line` looks like a secret: only the first thing found, or with `all` everything.
fn scan(line: &str, own: &[OwnRule], all: bool) -> Vec<(String, (usize, usize))> {
    let mut found = Vec::new();
    macro_rules! found {
        ($kind:expr, $span:expr) => {{
            found.push(($kind, $span));
            if !all {
                return found;
            }
        }};
    }
    for rule in own {
        if let OwnRule::Text { text, label } = rule {
            for m in text.find_iter(line).filter(|m| !m.as_str().trim().is_empty()) {
                found!(label.clone(), (m.start(), m.end()));
            }
        }
    }
    for (kind, re) in FORMATS.iter() {
        for caps in re.captures_iter(line) {
            // A password in a URL masks only the password.
            let m = caps.get(1).unwrap_or_else(|| caps.get(0).expect("whole match"));
            if *kind == "Password in URL" && placeholder(m.as_str(), true) {
                continue;
            }
            found!(kind.to_string(), (m.start(), m.end()));
        }
    }
    for caps in ASSIGNMENT.captures_iter(line) {
        let name = caps.get(1).expect("name");
        let Some((mut start, mut end, mut quoted)) = value_at(line, caps.get(0).expect("whole match").end()) else {
            continue;
        };
        // A type between the name and the value: `TOKEN: &str = "…"`, `token: string = '…'`.
        if !quoted {
            let after = line[end..].trim_start();
            if let Some(rest) = after.strip_prefix('=').filter(|r| !r.starts_with(['=', '>'])) {
                let at = line.len() - rest.trim_start().len();
                if let Some(value) = value_at(line, at) {
                    (start, end, quoted) = value;
                }
            }
        }
        let value = &line[start..end];
        let bare = name.as_str().rsplit('.').next().unwrap_or(name.as_str());
        let own_name = own.iter().find_map(|rule| match rule {
            OwnRule::Name { name: re, label }
                if (re.is_match(name.as_str()) || re.is_match(bare)) && !placeholder(value, quoted) =>
            {
                Some(label.clone())
            }
            _ => None,
        });
        if let Some(label) = own_name {
            found!(label, (start, end));
        } else if SECRET_NAME.is_match(bare) && !placeholder(value, quoted) && looks_random(value) {
            found!(name_kind(bare).to_owned(), (start, end));
        }
    }
    found
}

/// `line` with the secret in `span` replaced by dots, keeping a few characters at each end so
/// it can be recognized, and cut to a readable length.
fn mask(line: &str, (start, end): (usize, usize)) -> String {
    let secret: Vec<char> = line[start..end].chars().collect();
    let n = secret.len();
    let (head, tail) = match n {
        0..=5 => (0, 0),
        6..=11 => (1, 1),
        12..=19 => (3, 2),
        _ => (4, 3),
    };
    let hidden = (n - head - tail).min(32);
    let masked: String = secret[..head]
        .iter()
        .chain(std::iter::repeat_n(&'•', hidden.max(3)))
        .chain(&secret[n - tail..])
        .collect();
    let whole = format!("{}{}{}", &line[..start], masked, &line[end..]);
    let trimmed = whole.trim();
    if trimmed.chars().count() <= SHOWN {
        trimmed.to_owned()
    } else {
        trimmed.chars().take(SHOWN - 1).chain(['…']).collect()
    }
}

/// The findings in a patch: each added line that looks like it holds a secret.
pub(crate) fn scan_patch(patch: &str) -> Vec<SecretFinding> {
    let mut found = Vec::new();
    let mut path: Option<String> = None;
    let mut line = 0u32;
    // Between `diff --git` and the first `@@`: an added line `++ x` reads `+++ x` too.
    let mut header = false;
    for text in patch.lines() {
        if text.starts_with("diff --git ") {
            path = None;
            header = true;
        } else if header && let Some(rest) = text.strip_prefix("+++ ") {
            path = crate::worktree::header_path(rest).and_then(|p| p.strip_prefix("b/").map(str::to_owned));
        } else if header && !text.starts_with("@@ ") {
            continue;
        } else if let Some(hunk) = text.strip_prefix("@@ ") {
            header = false;
            // @@ -a,b +c,d @@
            line = hunk
                .split_whitespace()
                .find_map(|part| part.strip_prefix('+'))
                .and_then(|range| range.split(',').next())
                .and_then(|start| start.parse().ok())
                .unwrap_or(1);
        } else if let Some(added) = text.strip_prefix('+') {
            if let Some(path) = &path
                && let Some((kind, span)) = check_line(added.strip_suffix('\r').unwrap_or(added))
            {
                found.push(SecretFinding {
                    path: path.clone(),
                    line,
                    kind,
                    masked: mask(added, span),
                });
                if found.len() >= MAX_FINDINGS {
                    break;
                }
            }
            line += 1;
        } else if text.starts_with(' ') {
            line += 1;
        }
    }
    found
}

impl Repo {
    /// What looks like a secret in the lines the next commit adds.
    pub fn staged_secrets(&self) -> Result<Vec<SecretFinding>> {
        self.staged_secrets_in(&[])
    }

    /// The same for the staged lines of `paths` only; all of them without any.
    pub(crate) fn staged_secrets_in(&self, paths: &[String]) -> Result<Vec<SecretFinding>> {
        let mut args: Vec<String> = [
            "diff",
            "--cached",
            "--no-color",
            "--no-ext-diff",
            "--no-renames",
            "--text",
            "-U0",
            // Whatever diff.noprefix or diff.mnemonicPrefix say, the paths read `b/…`.
            "--src-prefix=a/",
            "--dst-prefix=b/",
        ]
        .map(str::to_owned)
        .into();
        if !paths.is_empty() {
            args.push("--".to_owned());
            args.extend(paths.iter().map(|p| crate::cli::literal(p)));
        }
        // The Console keeps the command, not the lines it read: they are what is checked.
        let patch = crate::console::without_output(|| self.run(&GitCommand::new(args)))?.stdout;
        Ok(scan_patch(&patch))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fake keys, split so that secret scanners don't take this file for a leak.
    const AWS: &str = concat!("AKIA", "Q7XKZ3M4P2LJ6WVB");
    const GITHUB: &str = concat!("gh", "p_R8mZ2kQx7LwP4vTnY9cB3hJd6sFa1EoUi5Gt");
    const STRIPE: &str = concat!("sk_", "live_4eC39HqLyjWDarjtT1zdp7dc");

    fn kind(line: &str) -> Option<String> {
        check_with(line, &[]).map(|(kind, _)| kind)
    }

    #[test]
    fn finds_own_patterns() {
        let own: Vec<OwnRule> = [
            SecretPattern {
                kind: PatternKind::Name,
                pattern: "CLIENT_ID".into(),
            },
            SecretPattern {
                kind: PatternKind::Name,
                pattern: "park_*".into(),
            },
            SecretPattern {
                kind: PatternKind::Text,
                pattern: r"acme_[0-9a-f]{8}".into(),
            },
        ]
        .iter()
        .filter_map(compile)
        .collect();
        let kind = |line: &str| check_with(line, &own).map(|(kind, _)| kind);
        assert_eq!(kind(r#"CLIENT_ID = "4471""#).as_deref(), Some("Matches CLIENT_ID"));
        assert_eq!(kind("self.client_id: 4471").as_deref(), Some("Matches CLIENT_ID"));
        assert_eq!(kind(r#"PARK_ID = "88""#).as_deref(), Some("Matches park_*"));
        assert_eq!(
            kind("let id = acme_0badf00d;").as_deref(),
            Some("Matches acme_[0-9a-f]{8}")
        );
        assert_eq!(kind(r#"CLIENT_ID = "***""#), None);
        assert_eq!(kind("CLIENT_ID = os.environ[\"CLIENT_ID\"]"), None);
    }

    #[test]
    fn finds_well_known_formats() {
        assert_eq!(
            kind(&format!("AWS_ACCESS_KEY_ID={AWS}")).as_deref(),
            Some("AWS access key")
        );
        assert_eq!(
            kind(&format!(r#"const T = "{GITHUB}";"#)).as_deref(),
            Some("GitHub token")
        );
        assert_eq!(
            kind("-----BEGIN OPENSSH PRIVATE KEY-----").as_deref(),
            Some("Private key")
        );
        assert_eq!(
            kind("url = postgres://app:s3cr3tpw@db:5432/app").as_deref(),
            Some("Password in URL")
        );
        assert_eq!(kind(&format!("stripe = '{STRIPE}'")).as_deref(), Some("Stripe key"));
    }

    #[test]
    fn finds_secret_names_with_a_value() {
        assert_eq!(kind(r#"API_KEY_V7 = "a8f3k29dk3m1""#).as_deref(), Some("API key"));
        assert_eq!(kind("DB_PASSWORD=hunter2hunter2").as_deref(), Some("Password"));
        assert_eq!(
            kind(r#"  "clientSecret": "9f8e7d6c5b4a3210","#).as_deref(),
            Some("Secret")
        );
        assert_eq!(
            kind(concat!(
                "AWS_SECRET_",
                "ACCESS_KEY=wJalrXUtnFEMI/K7MDENG/bPxRfiCYzEXAMPLEKEY"
            ))
            .as_deref(),
            Some("Secret")
        );
    }

    #[test]
    fn leaves_code_and_placeholders_alone() {
        for line in [
            "API_KEY = os.getenv(\"API_KEY\")",
            "token = settings.API_TOKEN",
            "password: String,",
            "let token: &str = &args.token;",
            r#"PASSWORD = "***""#,
            r#"API_KEY = "<your key>""#,
            "SECRET_KEY=${SECRET_KEY}",
            r#"label = "Enter your password here""#,
            r#"API_KEY = "a8f3k29dk3m1" # oxbow:allow"#,
            "primary_key = 42",
            "url = https://example.com:8080/path",
        ] {
            assert_eq!(kind(line), None, "{line}");
        }
    }

    #[test]
    fn masks_the_secret() {
        let line = format!(r#"TOKEN = "{GITHUB}""#);
        let (_, span) = check_with(&line, &[]).unwrap();
        let masked = mask(&line, span);
        assert!(masked.starts_with(r#"TOKEN = "ghp_••"#), "{masked}");
        assert!(masked.ends_with(r#"5Gt""#), "{masked}");
        assert!(!masked.contains("R8mZ2kQx7"));
    }

    #[test]
    fn reads_paths_and_line_numbers_from_a_patch() {
        let patch = &format!(
            "diff --git a/config/deploy.env b/config/deploy.env\n\
                     new file mode 100644\n\
                     --- /dev/null\n\
                     +++ b/config/deploy.env\n\
                     @@ -0,0 +1,3 @@\n\
                     +REGION=eu-central-1\n\
                     +AWS_ACCESS_KEY_ID={AWS}\n\
                     +BUCKET=staging\n\
                     diff --git a/src/config.rs b/src/config.rs\n\
                     --- a/src/config.rs\n\
                     +++ b/src/config.rs\n\
                     @@ -20,0 +21,2 @@ fn x()\n\
                     +// release script\n\
                     +pub const DEPLOY_TOKEN: &str = \"d9f8a7s6d5f4g3h2\";\n"
        );
        let found = scan_patch(patch);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(
            (found[0].path.as_str(), found[0].line),
            ("config/deploy.env", 2),
            "{found:?}"
        );
        assert_eq!(
            (found[1].path.as_str(), found[1].line, found[1].kind.as_str()),
            ("src/config.rs", 22, "Token")
        );
    }
}
