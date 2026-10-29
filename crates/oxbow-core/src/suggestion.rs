//! Apply Suggestion: the lines a reviewer suggested in a pull request, put in place of the lines
//! they were written on, in the file on disk. It is a patch of the file as the commit has it,
//! which git applies where those lines are now, or merges in when the file changed around them.

use crate::cli::GitCommand;
use crate::error::{Error, Result};
use crate::oplog::short_id;
use crate::repo::Repo;

/// Lines kept around the suggested ones, as in any patch: git finds the place by them.
const CONTEXT: usize = 3;

impl Repo {
    pub(crate) fn plan_apply_suggestion(
        &self,
        commit: &str,
        path: &str,
        start: u32,
        end: u32,
        lines: &str,
    ) -> Result<Vec<GitCommand>> {
        let sha = short_id(commit);
        let (old_id, text) = self.text_at(commit, path)?;
        // The blobs before and after, so git can merge it in when the lines moved on.
        let new_id = suggested(&text, start as usize, end as usize, lines).and_then(|new| {
            gix::objs::compute_hash(self.local().object_hash(), gix::object::Kind::Blob, new.as_bytes()).ok()
        });
        let index = new_id.map(|new| format!("index {old_id}..{new}"));
        let patch =
            suggestion_patch(path, &text, start as usize, end as usize, lines, index.as_deref()).ok_or_else(|| {
                Error::Git(format!(
                    "{path} in {sha} has no lines {start}–{end}: the suggestion is on lines that are gone"
                ))
            })?;
        if patch.is_empty() {
            return Err(Error::Git(format!("{path} already has the suggested lines in {sha}")));
        }
        self.plan_patch_on_disk(
            vec!["apply"],
            patch,
            path,
            &crate::edit::OnDisk {
                fits: format!("the suggested lines go into {path} on disk, in place of lines {start}–{end} of {sha}; nothing is staged"),
                since: format!("{path} changed since {sha}, where the suggestion was made"),
                merged: format!("--3way: {path} changed since {sha}, so the suggestion is merged in; the merged file is staged"),
                instead: "Change the file by hand".into(),
            },
        )
    }

    /// The file `path` as `commit` has it: its blob, and its text.
    fn text_at(&self, commit: &str, path: &str) -> Result<(gix::ObjectId, String)> {
        let repo = self.local();
        let missing = || {
            Error::Git(format!(
                "{} isn’t here, or has no {path}: fetch the pull request first",
                short_id(commit)
            ))
        };
        let id = repo
            .rev_parse_single(format!("{commit}:{path}").as_str())
            .map_err(|_| missing())?;
        let object = repo.find_object(id).map_err(|_| missing())?;
        if object.kind != gix::object::Kind::Blob {
            return Err(missing());
        }
        let text = String::from_utf8(object.detach().data).map_err(|_| {
            Error::Git(format!(
                "{path} isn’t UTF-8 text, so a suggestion can’t go into it from here"
            ))
        })?;
        Ok((id.detach(), text))
    }
}

/// The lines that go in place of lines `start` to `end` (counted from 1) of the file's `old`
/// lines: `lines`, one per line without the last line's end, ending the way the file's lines end,
/// `\r\n` or `\n`. None when the file has no such lines.
fn new_lines(old: &[&str], start: usize, end: usize, lines: &str) -> Option<Vec<String>> {
    if start == 0 || end < start || end > old.len() {
        return None;
    }
    let eol = if old[start - 1].ends_with("\r\n") { "\r\n" } else { "\n" };
    // A suggestion of nothing takes the lines out.
    let mut new: Vec<String> = if lines.is_empty() {
        Vec::new()
    } else {
        lines
            .split('\n')
            .map(|l| format!("{}{eol}", l.strip_suffix('\r').unwrap_or(l)))
            .collect()
    };
    // The file's last line without an end stays without one.
    if end == old.len()
        && !old[end - 1].ends_with('\n')
        && let Some(last) = new.last_mut()
    {
        last.truncate(last.len() - eol.len());
    }
    Some(new)
}

/// The whole file once the suggestion is in, or None when it has no such lines.
fn suggested(text: &str, start: usize, end: usize, lines: &str) -> Option<String> {
    let old: Vec<&str> = text.split_inclusive('\n').collect();
    let new = new_lines(&old, start, end, lines)?;
    Some(old[..start - 1].concat() + &new.concat() + &old[end..].concat())
}

