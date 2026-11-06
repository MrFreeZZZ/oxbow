use std::collections::{HashMap, HashSet};

use gix::bstr::ByteSlice;

use super::{Context, FileCheck, Level, MAX_COMMITS, Repo, RepoFileKind, Result, numbered, plural, replace};

/// Who wrote and committed the history: name and e-mail as written, with their commits.
#[derive(Default)]
pub(super) struct People {
    /// (name, e-mail) → commits.
    ids: HashMap<(String, String), usize>,
}

impl People {
    pub(super) fn read(repo: &Repo) -> Result<People> {
        let mut ids: HashMap<(String, String), usize> = HashMap::new();
        let local = repo.local();
        let mut tips: Vec<gix::ObjectId> = repo
            .refs()
            .unwrap_or_default()
            .iter()
            .filter_map(|r| gix::ObjectId::from_hex(r.target.as_bytes()).ok())
            .collect();
        if let Some(head) = repo
            .head()
            .ok()
            .and_then(|h| h.commit)
            .and_then(|h| gix::ObjectId::from_hex(h.as_bytes()).ok())
        {
            tips.push(head);
        }
        tips.sort();
        tips.dedup();
        if tips.is_empty() {
            return Ok(People { ids });
        }
        let Ok(walk) = local.rev_walk(tips).all() else {
            return Ok(People { ids });
        };
        for (n, info) in walk.take(MAX_COMMITS).flatten().enumerate() {
            if n % 1024 == 0 {
                crate::cli::check_stop()?;
            }
            let Ok(commit) = local.find_commit(info.id) else {
                continue;
            };
            let mut seen = |sig: gix::actor::SignatureRef<'_>, count: usize| {
                let key = (
                    sig.name.to_str_lossy().trim().to_owned(),
                    sig.email.to_str_lossy().trim().to_owned(),
                );
                *ids.entry(key).or_default() += count;
            };
            if let Ok(author) = commit.author() {
                seen(author, 1);
            }
            // A committer is someone the map should know too, but the commits are the author's.
            if let Ok(committer) = commit.committer() {
                seen(committer, 0);
            }
        }
        Ok(People { ids })
    }

    fn by_email(&self, email: &str) -> impl Iterator<Item = (&(String, String), &usize)> {
        let email = email.to_lowercase();
        self.ids.iter().filter(move |((_, e), _)| e.to_lowercase() == email)
    }

    /// The known e-mail closest to `email`, when it is a typo away.
    fn closest_email(&self, email: &str) -> Option<String> {
        let email = email.to_lowercase();
        let mut known: Vec<&String> = self.ids.keys().map(|(_, e)| e).collect();
        known.sort();
        known.dedup();
        known
            .into_iter()
            .map(|e| (distance(&email, &e.to_lowercase()), e))
            .filter(|(d, _)| (1..=2).contains(d))
            .min_by_key(|(d, _)| *d)
            .map(|(_, e)| e.clone())
    }
}

/// Edits between `a` and `b`.
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let next = (row[j + 1] + 1).min(row[j] + 1).min(prev + usize::from(ca != cb));
            prev = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

/// A .mailmap line: `Proper Name <proper@e-mail> Commit Name <commit@e-mail>`, any part but one
/// e-mail left out.
#[derive(Debug, Default, PartialEq, Eq)]
struct MailmapLine {
    name: Option<String>,
    email: String,
    old_name: Option<String>,
    old_email: Option<String>,
}

/// What is wrong with a line: the message, the part of the line, and the line put right.
type LineError = (String, (usize, usize), Option<String>);

