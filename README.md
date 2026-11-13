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
  A fast, modern desktop Git client.<br>
  History stays clear, a commit can be as small as the lines you choose, and a risky step shows its command before it runs.
</p>

<p align="center"><sub>Released for macOS, Linux and Windows</sub></p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/images/history-dark.png">
    <img src="docs/images/history-light.png" width="100%" alt="Oxbow showing a colored commit graph, branches, commit details and a code diff in one window">
  </picture>
</p>

<p align="center"><sub>Follow branches through the commit graph and inspect a change without leaving History.</sub></p>

Oxbow is a quiet desktop window for the whole repository: the graph, the diff, the review and the way back. It stays quick on a large history because commits and diffs are read in the app. Coming back to a window where nothing has changed does not start git. The tools are current — command palette, keyboard throughout, light and dark themes, updates in place — and every change still runs your own git, so hooks, SSH and credentials match the terminal.

## What you can do

**Follow the history.** The trunk of the graph is the production branch: `main` or `master`, unless you choose another. The checked-out branch takes the left column down to the commit it forked from, and the trunk stays on the left when that branch is the one checked out. Every other branch keeps its own column and a stable color for as long as its line runs. Yellow is reserved for tags. Commits the remote does not have yet show as hollow dots on dashed lines. Compare branches and commits, search history and changed code, and open a file's history or blame. Browse the files at a commit. Back and Forward return you to the same place in History.

**Shape the commit.** Stage by file, hunk or line. Amend replaces the last commit instead of adding a new one. Changelists group files and runs of lines, then commit them from an index of their own. The message draft stays with the folder. Before a commit, Oxbow checks staged lines for secrets unless you turn that off, and runs any checks this repository has set. Diffs sit side by side or in one column, file headers show line endings when they can be read, and selected code copies in one click.

**Step back.** A confirmation for a risky action explains the effect and shows the exact Git command. The Console and Activity show what ran. The Operation Log records supported local actions and can undo them, including commits, rebases, merges, deleted branches, dropped stashes, hard resets and discards. Lost commits and the other ways back are described below.

**Edit a stack.** Branches built one on another are rewritten together. Reorder, reword, edit, squash, fixup or drop commits, move a commit into another branch of the stack, split one commit into two, or move the stack onto the newest commit of the trunk. Before anything runs, Oxbow replays the plan and reports the first conflict, commits that would become empty, and which branches would need a force push. Every branch of the stack moves with its commits.

**Review on GitHub and GitLab.** Sign in to GitHub, or connect GitLab with a personal access token. A pull request and a merge request open on the same screen: the diff, comments, CI status and a draft review. Mark files viewed. Apply Suggestion puts the suggested lines in place of the ones the comment is on.

**Find the breaking commit.** Bisect starts from a commit that worked and one that is broken. Good, Bad and Skip each cut the remaining range in half, until one commit is left. Git keeps the bisect, so one started in a terminal shows here too.

**Keep the rest in one window.** Merge, rebase, cherry-pick and revert, and resolve conflicts in a three-way editor. Pull, push and fetch, with an explanation when a remote refuses. Clone a repository or initialize one. Publish a branch, or the repository itself, to GitHub or GitLab. Create a worktree and open it on its own. Create, delete and push tags. Stash what is staged, or a hunk or selected lines of a file; everything else stays, and Stashes brings it back. Save commits as patch files, or apply them: a mailbox of patches becomes commits, and a plain diff goes into the files uncommitted. Git LFS files are shown as pointers and downloaded into place when git-lfs is available; a tracking rule is written into `.gitattributes`. A submodule can be cloned, opened, updated to the commit this repository records, or pulled to the latest commit of the branch it follows. Run your own commands with live output and a Stop button. A repository does not run its own programs until you trust it. Recent repositories are in the same app.

**Shared rules.** Protected branches, by default `main`, `master` and the production branch, are never force-pushed. Commits a remote already has are not rewritten on them: Amend, Edit Stack, rebase and reset leave those commits, and Revert undoes one. Commits not pushed yet stay editable, and the list of names is yours to change. Shared files such as `.gitignore`, `.gitattributes`, `.mailmap` and `CODEOWNERS` are edited in Settings, checked as they are typed, and saved into the working copy so they show up in Changes.

## Why Oxbow

- Fast where you spend the time: history, the graph and diffs.
- Modern and calm: one window, a command palette, shortcuts, light and dark themes.
- Precise: a risky change shows your git command before it runs.
- Recoverable: lost commits stay findable, and a supported local mistake can be undone. The limit is stated.
- Careful: a repository does not run its own programs until you trust it.

## Find a lost commit

