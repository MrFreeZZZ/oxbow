//! Parsing a single-file diff and selecting changed lines from its hunks.

use std::collections::HashMap;

use crate::commit::{DiffLine, Hunk, LineKind, ModeChange, word_diff};

use super::fingerprint;

pub(super) struct ParsedHunk {
    /// The `@@ -a,b +c,d @@` part of the hunk's first line.
    pub(super) header: String,
    /// The hunk as it appears in the patch, header line included.
    pub(super) text: String,
    pub(super) hunk: Hunk,
}

pub(super) struct ParsedPatch {
    /// Everything before the first hunk: `diff --git`, mode and index lines, `---` and `+++`.
    pub(super) file_header: String,
    pub(super) hunks: Vec<ParsedHunk>,
    pub(super) binary: bool,
    /// `old mode` and `new mode` from the header.
    pub(super) mode: Option<ModeChange>,
    /// The blob ids of the `index` line.
    pub(super) ids: (Option<String>, Option<String>),
    /// The hunks stop short: the patch had more lines than were asked for.
    pub(super) cut: bool,
}

/// Parse the output of `git diff` for a single file.
pub(super) fn parse_patch(patch: &str) -> ParsedPatch {
    parse_patch_within(patch, usize::MAX)
}

/// [`parse_patch`], stopping once the hunks have more than `max` lines: the patch is then `cut`
/// and its hunks are only good for telling that it was too long.
pub(super) fn parse_patch_within(patch: &str, max: usize) -> ParsedPatch {
    let mut file_header = String::new();
    let mut lines = 0usize;
    let mut cut = false;
    let mut hunks: Vec<ParsedHunk> = Vec::new();
    let mut binary = false;
    let (mut old_mode, mut new_mode, mut ids) = (None, None, (None, None));
    let (mut old_no, mut new_no) = (0u32, 0u32);
    for line in patch.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        if let Some(rest) = body.strip_prefix("@@ ") {
            let Some(end) = rest.find(" @@") else { continue };
            let ranges = &rest[..end];
            let mut parts = ranges.split(' ');
            let (old_start, old_lines) = parse_range(parts.next().unwrap_or("-0").trim_start_matches('-'));
            let (new_start, new_lines) = parse_range(parts.next().unwrap_or("+0").trim_start_matches('+'));
            old_no = old_start;
            new_no = new_start;
            hunks.push(ParsedHunk {
                header: format!("@@ {ranges} @@"),
                text: line.to_owned(),
                hunk: Hunk {
                    header: format!("@@ {ranges} @@"),
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                    lines: Vec::new(),
                    check: String::new(),
                    runs: Vec::new(),
                },
            });
            continue;
        }
        let Some(current) = hunks.last_mut() else {
            if body.starts_with("Binary files ") || body == "GIT binary patch" {
                binary = true;
            } else if let Some(m) = body.strip_prefix("old mode ") {
                old_mode = Some(m.trim().to_owned());
            } else if let Some(m) = body.strip_prefix("new mode ") {
                new_mode = Some(m.trim().to_owned());
            } else if let Some((a, b)) = body
                .strip_prefix("index ")
                .and_then(|r| r.split(' ').next()?.split_once(".."))
            {
                ids = (Some(a.to_owned()), Some(b.to_owned()));
            }
            file_header.push_str(line);
            continue;
        };
        if lines >= max {
            cut = true;
            break;
        }
        current.text.push_str(line);
        let text = body.get(1..).unwrap_or_default().trim_end_matches('\r').to_owned();
        let cr = body.ends_with('\r');
        let (kind, old_line, new_line) = match body.chars().next() {
            Some('+') => {
                new_no += 1;
                (LineKind::Added, None, Some(new_no - 1))
            }
            Some('-') => {
                old_no += 1;
                (LineKind::Removed, Some(old_no - 1), None)
            }
            Some(' ') => {
                old_no += 1;
                new_no += 1;
                (LineKind::Context, Some(old_no - 1), Some(new_no - 1))
            }
            // "\ No newline at end of file" belongs to the patch but is not a line of the file.
            _ => continue,
        };
        lines += 1;
        current.hunk.lines.push(DiffLine {
            kind,
            old_line,
            new_line,
            text,
            cr,
            words: None,
            secret: None,
        });
    }
    // Changed words and the fingerprint that staging checks are for hunks that are shown.
    if !cut {
        for hunk in &mut hunks {
            mark_words(&mut hunk.hunk.lines);
            hunk.hunk.check = fingerprint(&hunk.text);
        }
        let mut seen = HashMap::new();
        for hunk in &mut hunks {
            hunk.hunk.runs = crate::changelists::runs_of(&hunk.hunk.lines, &mut seen)
                .into_iter()
                .map(|run| run.key)
                .collect();
        }
    }
    let mode = match (old_mode, new_mode) {
        (Some(old), Some(new)) => Some(ModeChange { old, new }),
        _ => None,
    };
    ParsedPatch {
        file_header,
        hunks,
        binary,
        mode,
        ids,
        cut,
    }
}

