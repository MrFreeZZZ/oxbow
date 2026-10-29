# Measuring how fast a repository opens

What opening and refreshing a repository costs: the history (graph, branches, HEAD), and after
it what the other worktrees and the submodules' folders have. The history shows as soon as it is
read; each other folder's state fills in after, at most four `git status` at a time.

## Fixtures

```sh
scripts/make-perf-fixtures.sh ~/oxbow-perf
```

About three minutes; it needs about 1 GB. It makes:

| Folder | What is in it |
|---|---|
| `history-100k` | 100,000 commits, 30 side branches (half merged back), 20 tags |
| `history-100k-clone` | a clone of it, as most repositories are: origin's branches, and one commit not pushed |
| `worktrees-20/main` | 2,000 files and 20 more worktrees; every third has 3 changed and 2 new files |
| `worktrees-50/main` | the same with 50 worktrees |
| `submodules-20` | 20 submodules: clean, changed inside, behind, ahead, two not cloned |
| `conflicts` | a merge stopped on 4 files of 10,000 and 100,000 lines, with 100 and 1,000 conflicts each, for the Conflicts screen |
| `changes-10k` | 10,000 files of 200 lines, each with two lines edited and the first 1,000 staged, for the Changes list |

## On a Mac, with a release build

1. Note the machine: model and chip, memory, macOS, git.

   ```sh
   sysctl -n machdep.cpu.brand_string hw.memsize; sw_vers -productVersion; git --version
   ```

2. Rust, if `cargo --version` doesn't answer: `curl https://sh.rustup.rs -sSf | sh`, then open a
   new Terminal window.

3. In the oxbow folder, on the latest `main`, warm (the disk cache has the files):

   ```sh
   RUNS=30 SLOW=2 cargo run --release -p oxbow-core --example reads -- \
     ~/oxbow-perf/history-100k ~/oxbow-perf/history-100k-clone ~/oxbow-perf/worktrees-20/main \
     ~/oxbow-perf/worktrees-50/main ~/oxbow-perf/submodules-20 | tee ~/oxbow-perf/warm.txt
   ```

4. Cold, with the disk cache emptied first (asks for your password):

   ```sh
   for f in history-100k history-100k-clone worktrees-20/main worktrees-50/main submodules-20; do
     sudo purge
     RUNS=1 cargo run --release -q -p oxbow-core --example reads -- ~/oxbow-perf/$f
   done | tee ~/oxbow-perf/cold.txt
   ```

5. In the app itself: open `~/oxbow-perf/worktrees-50/main`, then the Console (⇧⌘U). Each
   Reload (⌘R) adds two entries with their time: **Read the repository again** (the history) and
   **Read the worktrees and submodules** (the folders). Reload five times after clicking into the
   window, and note both times; the same for `submodules-20`.

6. Send `warm.txt`, `cold.txt` and the Console times.

## What the lines mean

| Line | Meaning |
|---|---|
| History shown | from asking until the graph, branches and HEAD can show |
| All statuses in | until the last worktree or submodule folder is read |
| Refresh, nothing changed | a refresh for nothing done in the window (a fetch, a change outside) with the repository watched: what the watch saw no change in isn't asked of git again, nor folders that show no change in their HEAD and index |
| Refresh, back to front | the window came back to the front: the same, but every other folder is read again, as files there may have changed unwatched |
| History answer (JSON) | what the history sends to the window |
| Open / Refresh: git processes | how many git processes ran, and the most at once |
| History shown, 2 s status | with `git status` made 2 s slower in every other folder: the history doesn't wait for them |
| Stopped reading the folders in | from asking to stop (another repository opened) until the reading stopped |

The starting targets from the plan: history of a small repository within 1 s, of a 100k-commit
one within 2 s; a stop within 500 ms.
