use std::collections::HashMap;

use super::{Context, FileCheck, Repo, glob_matches, numbered, plural, replace};

/// Diff drivers git has built in (userdiff.c).
const BUILTIN_DIFF: &[&str] = &[
    "ada", "bash", "bibtex", "cpp", "csharp", "css", "dts", "elixir", "fortran", "fountain", "golang", "html", "java",
    "kotlin", "markdown", "matlab", "objc", "pascal", "perl", "php", "python", "r", "ruby", "rust", "scheme", "tex",
];
const BUILTIN_MERGE: &[&str] = &["text", "binary", "union"];

fn attribute_name_ok(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_'))
}

impl Repo {
    pub(super) fn check_attributes(&self, text: &str, context: Option<&Context>, check: &mut FileCheck) {
        let tree = context.and_then(|c| c.tree.as_deref());
        let config = self.local().config_snapshot().clone();
        let has = |key: String| config.string(key.as_str()).is_some();
        // pattern → attribute → (state, line)
        let mut set: HashMap<String, HashMap<String, (String, usize)>> = HashMap::new();
        for (no, line) in numbered(text) {
            let start = line.len() - line.trim_start_matches([' ', '\t']).len();
            let rest = &line[start..];
            if rest.trim().is_empty() || rest.starts_with('#') {
                continue;
            }
            let (pattern, after) = if let Some(quoted) = rest.strip_prefix('"') {
                match quoted.find('"') {
                    Some(end) => (&rest[..end + 2], &rest[end + 2..]),
                    None => {
                        check.error(
                            no,
                            line,
                            (start, line.len()),
                            "The `\"` is never closed, so git skips the line.",
                            None,
                        );
                        continue;
                    }
                }
            } else {
                let end = rest.find([' ', '\t']).unwrap_or(rest.len());
                (&rest[..end], &rest[end..])
            };
            let after_at = line.len() - after.len();
            let is_macro = pattern.starts_with("[attr]");
            if let Some(name) = pattern.strip_prefix("[attr]")
                && !attribute_name_ok(name)
            {
                check.error(
                    no,
                    line,
                    (start, start + pattern.len()),
                    format!("`{name}` is not a name git takes for a macro, so it skips the line."),
                    None,
                );
                continue;
            }
            if pattern.starts_with('!') {
                check.error(no, line, (start, start + 1), "Git doesn't take `!` in .gitattributes and skips this line. To match a name that starts with `!`, write `\\!`.", None);
                continue;
            }
            let mut attrs: Vec<(&str, String, usize)> = Vec::new();
            let mut offset = after_at;
            let mut broken = false;
            for word in after.split([' ', '\t', '\r']) {
                let at = offset;
                offset += word.len() + 1;
                if word.is_empty() {
                    continue;
                }
                let (name, state) = if let Some(n) = word.strip_prefix('-') {
                    (n, "-".to_owned())
                } else if let Some(n) = word.strip_prefix('!') {
                    (n, "!".to_owned())
                } else if let Some((n, v)) = word.split_once('=') {
                    (n, format!("={v}"))
                } else {
                    (word, "+".to_owned())
                };
                if !attribute_name_ok(name) || name.starts_with("builtin_") {
                    check.error(
                        no,
                        line,
                        (at, at + word.len()),
                        format!("`{name}` is not an attribute name git takes, so it skips the line."),
                        None,
                    );
                    broken = true;
                    continue;
                }
                attrs.push((name, state, at));
            }
            if broken {
                continue;
            }
            if attrs.is_empty() {
                if !is_macro {
                    check.warn(
                        no,
                        line,
                        (start, start + pattern.len()),
                        "A pattern alone sets nothing.",
                        None,
                    );
                }
                continue;
            }
            for (name, state, at) in &attrs {
                let value = state.strip_prefix('=');
                let span = (*at, *at + name.len() + state.len() - usize::from(state == "+"));
                match (*name, value) {
                    ("eol", Some(v)) if v != "lf" && v != "crlf" => {
                        check.error(
                            no,
                            line,
                            span,
                            format!("`eol={v}` is not `lf` or `crlf`, so git skips that setting."),
                            None,
                        );
                    }
                    ("text", Some(v)) if v != "auto" => {
                        check.error(
                            no,
                            line,
                            span,
                            format!("`text={v}`: git takes `text`, `-text` or `text=auto`."),
                            None,
                        );
                    }
                    ("working-tree-encoding", Some("")) => {
                        check.error(
                            no,
                            line,
                            span,
                            "`working-tree-encoding=` needs an encoding, such as `UTF-16`.",
                            None,
                        );
                    }
                    ("diff", Some(d))
                        if !BUILTIN_DIFF.contains(&d)
                            && ![
                                "textconv",
                                "command",
                                "xfuncname",
                                "funcname",
                                "binary",
                                "wordregex",
                                "cachetextconv",
                            ]
                            .iter()
                            .any(|k| has(format!("diff.{d}.{k}")))
                            && d != "lfs" =>
                    {
                        check.warn(
                            no,
                            line,
                            span,
                            format!("No `diff.{d}` in git’s settings here, so git diffs these files as text."),
                            None,
                        );
                    }
                    ("merge", Some(m))
                        if !BUILTIN_MERGE.contains(&m) && !has(format!("merge.{m}.driver")) && m != "lfs" =>
                    {
                        check.warn(
                            no,
                            line,
                            span,
                            format!("No `merge.{m}.driver` in git’s settings here, so git merges these files as text."),
                            None,
                        );
                    }
                    ("filter", Some("lfs")) => {
                        if self.lfs_version().is_none() {
                            check.warn(
                                no,
                                line,
                                span,
                                "Git LFS is not installed here, so these files are committed as they are.",
                                None,
                            );
                        }
                    }
                    ("filter", Some(f))
                        if !["clean", "smudge", "process"]
                            .iter()
                            .any(|k| has(format!("filter.{f}.{k}"))) =>
                    {
                        check.warn(
                            no,
                            line,
                            span,
                            format!("No `filter.{f}` in git’s settings here, so git stores these files as they are."),
                            None,
                        );
                    }
                    _ => {}
                }
            }
            if attrs.iter().any(|(n, s, _)| *n == "filter" && s == "=lfs") {
                let missing: Vec<&str> = [
                    ("diff", "=lfs", "diff=lfs"),
                    ("merge", "=lfs", "merge=lfs"),
                    ("text", "-", "-text"),
                ]
                .into_iter()
                .filter(|(n, s, _)| !attrs.iter().any(|(an, as_, _)| an == n && as_ == s))
                .map(|(_, _, w)| w)
                .collect();
                if !missing.is_empty() {
                    let fixed = format!("{} {}", line.trim_end(), missing.join(" "));
                    check.warn(
                        no,
                        line,
                        (start, line.trim_end().len()),
                        format!(
                            "Git LFS rules also say {}, so diffs and merges leave these files alone.",
                            missing.iter().map(|m| format!("`{m}`")).collect::<Vec<_>>().join(", ")
                        ),
                        Some(replace(&format!("Add {}", missing.join(" ")), no, &fixed)),
                    );
                }
            }
            if !is_macro {
                let known = set.entry(pattern.to_owned()).or_default();
                for (name, state, at) in &attrs {
                    match known.get(*name) {
                        Some((before, other)) if before != state => {
                            check.warn(
                                no,
                                line,
                                (*at, *at + name.len()),
                                format!("Line {other} sets `{name}` for the same files another way; this line wins."),
                                None,
                            );
                        }
                        _ => {}
                    }
                    known.insert((*name).to_owned(), (state.clone(), no));
                }
                if let Some(tree) = tree {
                    let unquoted = pattern.trim_matches('"');
                    if let Some(glob) = gix::glob::Pattern::from_bytes(unquoted.as_bytes()) {
                        let files = tree.count_files(format!("attributes:{unquoted}"), |path| {
                            glob_matches(&glob, path, false, tree.case())
                        });
                        if unquoted.ends_with('/') {
                            check.warn(no, line, (start, start + pattern.len()), "In .gitattributes a pattern that ends with `/` matches nothing. To cover a folder, write `folder/**`.", None);
                        } else if files == 0 {
                            check.note(no, "nothing yet", None);
                        } else {
                            check.note(no, plural(files, "file", "files") + tree.more(), None);
                        }
                    }
                }
            }
        }
    }
}