/// The patch that puts `lines` in place of lines `start` to `end` of `text`, with `index` (the
/// blobs before and after) under its first line when known; empty when they are already there,
/// and None when the file has no such lines.
pub fn suggestion_patch(
    path: &str,
    text: &str,
    start: usize,
    end: usize,
    lines: &str,
    index: Option<&str>,
) -> Option<String> {
    let old: Vec<&str> = text.split_inclusive('\n').collect();
    let new = new_lines(&old, start, end, lines)?;
    if new.iter().map(String::as_str).eq(old[start - 1..end].iter().copied()) {
        return Some(String::new());
    }
    let first = start.saturating_sub(CONTEXT).max(1);
    let last = (end + CONTEXT).min(old.len());
    let before = &old[first - 1..start - 1];
    let after = &old[end..last];
    let old_count = last - first + 1;
    let new_count = before.len() + new.len() + after.len();
    // A hunk that leaves no lines starts at the line before it, 0 at the top.
    let new_start = if new_count == 0 { first - 1 } else { first };
    // A quoted name keeps a/ and b/ inside its quotes.
    let name = quoted(path);
    let (a, b) = match name.strip_prefix('"') {
        Some(inner) => (format!("\"a/{inner}"), format!("\"b/{inner}")),
        None => (format!("a/{name}"), format!("b/{name}")),
    };
    let index = index.map(|line| format!("{line}\n")).unwrap_or_default();
    let mut patch =
        format!("diff --git {a} {b}\n{index}--- {a}\n+++ {b}\n@@ -{first},{old_count} +{new_start},{new_count} @@\n");
    let mut push = |mark: char, line: &str| {
        patch.push(mark);
        patch.push_str(line);
        if !line.ends_with('\n') {
            patch.push_str("\n\\ No newline at end of file\n");
        }
    };
    for line in before {
        push(' ', line);
    }
    for line in &old[start - 1..end] {
        push('-', line);
    }
    for line in &new {
        push('+', line);
    }
    for line in after {
        push(' ', line);
    }
    Some(patch)
}

/// A path as a patch names it: in quotes, with C escapes, when it holds a quote, a backslash or
/// a control character; as it is otherwise.
fn quoted(path: &str) -> String {
    if !path.chars().any(|c| c == '"' || c == '\\' || c.is_control()) {
        return path.to_owned();
    }
    let mut out = String::from("\"");
    for c in path.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\{:03o}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEN: &str = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";

    #[test]
    fn the_suggested_lines_replace_the_ones_commented_on_with_three_lines_around() {
        let patch = suggestion_patch("a.txt", TEN, 5, 6, "five\nsix\nmore", None).unwrap();
        assert_eq!(
            patch,
            "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -2,8 +2,9 @@\n 2\n 3\n 4\n-5\n-6\n+five\n+six\n+more\n 7\n 8\n 9\n"
        );
    }

    #[test]
    fn the_first_and_last_lines_have_fewer_around_them() {
        let patch = suggestion_patch("a.txt", TEN, 1, 1, "one", None).unwrap();
        assert!(patch.contains("@@ -1,4 +1,4 @@\n-1\n+one\n 2\n"), "{patch}");
        let patch = suggestion_patch("a.txt", TEN, 10, 10, "ten", None).unwrap();
        assert!(patch.contains("@@ -7,4 +7,4 @@\n 7\n 8\n 9\n-10\n+ten\n"), "{patch}");
    }

    #[test]
    fn an_empty_suggestion_takes_the_lines_out() {
        let patch = suggestion_patch("a.txt", "x\n", 1, 1, "", None).unwrap();
        assert!(patch.ends_with("@@ -1,1 +0,0 @@\n-x\n"), "{patch}");
    }

    #[test]
    fn a_last_line_without_an_end_keeps_none() {
        let patch = suggestion_patch("a.txt", "a\nb", 2, 2, "B", None).unwrap();
        assert!(
            patch.ends_with(" a\n-b\n\\ No newline at end of file\n+B\n\\ No newline at end of file\n"),
            "{patch}"
        );
    }

    #[test]
    fn lines_of_a_crlf_file_end_with_crlf() {
        let patch = suggestion_patch("a.txt", "a\r\nb\r\n", 2, 2, "B\r\nC", None).unwrap();
        assert!(patch.ends_with("-b\r\n+B\r\n+C\r\n"), "{patch:?}");
    }

    #[test]
    fn lines_the_file_has_not_got_or_already_has_make_no_patch() {
        assert_eq!(suggestion_patch("a.txt", TEN, 10, 11, "x", None), None);
        assert_eq!(suggestion_patch("a.txt", TEN, 0, 1, "x", None), None);
        assert_eq!(suggestion_patch("a.txt", TEN, 3, 2, "x", None), None);
        assert_eq!(suggestion_patch("a.txt", TEN, 2, 3, "2\n3", None).as_deref(), Some(""));
    }

    #[test]
    fn the_file_after_has_the_suggestion_and_the_patch_names_both_blobs() {
        assert_eq!(suggested("a\nb\nc", 2, 3, "B\nC").as_deref(), Some("a\nB\nC"));
        assert_eq!(suggested("a\n", 2, 2, "x"), None);
        let patch = suggestion_patch("a.txt", "a\n", 1, 1, "b", Some("index 1111..2222")).unwrap();
        assert!(
            patch.starts_with("diff --git a/a.txt b/a.txt\nindex 1111..2222\n--- a/a.txt\n"),
            "{patch}"
        );
    }

    #[test]
    fn a_name_with_a_quote_is_quoted() {
        assert_eq!(quoted("a b/c.txt"), "a b/c.txt");
        assert_eq!(quoted("say \"hi\".txt"), "\"say \\\"hi\\\".txt\"");
        let patch = suggestion_patch("say \"hi\".txt", "x\n", 1, 1, "y", None).unwrap();
        assert!(
            patch.starts_with("diff --git \"a/say \\\"hi\\\".txt\" \"b/say \\\"hi\\\".txt\"\n"),
            "{patch}"
        );
    }
}