/// The hunk `text` (its `@@` line first) with only the changed lines whose indexes, counted
/// as in [`Hunk::lines`], are in `selected`, for `git apply` going `forward` or with `--reverse`.
/// git has no command for single lines; this is what `git add -p` and `e` do by hand. A changed
/// line left out is dropped when it only exists on the side the patch makes, and becomes
/// context when the side the patch applies to already has it. `None` when nothing is selected.
pub(crate) fn partial_hunk(text: &str, selected: &[usize], forward: bool) -> Option<String> {
    let mut rows = text.split_inclusive('\n');
    let head = rows.next()?;
    let rest = head.strip_prefix("@@ ")?;
    let end = rest.find(" @@")?;
    let mut ranges = rest[..end].split(' ');
    let (old_start, old_lines) = parse_range(ranges.next()?.trim_start_matches('-'));
    let (new_start, new_lines) = parse_range(ranges.next()?.trim_start_matches('+'));
    // The function name git shows after the second @@, and the line end.
    let tail = &rest[end + 3..];

    // Each line with its index, and the "\ No newline at end of file" that follows it.
    let mut parsed: Vec<(&str, Option<&str>, usize)> = Vec::new();
    for row in rows {
        match row.as_bytes().first() {
            Some(b'+' | b'-' | b' ') => parsed.push((row, None, parsed.len())),
            Some(b'\\') => {
                if let Some(last) = parsed.last_mut() {
                    last.1 = Some(row);
                }
            }
            _ => {}
        }
    }
    let mut emitted: Vec<(char, &str, bool)> = Vec::new();
    let (mut old, mut new, mut picked) = (0u32, 0u32, 0usize);
    let mut emit = |index: usize| {
        let (row, no_newline, _) = parsed[index];
        let added = row.starts_with('+');
        let kind = row.as_bytes()[0] as char;
        let kind = if row.starts_with(' ') {
            old += 1;
            new += 1;
            kind
        } else if selected.contains(&index) {
            picked += 1;
            if added {
                new += 1;
            } else {
                old += 1;
            }
            kind
        } else if added != forward {
            old += 1;
            new += 1;
            ' '
        } else {
            return;
        };
        emitted.push((kind, &row[1..], no_newline.is_some()));
    };
    // A run of changed lines goes pair by pair, the n-th removed line beside the n-th added one,
    // so the lines left out keep their places: discarding `b`/`B` alone of `-a -b +A +B`
    // leaves `A`, `b`, not `b`, `A`.
    let mut at = 0;
    while at < parsed.len() {
        if parsed[at].0.starts_with(' ') {
            emit(at);
            at += 1;
            continue;
        }
        let end = (at..parsed.len())
            .find(|&i| parsed[i].0.starts_with(' '))
            .unwrap_or(parsed.len());
        let run = &parsed[at..end];
        let removed: Vec<_> = run.iter().filter(|r| r.0.starts_with('-')).copied().collect();
        let added: Vec<_> = run.iter().filter(|r| r.0.starts_with('+')).copied().collect();
        for n in 0..removed.len().max(added.len()) {
            if let Some(&row) = removed.get(n) {
                emit(row.2);
            }
            if let Some(&row) = added.get(n) {
                emit(row.2);
            }
        }
        at = end;
    }
    if picked == 0 {
        return None;
    }
    // A retained unterminated line may now have selected lines after it. Each side's
    // marker belongs at its own EOF: leaving it on a context line would make git apply
    // concatenate the next addition. Split context when only one side needs a newline.
    let old_last = emitted.iter().rposition(|row| row.0 != '+');
    let new_last = emitted.iter().rposition(|row| row.0 != '-');
    let no_newline = |excluded: char, last: Option<usize>| {
        // A reverse patch ending in context keeps the retained line's own ending.
        // A restored old EOF before that context must not move its marker there.
        if !forward
            && excluded == '+'
            && let Some(i) = last.filter(|&i| emitted[i].0 == ' ')
        {
            return emitted[i].2;
        }
        let moved = emitted
            .iter()
            .enumerate()
            .any(|(i, row)| row.0 != excluded && row.2 && last != Some(i));
        if moved {
            // Extending the former EOF uses this side's original ending, even if
            // the last changed line on that side was left out of the selection.
            parsed
                .iter()
                .rfind(|row| !row.0.starts_with(excluded))
                .is_some_and(|row| row.1.is_some())
        } else {
            emitted.iter().any(|row| row.0 != excluded && row.2)
        }
    };
    let old_no_newline = no_newline('+', old_last);
    let new_no_newline = no_newline('-', new_last);
    let mut body = String::new();
    let mut emit_line = |kind: char, text: &str, no_newline: bool| {
        body.push(kind);
        body.push_str(text);
        if no_newline {
            body.push_str("\\ No newline at end of file\n");
        }
    };
    for (i, (kind, text, _)) in emitted.into_iter().enumerate() {
        let old_end = old_no_newline && old_last == Some(i);
        let new_end = new_no_newline && new_last == Some(i);
        if kind == ' ' && old_end != new_end {
            emit_line('-', text, old_end);
            emit_line('+', text, new_end);
        } else {
            emit_line(kind, text, if kind == '+' { new_end } else { old_end });
        }
    }
    // A side with no lines names the line before it, so the start moves by one when that changes.
    let start = |start: u32, was: u32, now: u32| match (was, now) {
        (0, n) if n > 0 => start + 1,
        (w, 0) if w > 0 => start.saturating_sub(1),
        _ => start,
    };
    Some(format!(
        "@@ -{},{} +{},{} @@{tail}{body}",
        start(old_start, old_lines, old),
        old,
        start(new_start, new_lines, new),
        new
    ))
}

