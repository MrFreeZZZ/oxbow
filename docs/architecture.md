# How Oxbow is put together

## Layout

```
crates/oxbow-core   Git core library, no UI: history, graph lanes, refs, diffs, every change
app/                Desktop app: Svelte + TypeScript front end
app/src-tauri       Tauri shell that exposes oxbow-core to the front end
scripts/            Developer helpers
```

`oxbow-core` doesn't depend on any UI, so it can be tested on its own and reused.

## How Git is accessed

- **Reads** (history, graph, refs, diffs) use [gitoxide](https://github.com/GitoxideLabs/gitoxide). It is fast and has no C dependencies.
- **Writes** (commit, rebase, push, hooks, SSH) run the system `git` command line. Hooks, credentials and SSH then behave exactly as they do in a terminal.

## The commit graph

The design rules are in `crates/oxbow-core/src/graph.rs`:

- The trunk (the first-parent chain of `main` or `master`) always sits in the leftmost column. It is drawn with a thicker line in Steel.
- Every other branch gets its own column to the right. It keeps that column for its whole life, and a column is reused only after its branch line has ended.
- A branch line runs down to the commit it forked from and curves into it. That commit gets a ring in the branch's color.
- A branch's color is stable, derived from its name. Yellow is reserved for tags.
- Commits that are not on any remote-tracking branch get hollow dots and dashed lines.

## Caches

What a read keeps for the next one, what it is keyed by, what lets it go, how much it may hold,
and whether an answer from it can be out of date. A new cache gets its row here.

In the core, the caches of a `Repo` are shared by its clones and carried over by `reopen` while
the folder stays the same; the window's (`session.rs`) go when it opens another repository.

| Cache | Holds | Key | Let go | Budget | Stale answer? |
|---|---|---|---|---|---|
| `HistoryCache.last` (history.rs) | the commit rows of the last history, the graph laid out | `rows_key`: the folder, how many commits and down to which, Current Branch Only, `HEAD`, refs, stashes, remotes, a rebase, whether there are changes, the production branch, where a shallow history stops, the unpushed answers | any of these changing; another repository in the window | one history | no: the rows are built from nothing else |
| `HistoryCache.walked` (history.rs) | the commits walked so far, newest first, and where the walk goes on from | the tips (refs, `HEAD`) and where a shallow history stops | another tip or a deeper fetch; another repository in the window | the commits loaded, about 60 bytes each | no: a commit never changes under its id |
| `HistoryCache.asked` `tracking`, `worktrees`, `rewrite` (history.rs) | git's answers of the last read: upstreams, the worktrees, the old versions of rewritten commits | the refs and the settings; the refs and `.git/worktrees`; the upstream and what the watch noticed | their key changing; another repository in the window | one answer each | no |
| `HistoryCache.asked.changes`, the window's `changes` (session.rs) | the uncommitted files of the last history, once counted with their lines (`working_counts`) | what the watch noticed, `HEAD`, what is staged | any change the watch notices; the next reading | one reading | no: a reading with other files keeps its own counts |
| `UnpushedAnswers` (history.rs) | which loaded commits past the walk are unpushed (`settle_unpushed`) | the tips of the branches and remote-tracking branches (`tips_key`) | other tips | the commits asked about | no |
| `ChangesCache` (commit.rs) | the changed files of the commit asked for last, for its file list, counts and pages of diffs | the commit | the next commit asked for | one commit | no: a commit's changes never change |
| `PairChangesCache` (commit.rs) | the changed files between the two trees compared last | the two trees | the next comparison | one comparison | no |
| `LINES_KEPT` (commit.rs) | the lines of blobs and working files asked for last, for the lines between hunks a page at a time | a blob by its id; a file by path, size, time of change and inode (`file_stamp`), kept only 2 s after its last change | the oldest past the budget; the watch seeing the file change (`forget_file_lines`) | 64 MB, none bigger than half of it; no lines of a side over 32 MB | no |
| `UntrackedCounts` (worktree.rs) | the lines counted in new files | the path, while the file keeps its size and time of change | files no longer new, when all new files are counted | the new files | no |
| `Repo.git_paths` (repo.rs) | `git rev-parse --git-path` answers | the name asked | never: `reopen` keeps them while `.git` is the same | a handful of paths | no: they never change for a repository |
| `read_log` `LAST` (oplog.rs) | the steps of the Operation Log | the log file, its time of change and length | the file changing; the oldest past the budget | 8 logs | no |
| `CACHE` in repofiles.rs | each repository's working copy (files and folders, what the index tracks) and the people of its history, for the checks of Repository Files | the repository's folder; `fresh` when a file is opened | after 10 minutes; a saved file; the oldest past the budget | 8 repositories, 600,000 files of working copies | yes, up to 10 minutes: only for warnings and notes, never for errors |
| `FolderCache` (folders.rs, one per window) | what the other worktrees' and the submodules' folders had: their changes and checkout | the folder, with its `HEAD`, index and `logs/HEAD` (`stamp`) | after 5 minutes; a reading asked since the window came back to the front or Reload (`since`); another repository | the folders the history lists | yes, up to 5 minutes: files edited there with nothing in git showing it |
| `git_version` `KNOWN` (config.rs) | the version of the `git` Oxbow runs | the program chosen in Settings | another program | one | no |
| `lfs_version` `FOUND` (lfs.rs) | the version of git-lfs, once found | none | never while the app runs | one | yes: an upgrade shows after a restart; not having it is asked each time |
| `CommitCache` (session.rs, the app's) | commit details, diffs and pages of diffs worked out | the folder, the commit, the path, the diff settings, where a shallow history stops | the oldest past the budget | 64 MB | no: a commit never changes |
| GitHub and GitLab tokens (accounts.rs, gitlab_accounts.rs) | the token read from the credential store | the account; the GitLab server | signing out | one per account | no |
| `cache` in syntax.ts | lines colored for the code theme | the language and the line's text | the oldest past the budget | 4,000,000 characters | no |
| `github` (github.svelte.ts) | the repository on GitHub, its pull requests, GitLab's merge requests | the repository's folder | another repository; asked again after a minute, or at once with `force` | one repository | yes, up to a minute, unless something done here asks again |

## Development

Prerequisites:

- Rust (stable) and Node.js 22 or newer.
- Platform dependencies for Tauri 2 (see the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)):
  - macOS: Xcode Command Line Tools.
  - Linux: `libwebkit2gtk-4.1-dev`, `libxdo-dev`, `libssl-dev`, `libayatana-appindicator3-dev` and `librsvg2-dev`.
  - Windows: Microsoft C++ Build Tools and WebView2 (preinstalled on Windows 11).

Run the app:

```sh
cd app
npm ci
npm run tauri dev                      # opens the last repository, or asks for one
npm run tauri dev -- -- /path/to/repo  # opens a specific repository
```

To try it on a small repository with branches, merges, tags and unpushed work:

```sh
scripts/make-demo-repo.sh /tmp/acme-api
cd app && npm run tauri dev -- -- /tmp/acme-api
```

Checks (formatting, lints and tests, only for the parts changed since `main`; `all` for everything):

```sh
scripts/check.sh
scripts/check.sh all
```

To measure how fast a repository's history loads, see [measuring-reads.md](measuring-reads.md).

Build an installable app with `cd app && npm run tauri build`. CI runs `scripts/check.sh all` on
Linux for every pull request, and builds an unsigned macOS app on every merge to `main`, with the
tests of the core and the shell run on macOS beside it; you can download the app from the workflow run's artifacts. The build
doesn't wait for the checks, so it counts as checked only when the run's other jobs are green too. Because it is
unsigned, run `xattr -cr Oxbow.app` once before opening it.
