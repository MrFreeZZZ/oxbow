# Oxbow — notes for Claude

Desktop Git client: Rust core + Tauri 2 shell + Svelte 5 / TypeScript UI. macOS first, also
Windows and Linux. Reads (history, graph, refs, diffs) go through gitoxide (`gix`); every change
to a repository runs the system `git` CLI, so hooks, credentials and SSH behave as in a terminal.

Read only the files the task needs. Start from the map below and `grep`, don't survey the tree.

## Map

```
crates/oxbow-core/          UI-free core library (all Git logic lives here)
  src/lib.rs                module list and re-exports
  src/repo.rs               Repo: open, HEAD, refs, remotes, stashes
  src/history.rs            history list for the graph (gix)
  src/commit.rs             commit details, commit/tree diffs, word diff
  src/graph.rs              graph lane layout; design rules in its header comment
  src/cli.rs                running `git`: commands, plans shown in confirmations, literal()
  src/worktree.rs           working copy and diffs; `enum Action` + plan() / perform(): every
                            change is an Action planned into git commands, then run
  src/edit.rs               History menu: cherry-pick, revert, reset, reword last
  src/operation.rs          merge/rebase and anything git can stop midway; conflicts
  src/oplog.rs              Operation Log and Undo; notkept.rs = what Undo can't bring back
  src/stack.rs              Edit Stack (rewrite stacked branches); series.rs = before/after a rewrite
  src/remote.rs             push/pull/fetch, upstreams, remote errors; ssh.rs, fetched.rs
  src/folders.rs            other worktrees' and submodules' folders: read after the history,
                            4 at a time, cancellable, kept while HEAD and index stay the same
  src/stash.rs tags.rs bisect.rs lfs.rs submodules.rs worktrees.rs search.rs compare.rs
  src/filehistory.rs        file history and blame;  reflog.rs = lost work
  src/fix.rs                Fix a Mistake plans;  secrets.rs = pre-commit secrets check
  src/github.rs pulls.rs    GitHub REST: sign-in, publish, pull requests
  src/config.rs setup.rs    git settings; clone/init;  console.rs = Console process log
  tests/<area>.rs           integration tests, one file per area; tests/support/mod.rs = Fixture
  examples/history.rs       history load benchmark;  reads.rs = opening with worktrees/submodules
app/src-tauri/              Tauri shell (crate oxbow-app)
  src/main.rs               all #[tauri::command]s and the generate_handler! list
  src/accounts.rs pulls.rs open_in.rs console_window.rs
  src/settings.rs           settings.json: read (never written over when broken), atomic write
  tauri.conf.json           window, CSP; build.rs sets version 0.1.<commit count>
app/src/                    Svelte front end
  App.svelte                main window: layout, wiring, most state (large: grep, don't read whole)
  lib/api.ts                typed invoke() wrappers, one per Tauri command
  lib/types.ts              TS mirrors of oxbow-core serde types
  lib/shortcuts.ts          every command and its keys (VS Code's, yours over them); keys.ts shows them;
                            shortcutEdit.svelte.ts = Settings › Keyboard Shortcuts
  lib/prefs.svelte.ts       settings;  SettingsWindow.svelte = Settings UI
  lib/confirm.svelte.ts     confirmations (ConfirmSheet.svelte);  activity.svelte.ts = Activity
  lib/*.svelte              one component per view/sheet (HistoryList, DiffView, ChangesPanel…)
  lib/*.ts                  pure logic next to its view (diffRows, conflicts, branches, graph…)
  app.css themes.ts branchPalettes.ts   design tokens, code theme Estuary, branch colors
app/tests/*.test.ts         node:test unit tests for the pure lib/*.ts modules
scripts/check.sh            quiet checks (below);  make-demo-repo.sh = demo repository
scripts/make-perf-fixtures.sh   big fixtures for examples/;  docs/measuring-reads.md = how to run them
```

A new backend action usually touches, in order: `crates/oxbow-core/src/<area>.rs` (+ test in
`tests/<area>.rs`) → a command in `app/src-tauri/src/main.rs` and its `generate_handler!` entry →
a wrapper in `app/src/lib/api.ts` and types in `lib/types.ts` → the Svelte view.

## Checking

Always use the script; it keeps the output out of the context:

```sh
scripts/check.sh          # auto: only the groups touched since main
scripts/check.sh ui       # npm test + svelte-check            (app/src, app/tests)
scripts/check.sh core     # cargo fmt, clippy, test oxbow-core  (crates/)
scripts/check.sh tauri    # cargo fmt, clippy, test oxbow-app   (app/src-tauri)
scripts/check.sh all
```

It prints one line when green, otherwise only the errors; full logs are in `target/check/*.log`.
Read a log with `grep`/`tail`, never `cat` it whole.

- Check only what the change touches: a UI change needs `ui`, a core change `core`.
- Run the script before every push. CI has no PR checks; it only builds the macOS app on merge.
- Never paste long build or test output into the chat or the thread; summarize it.
- One run of a single Rust test: `cargo test -p oxbow-core --test <file> <name> -q 2>&1 | tail -n 20`.
- UI changes are verified in a production build with the CSP on (`cd app && npm run build`).

Cloud sessions: `.claude/hooks/session-start.sh` installs the Tauri Linux packages
(libwebkit2gtk-4.1-dev, libgtk-3-dev, librsvg2-dev), runs `npm ci` and `cargo fetch`, and
fetches origin/main. Its log: `/tmp/oxbow-session-start.log`. Project threads don't read
`.claude/settings.json`, so the hook doesn't run there: if `app/node_modules` is missing, run
`CLAUDE_CODE_REMOTE=true .claude/hooks/session-start.sh` once (about a minute, prints one line)
before the first check or build. Use `npm ci`, not `npm install` (install rewrites
package-lock.json).

## Code rules

- Paths given to git go through `cli::literal()` (`:(literal)` pathspecs).
- Nothing Oxbow drops is deleted for good: stash Abort/drop keep a copy in `.git/oxbow/removed/`
  (30 days). Copies of commits are matched only by patch or by `.git/oxbow/replay`.
- Svelte 5: copy `$state` before mutating it.
- Keyboard shortcuts live only in `app/src/lib/shortcuts.ts` and match VS Code where VS Code has
  the command.
- Confirmations say «Do you really want to …» and show the exact git commands; by default only
  risky actions ask, simple ones go to Activity.
- Design: macOS HIG, soft and muted. Steel accent (`#4A6680`, dark `#5D7C99`) for actions and
  focus; no red/pink/green/yellow for branches (yellow is for tags); selection is a soft tint in
  the branch color. Diff: solid blocks, word capsules, no +/− markers.
- No AI features of any kind.
- No license file, on purpose. Never add one. Third-party credits only in THIRD-PARTY-NOTICES.md.
- Match the surrounding code: comment density, naming, plain-English user-facing text.
- Commit messages: one plain sentence saying what the user now gets (see `git log`).
