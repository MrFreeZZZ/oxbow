<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/images/name-dark.png">
    <img src="docs/images/name-light.png" width="220" alt="Oxbow">
  </picture>
</p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/images/code-dark.png">
    <img src="docs/images/code-light.png" alt="Your code is half the story. Oxbow tells the other half: commits, branches, history.">
  </picture>
</p>

<p align="center">
  A calm desktop Git client with a clear history, transparent commands<br>
  and recovery tools for everyday work.
</p>

<p align="center">
  <sub>macOS first. Windows and Linux builds are not ready for release.</sub>
</p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/images/history-dark.png">
    <img src="docs/images/history-light.png" alt="Oxbow: the commit graph of a repository with several stacked branches, and the diff of the selected commit">
  </picture>
</p>

## Why Oxbow

**Recover from mistakes.** The Operation Log records supported repository changes with their
before and after state. Undo (⌘Z on macOS) can restore local work after a commit, rebase, merge,
deleted branch, dropped stash, hard reset or discard. Confirmations explain what cannot be
recovered. Remote changes and arbitrary custom scripts are not covered by Undo.

**Transparent commands.** Risky actions ask first and show the planned git commands. Simple
actions run at once and appear in Activity. The Console shows command output for troubleshooting.

**Real git underneath.** History, the graph and diffs are read with
[gitoxide](https://github.com/GitoxideLabs/gitoxide), so even big repositories open quickly.
Every change runs your own `git`, so hooks, credentials and SSH behave exactly as in a terminal.

**More ways back.** Dropped stashes and aborted operations keep a copy for 30 days. Lost Work
finds commits from the reflog, and Fix a Mistake plans the way out of a commit on the wrong
branch, a forgotten file or a push you want to take back. While a repository is open, Local
History captures saved changes from any editor for comparison or restore. It keeps eligible
files up to 1 MB for seven days by default, groups nearby saves and has a 256 MB storage limit
per worktree. The retention period is configurable.

**Stacked branches.** Edit Stack rewrites a chain of branches in one go. Pull requests keep their
stack list up to date and the rest of a stack is restacked when its base is merged.

**Secrets stay out.** Before each commit the staged lines are checked for passwords, tokens and
private keys, with your own patterns on top.

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/images/confirm-dark.png">
    <img src="docs/images/confirm-light.png" width="49%" alt="A hard reset shows the exact git command before it runs">
  </picture>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/images/undo-dark.png">
    <img src="docs/images/undo-light.png" width="49%" alt="Undo puts the repository back as it was before the reset">
  </picture>
</p>

## Everything you use every day

- Staging by file, hunk or line; changelists, each committed on its own; amend, reword, fixup
- Branches, tags, stashes, worktrees, submodules, Git LFS
- Merge, rebase, cherry-pick and revert with a three-way conflict editor
- Push, pull and fetch with clear messages when a remote refuses
- Compare branches and commits, search history and code, file history and blame
- Bisect, with a test command if you have one
- GitHub: sign in, publish a repository, open, review and merge pull requests
- GitLab: connect with a personal access token, create, review and merge merge requests
- Review comments, suggested changes and CI status for pull and merge requests
- Custom commands with repository, branch and commit context, live output and a Stop button
- Welcome screen with local summaries of recent repositories, refreshed on demand without fetching
- Repository trust controls, protected branches and configurable checks before committing
- Import and export patches; edit shared repository files from Settings
- Command Palette and VS Code's keyboard shortcuts, light and dark themes

## Project status

Oxbow is under active development. macOS is the primary build and testing target; Windows and
Linux release builds, Developer ID signing and macOS notarization remain pending. GitLab
integration has automated coverage against a test server; validation against a live GitLab
service is still pending.

CI checks pull requests on Linux. Pushes to `main` also build a macOS app and run Rust tests on
macOS. A build artifact counts as checked only when all jobs in that run pass.

## Building from source

You need Rust stable (1.88 or newer), Node.js 22.12 or newer, a system `git` and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your system.

```sh
cd app
npm ci
npm run tauri dev -- -- /path/to/repo   # omit the path to use the saved startup preference
```

From `app/`, run `npm run tauri build` to build the application bundle.

From the repository root:

```sh
scripts/make-demo-repo.sh /tmp/acme-api  # create a repository to explore
scripts/check.sh                       # checks for changed app/core files
scripts/check.sh all                   # all formatting, lint, type and unit checks
```

## Architecture

- `crates/oxbow-core/` — UI-independent Rust library for Git operations
- `app/src-tauri/` — Tauri 2 desktop shell, native windows and system integration
- `app/src/` — Svelte 5 and TypeScript interface
- `app/tests/` and `app/e2e/` — frontend unit tests and application acceptance scenarios

History and diffs use gitoxide; repository changes run the system git CLI. Personal commands
are stored in application settings and run only when explicitly invoked. They show live output
in a resizable native window that remembers its size and position.

Third-party credits are in [THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt).