fn parse_range(range: &str) -> (u32, u32) {
    match range.split_once(',') {
        Some((start, count)) => (start.parse().unwrap_or(0), count.parse().unwrap_or(0)),
        None => (range.parse().unwrap_or(0), 1),
    }
}

/// Changed words for runs of removed lines followed by the same number of added lines.
fn mark_words(lines: &mut [DiffLine]) {
    let mut i = 0;
    while i < lines.len() {
        if lines[i].kind != LineKind::Removed {
            i += 1;
            continue;
        }
        let removed_end = (i..lines.len())
            .find(|&j| lines[j].kind != LineKind::Removed)
            .unwrap_or(lines.len());
        let added_end = (removed_end..lines.len())
            .find(|&j| lines[j].kind != LineKind::Added)
            .unwrap_or(lines.len());
        let count = removed_end - i;
        if count == added_end - removed_end {
            for k in 0..count {
                let (r, a) = word_diff(&lines[i + k].text, &lines[removed_end + k].text);
                lines[i + k].words = Some(r);
                lines[removed_end + k].words = Some(a);
            }
        }
        i = added_end.max(i + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_patch_past_the_budget_is_cut() {
        let patch = "diff --git a/f b/f\nindex 1111111..2222222 100644\n--- a/f\n+++ b/f\n\
                     @@ -1,2 +1,3 @@\n a\n-b\n+c\n+d\n";
        let whole = parse_patch_within(patch, 10);
        assert!(!whole.cut);
        assert_eq!(whole.hunks[0].hunk.lines.len(), 4);
        assert!(!whole.hunks[0].hunk.check.is_empty());
        let cut = parse_patch_within(patch, 2);
        assert!(cut.cut);
        assert_eq!(cut.hunks[0].hunk.lines.len(), 2);
    }

    #[test]
    fn patch_hunks_keep_their_text_and_line_numbers() {
        let patch = "diff --git a/f b/f\nindex 1..2 100644\n--- a/f\n+++ b/f\n\
                     @@ -1,3 +1,3 @@ fn main\n a\n-b c\n+b d\n e\n\
                     @@ -10 +10,2 @@\n x\n+y\n\\ No newline at end of file\n";
        let parsed = parse_patch(patch);
        assert_eq!(
            parsed.file_header,
            "diff --git a/f b/f\nindex 1..2 100644\n--- a/f\n+++ b/f\n"
        );
        assert_eq!(parsed.hunks.len(), 2);
        assert_eq!(parsed.hunks[0].header, "@@ -1,3 +1,3 @@");
        let first = &parsed.hunks[0].hunk;
        let lines: Vec<_> = first
            .lines
            .iter()
            .map(|l| (l.kind, l.old_line, l.new_line, l.text.as_str()))
            .collect();
        assert_eq!(
            lines,
            [
                (LineKind::Context, Some(1), Some(1), "a"),
                (LineKind::Removed, Some(2), None, "b c"),
                (LineKind::Added, None, Some(2), "b d"),
                (LineKind::Context, Some(3), Some(3), "e"),
            ]
        );
        assert!(first.lines[1].words.is_some());
        let second = &parsed.hunks[1];
        assert_eq!((second.hunk.old_start, second.hunk.old_lines), (10, 1));
        assert!(second.text.ends_with("\\ No newline at end of file\n"));
        assert_eq!(second.hunk.lines.len(), 2);
    }

    #[test]
    fn picked_lines_of_a_run_keep_the_others_in_their_places() {
        let hunk = "@@ -1,3 +1,3 @@\n x\n-a\n-b\n+A\n+B\n";
        // Indexes as in Hunk::lines: x 0, -a 1, -b 2, +A 3, +B 4.
        // Staging a/A alone: b stays, after A.
        assert_eq!(
            partial_hunk(hunk, &[1, 3], true).unwrap(),
            "@@ -1,3 +1,3 @@\n x\n-a\n+A\n b\n"
        );
        // Discarding b/B alone: A stays, before b.
        assert_eq!(
            partial_hunk(hunk, &[2, 4], false).unwrap(),
            "@@ -1,3 +1,3 @@\n x\n A\n-b\n+B\n"
        );
        // More added than removed: the extra ones come after the pairs.
        let more = "@@ -1,1 +1,3 @@\n-a\n+A\n+A2\n+A3\n";
        assert_eq!(partial_hunk(more, &[3], true).unwrap(), "@@ -1,1 +1,2 @@\n a\n+A3\n");
        assert_eq!(partial_hunk(hunk, &[0], true), None, "a context line is no pick");
    }
}
