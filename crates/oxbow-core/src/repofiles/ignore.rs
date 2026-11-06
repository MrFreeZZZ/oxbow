use std::collections::HashMap;

use gix::bstr::ByteSlice;

use super::{Context, FileCheck, Fix, Level, Repo, glob_matches, numbered, plural, replace};

/// A .gitignore line as git reads it: without the spaces at its end that aren't escaped.
fn ignore_text(line: &str) -> &str {
    let mut end = line.len();
    while line[..end].ends_with(' ') {
        let before = &line[..end - 1];
        let escapes = before.len() - before.trim_end_matches('\\').len();
        if escapes % 2 == 1 {
            break;
        }
        end -= 1;
    }
    &line[..end]
}

impl Repo {
    pub(super) fn check_ignore(&self, text: &str, context: Option<&Context>, check: &mut FileCheck) {
        let tree = context.and_then(|c| c.tree.as_deref());
        let workdir = self.workdir().to_string_lossy().trim_start_matches('/').to_owned();
        let lines: Vec<(usize, &str)> = numbered(text).collect();
        // Each line's pattern, for the lines after it.
        let mut patterns: Vec<(usize, &str, gix::glob::Pattern)> = Vec::new();
        let mut seen: HashMap<&str, usize> = HashMap::new();
        for &(no, line) in &lines {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let rule = ignore_text(line);
            let mut bad = false;
            if rule.len() < line.len() && !rule.is_empty() {
                check.warn(
                    no,
                    line,
                    (rule.len(), line.len()),
                    "Git drops the spaces at the end of a line.",
                    Some(replace("Remove Spaces", no, rule)),
                );
            }
            let escapes = rule.len() - rule.trim_end_matches('\\').len();
            if escapes % 2 == 1 {
                check.error(
                    no,
                    line,
                    (rule.len() - 1, rule.len()),
                    "A `\\` at the end makes git skip the line.",
                    Some(replace("Remove \\", no, &rule[..rule.len() - 1])),
                );
                bad = true;
            }
            let body = rule.strip_prefix('!').unwrap_or(rule);
            let lead = rule.len() - body.len();
            if let Some(rest) = body.strip_prefix("./") {
                check.error(
                    no,
                    line,
                    (lead, lead + 2),
                    "Git never matches a line that starts with `./`. The top folder is `/`.",
                    Some(replace(
                        &format!("Use /{rest}"),
                        no,
                        &format!("{}/{rest}", &rule[..lead]),
                    )),
                );
                bad = true;
            } else if body.starts_with("../") {
                check.error(
                    no,
                    line,
                    (lead, lead + 3),
                    "A .gitignore can't reach outside the repository, so git never matches this.",
                    None,
                );
                bad = true;
            } else if let Some(rest) = body
                .strip_prefix('/')
                .and_then(|b| b.strip_prefix(&workdir))
                .filter(|_| !workdir.is_empty())
                && (rest.is_empty() || rest.starts_with('/'))
            {
                let inside = if rest.is_empty() {
                    "/".to_owned()
                } else {
                    rest.to_owned()
                };
                check.error(
                    no,
                    line,
                    (lead, rule.len()),
                    "This is a path on your disk. A .gitignore takes paths from the top of the repository.",
                    Some(replace(
                        &format!("Use {inside}"),
                        no,
                        &format!("{}{inside}", &rule[..lead]),
                    )),
                );
                bad = true;
            } else if body.starts_with("~/")
                || body.as_bytes().get(1..3) == Some(b":\\")
                || body.as_bytes().get(1..3) == Some(b":/")
            {
                check.error(
                    no,
                    line,
                    (lead, rule.len()),
                    "This is a path on your disk. A .gitignore takes paths from the top of the repository.",
                    None,
                );
                bad = true;
            } else if let Some(at) = windows_separator(body) {
                let fixed = body.replace('\\', "/");
                check.error(
                    no,
                    line,
                    (lead + at, lead + at + 1),
                    "`\\` keeps the next character as it is, so this matches nothing. Use `/` between folders.",
                    Some(replace(
                        &format!("Use {fixed}"),
                        no,
                        &format!("{}{fixed}", &rule[..lead]),
                    )),
                );
                bad = true;
            }
            if let Some(&first) = seen.get(rule) {
                check.warn(
                    no,
                    line,
                    (0, rule.len()),
                    format!("Line {first} says the same."),
                    Some(Fix::Replace {
                        label: "Remove Line".into(),
                        line: no,
                        text: None,
                    }),
                );
            } else {
                seen.insert(rule, no);
            }
            let Some(pattern) = gix::glob::Pattern::from_bytes(rule.as_bytes()) else {
                continue;
            };
            if pattern.is_negative() {
                self.check_negation(no, line, &pattern, &patterns, check);
            } else if (rule == "*" || rule == "/*") && !lines.iter().any(|&(n, l)| n > no && l.starts_with('!')) {
                check.warn(
                    no,
                    line,
                    (0, rule.len()),
                    "This leaves out every file in the repository.",
                    None,
                );
            }
            if let Some(tree) = tree {
                let matched = tree.ignore_matches(rule, &pattern);
                if bad {
                    if !matched.any() {
                        check.note(no, "matches nothing", Some(Level::Error));
                    }
                } else if pattern.is_negative() {
                    if matched.any() {
                        check.note(no, matched.note(tree.more()), None);
                    }
                } else if matched.any() {
                    let later: Vec<gix::glob::Pattern> = lines
                        .iter()
                        .filter(|&&(n, _)| n > no)
                        .filter_map(|&(_, l)| gix::glob::Pattern::from_bytes(ignore_text(l).as_bytes()))
                        .filter(|p| p.is_negative())
                        .collect();
                    let tracked: Vec<String> = tree
                        .tracked_in(&matched)
                        .into_iter()
                        .filter(|path| !later.iter().any(|p| glob_matches(p, path, false, tree.case())))
                        .collect();
                    if tracked.is_empty() {
                        check.note(no, matched.note(tree.more()), None);
                    } else {
                        let (what, it) = match tracked.as_slice() {
                            [one] => (format!("`{one}` is"), "it"),
                            many => (format!("{} matching files are", many.len()), "them"),
                        };
                        check.warn(
                            no,
                            line,
                            (0, rule.len()),
                            format!("{what} already committed: git keeps tracking {it} until you stop."),
                            Some(Fix::Untrack {
                                label: "Stop Tracking…".into(),
                                paths: tracked.iter().take(1000).cloned().collect(),
                            }),
                        );
                        check.note(
                            no,
                            format!("{}, already committed", plural(tracked.len(), "file", "files")),
                            Some(Level::Warn),
                        );
                    }
                } else if !pattern.has_wildcard()
                    && !rule.contains('\\')
                    && rule.trim_end_matches('/').trim_start_matches('/').contains('/')
                    && !tree.partial
                {
                    check.warn(
                        no,
                        line,
                        (0, rule.len()),
                        format!(
                            "Nothing in the repository is called `{}`.",
                            rule.trim_start_matches('/')
                        ),
                        None,
                    );
                    check.note(no, "not found", Some(Level::Warn));
                } else {
                    check.note(no, "nothing yet", None);
                }
            }
            patterns.push((no, rule, pattern));
        }
    }