/// The parts of a line, or what is wrong with it.
fn parse_mailmap(line: &str) -> std::result::Result<MailmapLine, LineError> {
    let content = line.split('#').next().unwrap_or("");
    let Some(open) = content.find('<') else {
        // An address written without its brackets.
        let words: Vec<(usize, &str)> = content
            .split(' ')
            .scan(0, |at, w| {
                let start = *at;
                *at += w.len() + 1;
                Some((start, w))
            })
            .collect();
        if let Some(&(at, word)) = words.iter().find(|(_, w)| w.contains('@')) {
            let fixed = format!("{}<{word}>{}", &line[..at], &line[at + word.len()..]);
            return Err((
                format!("An e-mail goes in angle brackets: `<{word}>`."),
                (at, at + word.len()),
                Some(fixed),
            ));
        }
        return Err((
            "Each line needs an e-mail in `< >`.".into(),
            (0, content.trim_end().len()),
            None,
        ));
    };
    let Some(close) = content[open..].find('>').map(|c| open + c) else {
        let end = content.trim_end().len();
        return Err((
            "The `<` is never closed.".into(),
            (open, end),
            Some(format!("{}>{}", &line[..end], &line[end..])),
        ));
    };
    let name = content[..open].trim();
    let email = content[open + 1..close].trim();
    let rest = &content[close + 1..];
    let (old_name, old_email) = match rest.find('<') {
        Some(o) => {
            let o = close + 1 + o;
            let Some(c) = content[o..].find('>').map(|c| o + c) else {
                let end = content.trim_end().len();
                return Err((
                    "The `<` is never closed.".into(),
                    (o, end),
                    Some(format!("{}>{}", &line[..end], &line[end..])),
                ));
            };
            let old = content[close + 1..o].trim();
            (
                Some(old).filter(|n| !n.is_empty()).map(str::to_owned),
                Some(content[o + 1..c].trim().to_owned()),
            )
        }
        None => (None, None),
    };
    if name.is_empty() && old_email.is_none() {
        return Err((
            "Give a name before the e-mail, or the e-mail used in commits after it in `< >`.".into(),
            (open, close + 1),
            None,
        ));
    }
    if !email.contains('@') {
        return Err((format!("`{email}` is not an e-mail address."), (open, close + 1), None));
    }
    Ok(MailmapLine {
        name: Some(name.to_owned()).filter(|n| !n.is_empty()),
        email: email.to_owned(),
        old_name,
        old_email,
    })
}

/// One person of a .mailmap: their first line, the identities it joins and their commits.
struct Person {
    first: usize,
    ids: HashSet<(String, String)>,
    commits: usize,
}

pub(super) fn check_mailmap(text: &str, people: Option<&People>, check: &mut FileCheck) {
    // The commit identity a line maps → (line, proper identity).
    let mut mapped: HashMap<(Option<String>, String), (usize, String)> = HashMap::new();
    let mut groups: HashMap<String, Person> = HashMap::new();
    let mut lines_of: Vec<(usize, String, usize)> = Vec::new();
    for (no, line) in numbered(text) {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let entry = match parse_mailmap(line) {
            Ok(entry) => entry,
            Err((message, range, fixed)) => {
                let fix = fixed.map(|f| replace(&format!("Use {}", f.trim()), no, &f));
                check.error(no, line, range, message, fix);
                continue;
            }
        };
        let commit_email = entry.old_email.clone().unwrap_or_else(|| entry.email.clone());
        let key = (
            entry.old_name.as_ref().map(|n| n.to_lowercase()),
            commit_email.to_lowercase(),
        );
        let proper = format!(
            "{}\u{0}{}",
            entry.name.clone().unwrap_or_default(),
            entry.email.to_lowercase()
        );
        let at = line.rfind('<').unwrap_or(0);
        let range = (at, line[at..].find('>').map_or(line.len(), |c| at + c + 1));
        if let Some((other, before)) = mapped.get(&key)
            && *before != proper
        {
            check.warn(
                no,
                line,
                range,
                format!("Line {other} maps the same commits; this line wins."),
                None,
            );
        }
        mapped.insert(key, (no, proper.clone()));
        let Some(people) = people else { continue };
        let covered: Vec<(&(String, String), &usize)> = people
            .by_email(&commit_email)
            .filter(|((name, _), _)| entry.old_name.as_ref().is_none_or(|n| n.eq_ignore_ascii_case(name)))
            .collect();
        if covered.is_empty() {
            let any = people.by_email(&commit_email).next().is_some();
            let (message, fix) = match (&entry.old_name, any) {
                (Some(old), true) => (format!("No commit is signed `{old} <{commit_email}>`."), None),
                _ => match people.closest_email(&commit_email) {
                    Some(close) => {
                        let who = people
                            .by_email(&close)
                            .max_by_key(|(_, n)| **n)
                            .map(|((name, _), _)| name.clone())
                            .unwrap_or_default();
                        (
                            format!("No commit uses `{commit_email}`. {who} commits as `{close}`."),
                            Some(replace(
                                &format!("Use {close}"),
                                no,
                                &line.replacen(&format!("<{commit_email}>"), &format!("<{close}>"), 1),
                            )),
                        )
                    }
                    None => (format!("No commit uses `{commit_email}`."), None),
                },
            };
            check.warn(no, line, range, message, fix);
            check.note(no, "no commit uses this e-mail", Some(Level::Warn));
            continue;
        }
        let commits: usize = covered.iter().map(|(_, n)| **n).sum();
        let person = groups.entry(proper.clone()).or_insert(Person {
            first: no,
            ids: HashSet::new(),
            commits: 0,
        });
        person
            .ids
            .extend(covered.iter().map(|(id, _)| (id.0.clone(), id.1.to_lowercase())));
        person.commits += commits;
        lines_of.push((no, proper, commits));
    }
    // What each line joins, the first line of a person saying how many spellings in all.
    for (no, proper, commits) in lines_of {
        let person = &groups[&proper];
        if no == person.first && person.ids.len() > 1 {
            check.note(
                no,
                format!(
                    "joins {} spellings · {}",
                    person.ids.len(),
                    plural(person.commits, "commit", "commits")
                ),
                None,
            );
        } else {
            check.note(no, plural(commits, "commit", "commits"), None);
        }
    }
}

