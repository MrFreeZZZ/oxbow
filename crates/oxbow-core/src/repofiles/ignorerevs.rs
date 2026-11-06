use std::collections::HashMap;

use gix::bstr::ByteSlice;

use super::{FileCheck, Fix, Level, Repo, numbered, replace};

impl Repo {
    pub(super) fn check_ignore_revs(&self, text: &str, check: &mut FileCheck) {
        let local = self.local();
        let mut seen: HashMap<String, usize> = HashMap::new();
        for (no, line) in numbered(text) {
            // As git reads it: what follows `#` is a comment, and spaces around don't count.
            let content = line.split('#').next().unwrap_or("");
            let hash = content.trim();
            if hash.is_empty() {
                continue;
            }
            let at = content.find(hash).unwrap_or(0);
            let range = (at, at + hash.len());
            let hex = hash.bytes().all(|b| b.is_ascii_hexdigit());
            if hex && hash.len() == 40 {
                let lower = hash.to_lowercase();
                if let Some(first) = seen.get(&lower) {
                    check.warn(
                        no,
                        line,
                        range,
                        format!("Line {first} lists the same commit."),
                        Some(Fix::Replace {
                            label: "Remove Line".into(),
                            line: no,
                            text: None,
                        }),
                    );
                } else {
                    seen.insert(lower, no);
                }
                match gix::ObjectId::from_hex(hash.as_bytes())
                    .ok()
                    .and_then(|id| local.find_commit(id).ok())
                {
                    Some(commit) => check.note(no, commit_words(&commit), None),
                    None => {
                        check.warn(no, line, range, "No such commit here. Blame skips the line; the commit may be on a branch you haven’t fetched.", None);
                        check.note(no, "not in this repository", Some(Level::Warn));
                    }
                }
                continue;
            }
            let found = (hex && hash.len() >= 4 && hash.len() < 40)
                .then(|| local.rev_parse_single(hash).ok())
                .flatten()
                .and_then(|id| local.find_commit(id.detach()).ok());
            match found {
                Some(commit) => {
                    let full = commit.id.to_string();
                    check.error(
                        no,
                        line,
                        range,
                        "A short hash: `git blame` stops with an error for everyone who uses this file.",
                        Some(replace("Use Full Hash", no, &line.replacen(hash, &full, 1))),
                    );
                    check.note(no, commit_words(&commit), None);
                }
                None => check.error(
                    no,
                    line,
                    range,
                    "Not a full commit hash: `git blame` stops with an error for everyone who uses this file.",
                    None,
                ),
            }
        }
    }
}

/// A commit's summary and day, e.g. "Format all Rust code · 12 Sep 2026".
fn commit_words(commit: &gix::Commit<'_>) -> String {
    let message = commit.message_raw_sloppy();
    let summary = message
        .lines()
        .next()
        .unwrap_or_default()
        .to_str_lossy()
        .trim()
        .to_owned();
    match commit.time() {
        Ok(time) => format!("{summary} · {}", day(time.seconds)),
        Err(_) => summary,
    }
}

/// "12 Sep 2026" for seconds since 1970, in UTC.
fn day(seconds: i64) -> String {
    // Howard Hinnant's days-to-civil.
    let z = seconds.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!("{d} {} {y}", MONTHS[(m - 1) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days() {
        assert_eq!(day(0), "1 Jan 1970");
        assert_eq!(day(1_700_000_000), "14 Nov 2023");
    }
}
