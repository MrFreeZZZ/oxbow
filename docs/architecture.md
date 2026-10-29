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

What a read keeps for the next one, what it is keyed by, and what lets it go. Started with the
caches of the speed series of 29 October; the older ones are still to be listed.

| Cache | Holds | Key | Let go | Budget | Stale answer? |
|---|---|---|---|---|---|
| `HistoryCache.walked` (history.rs) | the commits walked so far, newest first, and where the walk goes on from | the tips (refs, `HEAD`) and where a shallow history stops | another tip or a deeper fetch; another repository in the window | the commits loaded, about 60 bytes each | no: a commit never changes under its id |
| `CACHE` in repofiles.rs | each repository's working copy (files and folders, what the index tracks) and the people of its history, for the checks of Repository Files | the repository's folder; `fresh` when a file is opened | after 10 minutes; a saved file; the oldest past the budget | 8 repositories, 600,000 files of working copies | yes, up to 10 minutes: only for warnings and notes, never for errors |
| `HistoryCache.asked.changes`, the window's `changes` (session.rs) | the uncommitted files of the last history, once counted with their lines (`working_counts`) | what the watch noticed, `HEAD`, what is staged | any change the watch notices; the next reading | one reading | no: a reading with other files keeps its own counts |

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
tests of the core and the shell run on macOS beside it; you can download the app from the workflow run's artifacts. Because it is
unsigned, run `xattr -cr Oxbow.app` once before opening it.