impl Repo {
    /// A new .mailmap: the people the history shows under more than one e-mail or name, each
    /// put under the one they use most.
    pub(super) fn mailmap_starter(&self) -> String {
        let mut text = String::from(
            "# One person under one name and e-mail in History and Blame.\n# Proper Name <proper@e-mail> Name in Commits <e-mail in commits>\n",
        );
        let people = Context::of(self, RepoFileKind::Mailmap, false)
            .ok()
            .and_then(|c| c.people)
            .unwrap_or_default();
        let mut by_name: HashMap<String, Vec<(&String, &String, usize)>> = HashMap::new();
        for ((name, email), &n) in &people.ids {
            if n > 0 && !name.is_empty() {
                by_name.entry(name.to_lowercase()).or_default().push((name, email, n));
            }
        }
        let mut lines = Vec::new();
        let mut names: Vec<&String> = by_name.keys().collect();
        names.sort();
        for key in names {
            let ids = &by_name[key];
            let mut emails: Vec<(&String, usize)> = Vec::new();
            for (_, email, n) in ids {
                match emails.iter_mut().find(|(e, _)| e.eq_ignore_ascii_case(email)) {
                    Some(found) => found.1 += n,
                    None => emails.push((email, *n)),
                }
            }
            if emails.len() < 2 {
                continue;
            }
            emails.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
            let name = ids
                .iter()
                .max_by_key(|(_, _, n)| *n)
                .map(|(name, _, _)| *name)
                .expect("one at least");
            for (email, _) in &emails[1..] {
                lines.push(format!("{name} <{}> <{email}>", emails[0].0));
            }
        }
        if !lines.is_empty() {
            text.push('\n');
            for line in lines.into_iter().take(40) {
                text.push_str(&line);
                text.push('\n');
            }
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailmap_lines() {
        let line = parse_mailmap("Alex Morgan <alex@oxbow.dev> <alex@old.local>").unwrap();
        assert_eq!(line.name.as_deref(), Some("Alex Morgan"));
        assert_eq!(line.old_email.as_deref(), Some("alex@old.local"));
        let line = parse_mailmap("Dana <dana@oxbow.dev> dana <DANA@oxbow.dev>").unwrap();
        assert_eq!(line.old_name.as_deref(), Some("dana"));
        let (message, _, fixed) = parse_mailmap("Sam Ruiz sam@oxbow.dev").unwrap_err();
        assert!(message.contains("angle brackets"));
        assert_eq!(fixed.as_deref(), Some("Sam Ruiz <sam@oxbow.dev>"));
        assert!(parse_mailmap("<alex@oxbow.dev>").is_err());
        assert!(parse_mailmap("Lee <lee@oxbow.dev").is_err());
    }
}
