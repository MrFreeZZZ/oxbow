use super::{FileCheck, Level, Tree, numbered, plural, replace};

/// Known properties and the values each takes (besides `unset`); none: a number.
const EDITORCONFIG: &[(&str, Option<&[&str]>)] = &[
    ("indent_style", Some(&["tab", "space"])),
    ("indent_size", None),
    ("tab_width", None),
    ("end_of_line", Some(&["lf", "cr", "crlf"])),
    (
        "charset",
        Some(&["latin1", "utf-8", "utf-8-bom", "utf-16be", "utf-16le"]),
    ),
    ("trim_trailing_whitespace", Some(&["true", "false"])),
    ("insert_final_newline", Some(&["true", "false"])),
    ("max_line_length", None),
    ("spelling_language", Some(&[])),
];

/// An .editorconfig section's glob as a regular expression over paths from the top.
fn editorconfig_regex(glob: &str) -> Option<regex::Regex> {
    let anchored = glob.contains('/');
    let glob = glob.trim_start_matches('/');
    let mut out = String::from(if anchored { "^" } else { "^(?:.*/)?" });
    let chars: Vec<char> = glob.chars().collect();
    let mut i = 0;
    let mut braces = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '*' if chars.get(i + 1) == Some(&'*') => {
                out.push_str(".*");
                i += 1;
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            '[' => {
                let end = chars[i..].iter().position(|&c| c == ']')?;
                let class: String = chars[i + 1..i + end].iter().collect();
                let class = class.strip_prefix('!').map_or(class.clone(), |c| format!("^{c}"));
                out.push('[');
                out.push_str(&class.replace('\\', "\\\\"));
                out.push(']');
                i += end;
            }
            '{' => {
                let end = chars[i..].iter().position(|&c| c == '}')? + i;
                let inner: String = chars[i + 1..end].iter().collect();
                if let Some((a, b)) = inner.split_once("..")
                    && let (Ok(a), Ok(b)) = (a.parse::<i64>(), b.parse::<i64>())
                {
                    let (lo, hi) = (a.min(b), a.max(b));
                    if hi.checked_sub(lo).is_none_or(|count| count > 1000) {
                        return None;
                    }
                    out.push_str(&format!(
                        "(?:{})",
                        (lo..=hi).map(|n| n.to_string()).collect::<Vec<_>>().join("|")
                    ));
                    i = end;
                } else if inner.contains(',') {
                    out.push_str("(?:");
                    braces += 1;
                } else {
                    out.push_str(&regex::escape(&format!("{{{inner}}}")));
                    i = end;
                }
            }
            ',' if braces > 0 => out.push('|'),
            '}' if braces > 0 => {
                out.push(')');
                braces -= 1;
            }
            '\\' if i + 1 < chars.len() => {
                out.push_str(&regex::escape(&chars[i + 1].to_string()));
                i += 1;
            }
            c => out.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    out.push('$');
    regex::Regex::new(&out).ok()
}

pub(super) fn check_editorconfig(text: &str, tree: Option<&Tree>, check: &mut FileCheck) {
    let mut in_section = false;
    for (no, line) in numbered(text) {
        let trimmed = line.trim();
        let start = line.len() - line.trim_start().len();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if let Some(head) = trimmed.strip_prefix('[') {
            in_section = true;
            let Some(glob) = head.strip_suffix(']') else {
                check.error(
                    no,
                    line,
                    (start, line.trim_end().len()),
                    "The `[` is never closed.",
                    Some(replace("Add ]", no, &format!("{}]", line.trim_end()))),
                );
                continue;
            };
            if glob.matches('{').count() != glob.matches('}').count() {
                check.error(
                    no,
                    line,
                    (start, line.trim_end().len()),
                    "A `{` here is never closed, so no file matches.",
                    None,
                );
                continue;
            }
            let Some(regex) = editorconfig_regex(glob) else {
                check.error(
                    no,
                    line,
                    (start, line.trim_end().len()),
                    "Editors can’t read this section’s pattern.",
                    None,
                );
                continue;
            };
            if let Some(tree) = tree {
                let files = tree.count_files(format!("editorconfig:{glob}"), |p| regex.is_match(p));
                if files == 0 {
                    check.warn(
                        no,
                        line,
                        (start, line.trim_end().len()),
                        format!("No file matches `[{glob}]`."),
                        None,
                    );
                    check.note(no, "matches nothing", Some(Level::Warn));
                } else {
                    check.note(no, plural(files, "file", "files") + tree.more(), None);
                }
            }
            continue;
        }
        let (key, value) = match trimmed.split_once(['=', ':']) {
            Some((k, v)) => (k.trim().to_lowercase(), v.trim().to_lowercase()),
            None => {
                check.error(
                    no,
                    line,
                    (start, line.trim_end().len()),
                    "A property needs a value: `name = value`.",
                    None,
                );
                continue;
            }
        };
        let span = (start, line.trim_end().len());
        if key == "root" {
            if in_section {
                check.error(no, line, span, "`root` goes at the top, before any section.", None);
            } else if value != "true" && value != "false" {
                check.error(no, line, span, "`root` is `true` or `false`.", None);
            }
            continue;
        }
        if !in_section {
            check.error(no, line, span, "Properties go under a section, such as `[*]`.", None);
            continue;
        }
        let Some((_, values)) = EDITORCONFIG.iter().find(|(k, _)| *k == key) else {
            check.note(no, "not a standard property", None);
            continue;
        };
        if value == "unset" {
            continue;
        }
        let ok = match values {
            Some([]) => true,
            Some(values) => values.contains(&value.as_str()),
            None => {
                value.parse::<u32>().is_ok_and(|n| n > 0)
                    || (key == "indent_size" && value == "tab")
                    || (key == "max_line_length" && value == "off")
            }
        };
        if !ok {
            let takes = match values {
                Some(values) => values.iter().map(|v| format!("`{v}`")).collect::<Vec<_>>().join(", "),
                None if key == "indent_size" => "a number or `tab`".to_owned(),
                None if key == "max_line_length" => "a number or `off`".to_owned(),
                None => "a number".to_owned(),
            };
            check.error(no, line, span, format!("`{key}` takes {takes}."), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editorconfig_globs() {
        let any = editorconfig_regex("*.md").unwrap();
        assert!(any.is_match("README.md") && any.is_match("docs/a.md") && !any.is_match("a.mdx"));
        let set = editorconfig_regex("*.{js,ts}").unwrap();
        assert!(set.is_match("app/x.ts") && !set.is_match("x.rs"));
        let anchored = editorconfig_regex("lib/**.py").unwrap();
        assert!(anchored.is_match("lib/a/b.py") && !anchored.is_match("src/lib/b.py"));
    }
}