A commit can leave every branch and still be in git's reflog: after a reset, a deleted branch, a rebase, or work done in a terminal or another app. Open the Operation Log and choose **Git Reflog**. It reads git's own log, marks commits that no branch, tag or remote-tracking branch still has, and can show the commit and its changes. **Create Branch…** keeps that commit, and the commits before it, without moving HEAD. The date beside a lost commit is when git may delete it.

**Fix a Mistake** starts from the repository as it is now and plans one of the usual ways out, with the Git commands visible before they run:

- I committed to the wrong branch
- I forgot a file in the last commit
- Undo the last commit, and keep its changes
- Undo a commit that is already pushed, by adding a commit that reverses it
- Save work from a detached HEAD, including commits no branch has any more

If the damage is a file rather than a commit, **Local History** keeps eligible saves from any editor while the repository is open. A version from between commits can be compared and restored. If a merge, rebase, cherry-pick or revert stops, finish it in the conflict editor or abort it. Undo is not available while that operation is still in progress. After it, the Operation Log can still bring the repository back, within the limits below.

## Recovery, with the details visible

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/images/confirm-dark.png">
    <img src="docs/images/confirm-light.png" width="720" alt="Oxbow explaining a hard reset and showing the exact Git command before confirmation">
  </picture>
</p>

<p align="center"><sub>Before a hard reset, Oxbow shows what will be discarded and the command it plans to run.</sub></p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/images/undo-dark.png">
    <img src="docs/images/undo-light.png" width="720" alt="Oxbow explaining how it will undo a supported hard reset and showing the restore commands">
  </picture>
</p>

<p align="center"><sub>For a supported action, Undo shows how the local repository will be restored.</sub></p>

Undo covers supported local actions, not remote changes or arbitrary custom scripts. Conflict
resolution cannot be undone as a conflict: snapshots do not preserve unmerged index stages,
operation metadata or Git's shared rerere cache. The Operation Log can still expose saved file
copies through **Compare with Now → Browse Saved Files → Save As**. Save a copy before replacing
current files. Undo and Restore are unavailable while a Git operation is in progress.

Undo history is kept for 30 days by default, or for less if git expires those objects sooner, and the period is configurable. Files Oxbow moves aside stay for 30 days. Local History keeps eligible
files up to 1 MB for seven days by default, with a 256 MB limit per worktree; its retention period
is configurable. It captures saves only while the repository is open.

## Project status

Releases include packages for macOS Apple Silicon and Intel, Linux x64 and Windows x64. The macOS app is ad-hoc signed and is not notarized. Its update packages are signed separately. GitLab integration has automated tests against a test server; a check against a live GitLab service is still ahead.

Development branches start from `preview` and target it with pull requests. `main` is reserved
for releases: merge a validated `preview` (or a temporary `release/*` branch) through a pull
request, then tag the merged commit. Urgent `hotfix/*` branches start from `main`; merge their
fixes back into `preview` as well. Keep release merges as merge commits rather than squashing
or rebasing, so both long-lived branches retain their shared history. Release notes and the
changelog require approval before publication.

CI checks pull requests and pushes to `main` on Linux. Pushes to `preview` run Preview, which checks Linux and tests
macOS once before building and publishing a signed macOS Apple Silicon update.
Version tags run a separate release workflow that checks the tagged commit, builds all three
platforms and prepares a draft release for review. Release commits must belong to `main`.

## Build from source

You need Rust 1.99.1 (pinned in `rust-toolchain.toml`), Node.js 26.13 or newer, npm 12.2.2 or newer, a system `git` and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your system.

```sh
cd app
npm ci
npm run tauri dev -- -- /path/to/repo   # omit the path to use the saved startup preference
```

From `app/`, run `npm run tauri build` to build the application bundle. From the repository root:

```sh
scripts/make-demo-repo.sh /tmp/oxbow-demo  # replaces this path with a demo repository
scripts/check.sh                            # checks what changed since preview
scripts/check.sh all                        # all formatting, lint, type and unit checks
```

## Architecture

- `crates/oxbow-core/` — UI-independent Rust library for Git operations
- `app/src-tauri/` — Tauri 2 desktop shell, native windows and system integration
- `app/src/` — Svelte 5 and TypeScript interface
- `app/tests/` and `app/e2e/` — frontend unit tests and application acceptance scenarios

History and diffs use [gitoxide](https://github.com/GitoxideLabs/gitoxide); repository changes
run the system Git CLI. Personal commands run only when invoked and show live output in a
resizable native window that remembers its size and position.

Third-party credits are in [THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt).

Copyright (c) 2026 Alexander Marin. All rights reserved.
