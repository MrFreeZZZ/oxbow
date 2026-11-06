use super::{FileCheck, Level, Repo, RepoFileKind, Tree, numbered, plural};

fn owner_ok(owner: &str, gitlab: bool) -> bool {
    let name = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    if let Some(account) = owner.strip_prefix('@') {
        if gitlab {
            return account
                .strip_prefix('@')
                .map_or_else(|| account.split('/').all(name), name);
        }
        return match account.split_once('/') {
            Some((org, team)) => name(org) && name(team),
            None => name(account) && !account.contains(['.', '_']),
        };
    }
    owner.contains('@') && !owner.starts_with('@') && owner.rsplit('@').next().is_some_and(|d| d.contains('.'))
}

impl Repo {
    pub(super) fn check_codeowners(&self, text: &str, gitlab: bool, tree: Option<&Tree>, check: &mut FileCheck) {
        let forge = if gitlab { "GitLab" } else { "GitHub" };
        let path = self.repo_file_path(RepoFileKind::Codeowners, gitlab);
        let others: Vec<&str> = RepoFileKind::Codeowners
            .places(gitlab)
            .iter()
            .copied()
            .filter(|p| *p != path && self.workdir().join(p).is_file())
            .collect();
        if let Some(other) = others.first() {
            check.warn(
                0,
                "",
                (0, 0),
                format!("`{other}` is there too; {forge} reads only `{path}`."),
                None,
            );
        }
        let mut defaults = false;
        for (no, line) in numbered(text) {
            let content = strip_owner_comment(line);
            let trimmed = content.trim();
            if trimmed.is_empty() {
                continue;
            }
            let start = content.len() - content.trim_start().len();
            let header = trimmed.strip_prefix('^').unwrap_or(trimmed);
            if header.starts_with('[') {
                if !gitlab {
                    check.error(
                        no,
                        line,
                        (start, content.trim_end().len()),
                        "Sections in `[ ]` are GitLab’s; GitHub skips this line.",
                        None,
                    );
                    continue;
                }
                let Some(close) = header.find(']') else {
                    check.error(
                        no,
                        line,
                        (start, content.trim_end().len()),
                        "The `[` is never closed.",
                        None,
                    );
                    continue;
                };
                let after = header[close + 1..].trim_start();
                let after = match after.strip_prefix('[').and_then(|a| a.split_once(']')) {
                    Some((count, rest)) if count.chars().all(|c| c.is_ascii_digit()) => rest,
                    _ => after,
                };
                defaults = !after.trim().is_empty();
                self.check_owner_words(no, line, content, after, gitlab, check);
                continue;
            }
            let pattern = trimmed.split_whitespace().next().unwrap_or("");
            let owners = &trimmed[pattern.len()..];
            if pattern.starts_with('!') {
                check.error(
                    no,
                    line,
                    (start, start + 1),
                    format!("{forge} doesn’t take `!` in CODEOWNERS and skips the line."),
                    None,
                );
                continue;
            }
            if !gitlab && (pattern.contains('[') && !pattern.contains("\\[")) {
                check.error(
                    no,
                    line,
                    (start, start + pattern.len()),
                    "GitHub doesn’t take `[ ]` ranges in CODEOWNERS and skips the line.",
                    None,
                );
                continue;
            }
            if self.check_owner_words(no, line, content, owners, gitlab, check) {
                continue;
            }
            if owners.trim().is_empty() && !defaults {
                check.note(no, "no owners", None);
                continue;
            }
            if let Some(tree) = tree {
                let files = tree
                    .tracked
                    .iter()
                    .filter(|p| crate::pulls::owner_pattern_matches(pattern, p))
                    .count();
                if files == 0 {
                    check.warn(
                        no,
                        line,
                        (start, start + pattern.len()),
                        "No committed file matches this.",
                        None,
                    );
                    check.note(no, "matches nothing", Some(Level::Warn));
                } else {
                    check.note(no, plural(files, "file", "files"), None);
                }
            }
        }
    }

    /// Errors for words that aren't owners; whether there were any.
    fn check_owner_words(
        &self,
        no: usize,
        line: &str,
        content: &str,
        owners: &str,
        gitlab: bool,
        check: &mut FileCheck,
    ) -> bool {
        let base = content.len() - content.trim_start().len()
            + content
                .trim_start()
                .find(owners.trim_start())
                .filter(|_| !owners.trim().is_empty())
                .unwrap_or(0);
        let mut bad = false;
        let mut offset = 0;
        for word in owners.trim_start().split([' ', '\t']) {
            let at = base + offset;
            offset += word.len() + 1;
            if word.is_empty() || owner_ok(word, gitlab) {
                continue;
            }
            let how = if gitlab {
                "`@user`, `@group`, `@@role`"
            } else {
                "`@login`, `@org/team`"
            };
            check.error(
                no,
                line,
                (at, at + word.len()),
                format!("`{word}` is not an owner: write {how} or an e-mail."),
                None,
            );
            bad = true;
        }
        bad
    }
}

/// A CODEOWNERS line without its comment; `\#` is a `#` in a name.
fn strip_owner_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let end = (0..bytes.len())
        .find(|&i| bytes[i] == b'#' && (i == 0 || bytes[i - 1] != b'\\'))
        .unwrap_or(line.len());
    &line[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owners() {
        assert!(owner_ok("@octocat", false) && owner_ok("@acme/core", false) && owner_ok("dev@acme.io", false));
        assert!(!owner_ok("octocat", false) && !owner_ok("@a/b/c", false));
        assert!(owner_ok("@group/sub/team", true) && owner_ok("@@developer", true));
    }
}