    /// A `!` line: something above must leave out what it brings back, and not its folder.
    fn check_negation(
        &self,
        no: usize,
        line: &str,
        pattern: &gix::glob::Pattern,
        before: &[(usize, &str, gix::glob::Pattern)],
        check: &mut FileCheck,
    ) {
        let positive: Vec<&(usize, &str, gix::glob::Pattern)> =
            before.iter().filter(|(_, _, p)| !p.is_negative()).collect();
        if positive.is_empty() {
            check.warn(
                no,
                line,
                (0, line.trim_end().len()),
                "Nothing above leaves anything out, so `!` has nothing to bring back.",
                None,
            );
            return;
        }
        if pattern.has_wildcard() {
            return;
        }
        let path = pattern.text.to_str_lossy();
        let path = path.trim_end_matches('/');
        let mut folders = Vec::new();
        let mut at = 0;
        while let Some(slash) = path[at..].find('/') {
            folders.push(&path[..at + slash]);
            at += slash + 1;
        }
        for folder in folders {
            if let Some((other, text)) = positive
                .iter()
                .find(|(_, _, p)| glob_matches(p, folder, true, gix::glob::pattern::Case::Sensitive))
                .map(|e| (e.0, e.1))
            {
                let name = folder.rsplit('/').next().unwrap_or(folder);
                let fix = text
                    .strip_suffix('/')
                    .filter(|t| t.trim_start_matches('/') == name || t.trim_start_matches('/') == folder)
                    .map(|t| replace(&format!("Use {t}/* on line {other}"), other, &format!("{t}/*")));
                check.warn(no, line, (0, line.trim_end().len()), format!("`{text}` on line {other} leaves out the whole folder, so git can't bring back a file inside it."), fix);
                return;
            }
        }
    }
}

/// Where a `\` stands between folder names (`build\out`), as on Windows: escaping a letter or
/// a dot means nothing to git.
fn windows_separator(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let name = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-');
    (1..bytes.len().saturating_sub(1))
        .find(|&i| bytes[i] == b'\\' && (name(bytes[i - 1]) || bytes[i - 1] == b'*') && name(bytes[i + 1]))
}
