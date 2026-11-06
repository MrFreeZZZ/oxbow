use super::{ConfigLine, FileCheck, Repo, config_lines, replace};

/// What git-lfs reads from .lfsconfig; anything else there it leaves alone (git-lfs
/// `safeKeys` and the patterns beside it).
fn lfs_reads(full: &str) -> bool {
    const KEYS: &[&str] = &[
        "lfs.allowincompletepush",
        "lfs.fetchexclude",
        "lfs.fetchinclude",
        "lfs.gitprotocol",
        "lfs.locksverify",
        "lfs.pushurl",
        "lfs.skipdownloaderrors",
        "lfs.url",
    ];
    if KEYS.contains(&full) {
        return true;
    }
    let (first, rest) = full.split_once('.').unwrap_or((full, ""));
    (first == "lfs" && (rest.ends_with(".access") || rest.ends_with(".locksverify")))
        || (first == "remote" && (rest.ends_with(".lfsurl") || rest.ends_with(".lfspushurl")))
}

impl Repo {
    pub(super) fn check_lfsconfig(&self, text: &str, check: &mut FileCheck) {
        let lines = match config_lines(text) {
            Ok(lines) => lines,
            Err(no) => {
                let line = text.split('\n').nth(no.saturating_sub(1)).unwrap_or("");
                check.error(
                    no,
                    line,
                    (0, line.trim_end().len()),
                    "Git can’t read this line, so git-lfs ignores the whole file.",
                    None,
                );
                return;
            }
        };
        for (no, line, section, parsed) in &lines {
            let ConfigLine::Key { name, value, at } = parsed else {
                continue;
            };
            let full = format!("{section}.{name}");
            let span = (*at, line.trim_end().len());
            if !lfs_reads(&full) {
                check.warn(
                    *no,
                    line,
                    span,
                    format!("git-lfs doesn’t read `{full}` from .lfsconfig; it belongs in git’s settings."),
                    None,
                );
                continue;
            }
            let is_url = full == "lfs.url"
                || full == "lfs.pushurl"
                || full.ends_with(".lfsurl")
                || full.ends_with(".lfspushurl");
            let Some(value) = value.filter(|_| is_url) else {
                continue;
            };
            let Some((scheme, rest)) = value.split_once("://") else {
                check.error(
                    *no,
                    line,
                    span,
                    "git-lfs needs an address that starts with `https://` or `ssh://`.",
                    None,
                );
                continue;
            };
            if !["https", "http", "ssh", "file"].contains(&scheme.to_lowercase().as_str()) {
                check.error(
                    *no,
                    line,
                    span,
                    "git-lfs needs an address that starts with `https://` or `ssh://`.",
                    None,
                );
                continue;
            }
            let host_part = rest.split('/').next().unwrap_or("");
            if let Some((user, _)) = host_part.rsplit_once('@')
                && user.contains(':')
            {
                let clean = value.replacen(&format!("{user}@"), "", 1);
                check.error(
                    *no,
                    line,
                    span,
                    "This puts a password in the repository for everyone who clones it.",
                    Some(replace("Remove the Password", *no, &line.replacen(value, &clean, 1))),
                );
            }
        }
        let attributes = crate::lfs::read_attributes(self.workdir()).unwrap_or_default();
        if crate::lfs::lfs_patterns(&attributes).is_empty()
            && lines.iter().any(|(_, _, _, p)| matches!(p, ConfigLine::Key { .. }))
        {
            check.warn(
                0,
                "",
                (0, 0),
                "No rule in .gitattributes sends files to Git LFS, so this file changes nothing yet.",
                None,
            );
        }
    }
}
