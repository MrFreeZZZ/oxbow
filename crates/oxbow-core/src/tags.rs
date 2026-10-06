//! Tags: making, pushing and deleting them, and which ones the remote has.
//!
//! `git push` never sends tags, so a tag is easily left on one computer. Git keeps no record of
//! the remote's tags either, so Oxbow asks the remote (`git ls-remote --tags`) after fetching and
//! keeps the answer in `.git/OXBOW_REMOTE_TAGS`; tags missing from it are drawn as local.

use std::collections::BTreeSet;

use crate::cli::GitCommand;
use crate::error::Result;
use crate::repo::{RefInfo, RefKind, Repo};
use crate::worktree::Action;

const REMOTE_TAGS: &str = "OXBOW_REMOTE_TAGS";

/// Tag commands of [`crate::worktree::Action`].
pub(crate) fn plan_create(name: &str, commit: &str, message: Option<&str>, push: Option<&str>) -> Vec<GitCommand> {
    let mut commands = Vec::new();
    match message.map(str::trim).filter(|m| !m.is_empty()) {
        Some(message) => {
            let mut args = vec!["tag".to_owned(), "-a".to_owned(), name.to_owned()];
            for paragraph in crate::worktree::paragraphs(message) {
                args.push("-m".to_owned());
                args.push(paragraph);
            }
            args.push(commit.to_owned());
            commands.push(GitCommand::new(args).comment("-a: an annotated tag, with its own message, author and date"));
        }
        None => commands
            .push(GitCommand::new(["tag", name, commit]).comment("a lightweight tag: just a name for the commit")),
    }
    if let Some(remote) = push {
        commands.push(push_command(remote, &[name.to_owned()]));
    }
    commands
}

pub(crate) fn push_command(remote: &str, names: &[String]) -> GitCommand {
    let mut args = vec!["push".to_owned(), remote.to_owned()];
    for name in names {
        args.push("tag".to_owned());
        args.push(name.clone());
    }
    GitCommand::new(args)
        .comment("git push never sends tags on its own; tag <name> sends that one")
        .with_progress()
}

pub(crate) fn plan_delete(name: &str, remote: Option<&str>) -> Vec<GitCommand> {
    let mut commands = Vec::new();
    // The remote first: if it can't be reached, the tag is still here to try again.
    if let Some(remote) = remote {
        commands.push(
            GitCommand::new(["push", remote, "--delete", &format!("refs/tags/{name}")])
                .comment(format!("--delete: remove the tag on {remote}"))
                .with_progress(),
        );
    }
    commands.push(GitCommand::new(["tag", "-d", name]).comment("-d: delete the tag here; the commit stays"));
    commands
}

pub(crate) fn plan_fetch(remote: &str) -> Vec<GitCommand> {
    vec![
        GitCommand::new(["fetch", remote, "--tags"])
            .comment("--tags: every tag, also ones on commits no branch has")
            .with_progress(),
    ]
}

impl Repo {
    /// Ask `remote` which tags it has and remember the answer. True when it changed.
    pub fn refresh_remote_tags(&self, remote: &str) -> Result<bool> {
        let out = self.run(&GitCommand::new(["ls-remote", "--tags", "--refs", remote]))?;
        let names = parse_ls_remote(&out.stdout);
        if self.saved_remote_tags(remote).as_ref() == Some(&names) {
            return Ok(false);
        }
        self.save_remote_tags(remote, &names);
        Ok(true)
    }

    /// The tags `remote` had when it was last asked; `None` before it was ever asked.
    fn saved_remote_tags(&self, remote: &str) -> Option<BTreeSet<String>> {
        let text = std::fs::read_to_string(self.local().git_dir().join(REMOTE_TAGS)).ok()?;
        let mut lines = text.lines();
        (lines.next()? == remote).then(|| lines.map(str::to_owned).collect())
    }

    fn save_remote_tags(&self, remote: &str, names: &BTreeSet<String>) {
        let mut text = format!("{remote}\n");
        for name in names {
            text.push_str(name);
            text.push('\n');
        }
        let _ = std::fs::write(self.local().git_dir().join(REMOTE_TAGS), text);
    }

    /// Tags among `refs` the default remote does not have, as far as Oxbow knows. Empty when it
    /// was never asked.
    pub(crate) fn local_tags(&self, refs: &[RefInfo]) -> Vec<String> {
        let Some(known) = self.default_remote().and_then(|remote| self.saved_remote_tags(&remote)) else {
            return Vec::new();
        };
        refs.iter()
            .filter(|r| r.kind == RefKind::Tag && !known.contains(&r.name))
            .map(|r| r.name.clone())
            .collect()
    }

    /// Keep the remembered remote tags right after an action that changed them went through.
    pub(crate) fn after_tags(&self, action: &Action) {
        match action {
            Action::CreateTag {
                name,
                push: Some(remote),
                ..
            } => self.note_remote_tags(remote, std::slice::from_ref(name), &[]),
            Action::PushTags { remote, names } => self.note_remote_tags(remote, names, &[]),
            Action::DeleteTag {
                name,
                remote: Some(remote),
            } => self.note_remote_tags(remote, &[], std::slice::from_ref(name)),
            _ => {}
        }
    }

    /// The remote's tags changed in a known way.
    fn note_remote_tags(&self, remote: &str, added: &[String], removed: &[String]) {
        let Some(mut known) = self.saved_remote_tags(remote) else {
            return;
        };
        known.extend(added.iter().cloned());
        for name in removed {
            known.remove(name);
        }
        self.save_remote_tags(remote, &known);
    }
}

/// Tag names from `git ls-remote --tags --refs`: `<sha>\trefs/tags/<name>` lines.
fn parse_ls_remote(out: &str) -> BTreeSet<String> {
    out.lines()
        .filter_map(|line| line.split_once('\t'))
        .filter_map(|(_, name)| name.strip_prefix("refs/tags/"))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ls_remote_names() {
        let out = "1a2b\trefs/tags/v1.0.0\n3c4d\trefs/tags/release/2\n";
        let names: Vec<_> = parse_ls_remote(out).into_iter().collect();
        assert_eq!(names, ["release/2", "v1.0.0"]);
    }

    #[test]
    fn annotated_tag_carries_its_message() {
        let commands = plan_create("v1.0.0", "abc123", Some("First release"), Some("origin"));
        assert_eq!(commands[0].display(), "git tag -a v1.0.0 -m 'First release' abc123");
        assert_eq!(commands[1].display(), "git push origin tag v1.0.0");
        let light = plan_create("v1.0.0", "abc123", Some("  "), None);
        assert_eq!(light.len(), 1);
        assert_eq!(light[0].display(), "git tag v1.0.0 abc123");
    }
}
