# Oxbow

A calm desktop Git client for macOS, Windows and Linux.

Oxbow reads history, graphs and diffs with gitoxide and changes repositories through the git command line: staging and commits, branches, merge and rebase with conflict resolution, stashes, tags, push, pull and fetch, Edit Stack, and an Operation Log with Undo. Every change shows the exact git commands first.

## Layout

```
crates/oxbow-core   Git core library, no UI: history, graph lanes, refs, diffs
app/                Desktop app: Svelte + TypeScript front end
app/src-tauri       Tauri shell that exposes oxbow-core to the front end
scripts/            Developer helpers
```

### How Git is accessed

- **Reads** (history, graph, refs, diffs) use [gitoxide](https://github.com/GitoxideLabs/gitoxide). It is fast and has no C dependencies.
- **Writes** (commit, rebase, push, hooks, SSH) will run the system `git` command line. Hooks, credentials and SSH then behave exactly as they do in a terminal.

`oxbow-core` doesn't depend on any UI, so it can be tested on its own and reused.

### The commit graph

The design rules are in `crates/oxbow-core/src/graph.rs`:

- The trunk (the first-parent chain of `main` or `master`) always sits in the leftmost column. It is drawn with a thicker line in Steel.
- Every other branch gets its own column to the right. It keeps that column for its whole life, and a column is reused only after its branch line has ended.
- A branch line runs down to the commit it forked from and curves into it. That commit gets a ring in the branch's color.
- A branch's color is stable, derived from its name. Yellow is reserved for tags.
- Commits that are not on any remote-tracking branch get hollow dots and dashed lines.

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
npm install
npm run tauri dev                    # opens the last repository, or asks for one
npm run tauri dev -- -- /path/to/repo  # opens a specific repository
```

To try it on a small repository with branches, merges, tags and unpushed work:

```sh
scripts/make-demo-repo.sh /tmp/acme-api
cd app && npm run tauri dev -- -- /tmp/acme-api
```

Checks, the same ones CI runs:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p oxbow-core
cd app && npm run check
```

To measure how fast a repository's history loads:

```sh
cargo run --release -p oxbow-core --example history -- /path/to/repo
```

Build an installable app with `cd app && npm run tauri build`. CI also builds an unsigned `.app` for macOS and a `.deb` for Linux on every pull request; you can download them from the workflow run's artifacts. Because the macOS build is unsigned, run `xattr -cr Oxbow.app` once before opening it.
