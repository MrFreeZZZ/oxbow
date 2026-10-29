# Speed: what was measured, and against what

Three kinds of numbers, each from its own script:

- **Reads** (`examples/reads.rs`, `examples/steps.rs`; docs/measuring-reads.md): what Oxbow's
  core takes to read a repository and to stage, without a window; `examples/localhistory.rs`,
  what Local History costs.
- **Oxbow's window** (`scripts/acceptance.sh --speed`): from a real key press or start to what
  the window shows, in the release build. Linux, under Xvfb.
- **Against other clients** (`scripts/compare-clients.py`): the same three actions in each
  client, timed from the screen, on a Mac.

Until the third has numbers from a Mac, Oxbow isn't called the fastest client anywhere: the
first two say only that Oxbow didn't get slower and where it stands against its own targets.

## Reads, again (28 October)

The October measurements run again, old code (5487926, 21 October) and new on the same machine
(cloud, Xeon, 4 cores, git 2.43; warm, 15 runs, median). The earlier saved numbers came from a
faster machine, so only the two columns here compare.

| Fixture | Line | 21 Oct | Now |
|---|---|---|---|
| history-100k | History shown | 45.8 ms | 49.3 ms |
| history-100k | Refresh, nothing changed | 25.1 ms | 4.1 ms, no git process |
| history-100k-clone | History shown | — | 50 ms (750 ms before the fix below) |
| worktrees-20 | History shown | 27.0 ms | 33.6 ms |
| worktrees-20 | All statuses in | 167 ms | 158 ms |
| worktrees-20 | Refresh, nothing changed | 28.6 ms | 6.3 ms |
| worktrees-50 | History shown | 32.2 ms | 39.9 ms |
| worktrees-50 | All statuses in | 364 ms | 324 ms |
| submodules-20 | History shown | 163 ms | 170 ms |
| submodules-20 | All statuses in | 276 ms | 260 ms |
| submodules-20 | Refresh, back to front | 274 ms | 88 ms |

History shown is 4–8 ms slower everywhere, well inside its targets; not traced yet. Refreshes,
which happen far more often, now cost almost nothing.

Found and fixed on the way:

- **A clone's history took 750 ms.** The earlier fixtures had no remote; with origin's branches,
  finding the commits not pushed walked all 100,000 commits. It now walks back from the branches
  only as far as origin has them, as git does (`history.rs`, `only_from`): 50 ms.
  `make-perf-fixtures.sh` now makes `history-100k-clone`, so this stays measured.
- **Stage with Undo got slower in #181**: 67 ms → 87 ms (`examples/steps.rs`, a 15 MB file). Every
  git command waited 20 ms after its output closed; now 1 ms at first (`cli.rs`): 71–76 ms.
- **A worktree's own Git folder was named with `..`** (`.git/worktrees/side/../..`), so a gone
  worktree wasn't matched to its main folder (`watch.rs`, `common_dir`).

## Stage with Undo, and lines of a file (29 October)

`cargo run --release -p oxbow-core --example steps` (2,000 files and one of 15 MB; cloud, Xeon,
4 cores, git 2.43; median of 15). It takes Stage with Undo apart by the git processes it starts,
as the Console lists them; the parts before the action run side by side, so they add up to more
than the whole.

| Measure | Before (df32540) | After |
|---|---|---|
| Stage with Undo | 44–48 ms, 18 git | |
| The same after a refresh (the repository opened again) | 56–61 ms, 25 git: 7 of them ask where git keeps its files | 49 ms, 18 git |
| `git add` alone | 4–5 ms | |
| 20 lines of a 15 MB file on disk, asked again | 16 ms: read and split again each time | 0.003 ms |
| The same of a committed file | 0.002 ms | 0.002 ms |

Where Stage with Undo goes, before: HEAD, refs and stash 4 git (6 ms), the staging area and the
files 5 git (18 ms), `git add` itself (5 ms), writing the step down 8 git (16 ms).

- **A refresh made the next Undo ask git where its files are again.** Each read of the history
  opens the repository again, and that forgot the answers; they are now kept while the Git
  folder is the same (`repo.rs`, `reopen`).
- **Lines of a file on disk were read whole for every page.** They are now kept like a blob's,
  by the file's size, time of change and inode, and forgotten when the watch sees the file
  change; a file changed in the last 2 seconds isn't kept yet, since the system's clock for
  files is coarse. Neither side is read past 32 MB, as a preview (`commit.rs`, `read_lines`).

Then the snapshot by step: the example timed again with a watch, as the window has one, and the
repository opened again before each step. Old (8e4c6f3) and new built side by side and run in
turns, four times each on a busier machine than the table above:

| Stage with Undo | Old | New |
|---|---|---|
| A file just edited | 53–61 ms, 18 git | 39–43 ms, 9 git |
| Files edited earlier, staged one after another | 51–60 ms, 18 git | 27–29 ms, 5 git |
| `git add` alone | 5 ms | 5 ms |

- **A step that only stages reused the last snapshot** while the watch had counted no change of
  files or branches since. Taken back the same day, see below: a file saved a moment before the
  step was not counted yet, and Undo put back the older text.
- **The Operation Log's own ref is no change for the watch**: it moves with every step, so each
  Stage counted as a change and the next one read every file again (`watch.rs`).
- **HEAD, the refs and the stash are read in Oxbow**, not by 4 git processes; writing the step
  down takes 3 git instead of 8: the ref is moved and put back in one `update-ref --stdin`, the
  objects' kinds, the count of files a step changed and the expiry settings are read in Oxbow,
  and git's version is asked once (`config.rs`). The untracked files are listed while the
  tracked ones are added.

What a step after an edit still costs is git processes that write: the tree of the index and of
the files, `git add` itself, the tree of the index after it, and the step's tree, commit and ref.

Then the snapshot before a stage is read whole again (the recheck of the audit, R01). The watch
counts a change only once the system has told it, a moment after the file is written: a file
saved just before Stage wasn't counted yet, the step reused the snapshot without it, and Undo of
that Stage wrote the file back as it was before the save, or deleted a file made just then. No
count of what the watch heard can say the files are as they were, so every step reads them
(`perform.rs`); the tests in `tests/oplog.rs` save and stage with no wait between. The snapshot
after a stage still takes the files from the one before it: staging writes no file. Old (40e7a47)
and new, the same example in turns, four times each (cloud, Xeon, 4 cores, 2,000 files):

| Stage with Undo | Old | New |
|---|---|---|
| A file just edited, not watched | 31–37 ms, 9 git | 29–39 ms, 9 git |
| A file just edited, seen by the watch | 31–38 ms, 9 git | 29–40 ms, 9 git |
| Files edited earlier, staged one after another | 21–25 ms, 5 git | 32–42 ms, 9 git |
| `git add` alone | 4–5 ms | 4–5 ms |

Staging one file after another costs as much as staging right after an edit again, about 10 ms
more than with the reuse; the rest of the gain of that day (3 git to write a step down instead
of 8, HEAD and refs read in Oxbow) stays.

## Local History (29 October)

Before Local History was written, what it would keep was measured on a shallow clone of VS Code
(21,126 files of at most 1 MB, 261 MB; 72 MB compressed as git compresses loose objects), then
again with the code (`cargo run --release -p oxbow-core --example localhistory -- <clone>`; cloud,
Xeon, 4 cores):

| What happens | Kept on disk | Time |
|---|---|---|
| A day: 50 saves of 20 files of 5–100 KB | 6.3 MB | 8 ms a save of all 20 |
| A formatter over every file | 90 MB, once | 3.4 s, in the background (writing the files took 2.8 s) |
| A branch switch writing every file back | 0.2 MB: the index has them | 0.4 s |
| Letting old versions go, then reading the lists | lists 3.9 MB | 84 ms, then 36 ms |

So the limits: versions are kept 7 days (3 or off in Settings › General), at most 256 MB of blobs
a repository with the oldest going first, files over 1 MB are left out (19 in VS Code, bundles
and pictures), and saves of a file within 30 seconds make one version, so an editor saving every
second keeps two a minute, not sixty. The blobs stay outside git's own objects, so `git gc`
neither keeps nor counts them, and nothing a branch switch writes costs a blob.

## Changes of 10,000 files (29 October)

`cargo run --release -p oxbow-core --example reads -- ~/oxbow-perf/changes-10k` (the fixture of
`make-perf-fixtures.sh changes`; cloud, Xeon, 4 cores, git 2.43; median of 7).

| Measure | Before (43098e7) | After |
|---|---|---|
| History shown, with the list of changed files | 1.2 s, 6 git | 92 ms, 3 git |
| Changed lines counted after, while the list already shows | — | 1.0 s |
| Refresh, nothing changed | 28 ms | 21 ms |

`git status` lists the 10,000 files in about 50 ms; `git diff --numstat` of the files not staged
takes 600 ms of the second, as it reads and compares every file. The history no longer waits
for it: the list shows with "…" for each file's lines, the file open counts its own lines with
its diff, and the rest come in one answer after. Running the two `numstat` side by side would
save at most the 50 ms of the staged one, so they still run one after the other.

## Older pages of History (29 October)

`cargo run --release -p oxbow-core --example pages -- ~/oxbow-perf/history-100k-clone` scrolls
History down as the window does (2,000 commits, then twice as many, at most 50,000 more at a
time) and jumps to the oldest commit from a window just opened (cloud, Xeon, 4 cores; median of 3).

| Page | Before (ce060d2) | After | Per 1,000 commits after |
|---|---|---|---|
| 1–2,000 | 55 ms | 35 ms | 18 ms |
| 2,001–4,000 | 50 ms | 34 ms | 17 ms |
| 8,001–16,000 | 138 ms | 94 ms | 12 ms |
| 32,001–64,000 | 682 ms | 447 ms | 14 ms |
| 64,001–100,100 | 1.1 s | 630 ms | 17 ms |
| Jump to the oldest commit | 1.1 s | 1.0 s | |

Each page walked every commit from the tips again; now the walk goes on from where the last page
stopped, so a page walks only its own commits. What still grows with the commits loaded is the
work on all rows: their order, the graph's layout, the rows themselves and comparing them with
the rows the window has, about 250 ms at 100,000. Every row already shown comes back the same
(64,000 of 64,000 on the last page), so the graph doesn't move as older commits come in. A jump
straight to an old commit still walks everything above it once.

The review after the plan (29 October) found that going on from where a page stopped could give
another order than one walk from the tips when commits share a second, as a rebase writes them,
and that a shallow clone's next page failed on the parents it doesn't have. The walk is now
Oxbow's own: newest first, and the greater id first within a second, so the commits waiting
alone decide what comes next; past a shallow boundary no parent is asked for. On the same clone
with and without a commit-graph file, every page takes what it took before, within the noise.

## Repository Files on a big working copy (29 October)

A working copy of 200,000 files (`node_modules`, 400 folders of 500), `.gitignore` checked as it
is typed (cloud, Xeon, 4 cores, warm):

| Check | Before | After |
|---|---|---|
| A key typed while the files are read | waited for the reading, 143 ms | 0.02 ms, the text alone; the rest when read |
| A key typed after | 7 ms | 7 ms |
| Another repository's file meanwhile | waited for the reading too | not held up: each repository is read under its own lock |

Each repository keeps what was read of it (8 repositories at most, and 600,000 files of working
copies together), and leaving the file stops a reading still going.

## Oxbow's window

```sh
scripts/make-perf-fixtures.sh ~/oxbow-perf history
scripts/acceptance.sh --speed ~/oxbow-perf          # RUNS=15 by default
```

Release build under Xvfb with software rendering, same machine; median / p95 of 15 runs:

| Measure | Median / p95 | Target |
|---|---|---|
| Start to the history of `history-100k-clone` | 1777 / 2375 ms | 2 s for 100k commits |
| First diff, ↓ 1.5 s after opening | 195 / 647 ms | |
| Next commit, ↓ (read the first time) | 154 / 1621 ms | |
| Back, ↑ (read before) | 97 / 177 ms | |
| A commit made in a terminal, to its row | 534 / 630 ms (1265 before the fix above) | |
| Reload (F5) when nothing changed, to «Reloaded» | 189 / 244 ms | |

What these include and don't:

- A commit made outside shows after the watch's 300 ms quiet time for git's own files, then a
  read of the history; the rest is the window.
- The ↓ p95 is one slow step out of 15, probably the automatic fetch a few seconds after opening.
- Under Xvfb the first paint after a window opens can wait 1–2.6 s: the diff is in the page
  within 50 ms, but WebKitGTK paints its first frame late with software GL. So the first diff is
  timed 1.5 s after opening; M10 in docs/acceptance.md checks the Mac has no such wait.
- Linux with software rendering is slower than a Mac at painting; these are for comparing Oxbow
  with itself over time, not with other clients.

## Against other clients, on a Mac

`scripts/compare-clients.py` times the same three actions in each client, from the screen: it
puts the client's window at one place, watches that part of the screen, and notes when it first
changed and when it changed for the last time before staying still (1 s; 2 s after a start). So
the clients need nothing but their windows, and each is timed the same way.

| Action | From | First change | Complete |
|---|---|---|---|
| open | the app started (`open -a`), the repository given or reopened | the window | the history shown |
| outside commit | `git commit` in a terminal returned | the new row | the list still |
| next ↓, back ↑ | a key press in the history | the selection moved | the diff painted |

Steps:

1. The clients you want, in /Applications: e.g. Fork, Tower, GitHub Desktop, Sourcetree,
   GitKraken, and Oxbow from the latest build. Open each once, sign-in and welcome screens done.
2. The fixtures: `scripts/make-perf-fixtures.sh ~/oxbow-perf history`. Add
   `~/oxbow-perf/history-100k-clone` in each client once (GitHub Desktop: Add Existing
   Repository), so it opens it again by itself.
3. System Settings › Privacy & Security: allow Terminal in **Screen Recording** and in
   **Accessibility**. Turn on Do Not Disturb, so no notification crosses the window.
4. Put Terminal on the right half of the screen (the clients go at the top left, 1280×800 points;
   on a smaller screen give `--bounds 0,40,1000,700`), and close other apps.
5. In the oxbow folder:

   ```sh
   python3 scripts/compare-clients.py ~/oxbow-perf/history-100k-clone \
     --apps "Oxbow,Fork,Tower,GitHub Desktop,Sourcetree,GitKraken" --out ~/oxbow-perf/compare
   ```

   For each client it opens the repository, puts the window in place, hides other apps and
   starts it 10 times (`--runs`), then makes 10 empty commits «Outside …» in the repository.
   Before ↓ and ↑ it asks you to show the history and click its newest commit (GitHub Desktop:
   the History tab), then press Return in Terminal. About 3 minutes a client.
6. Send `~/oxbow-perf/compare/summary.md` and `results.csv`.

Reading the numbers:

- A client that keeps something moving (a spinner, an animation) never stays still; its
  complete shows «—», and its first change is the time to compare.
- The first open run is with the client's files not yet read from disk; the median is of all.
- Times are a frame or two (10–20 ms) late at most: what the screen showed when it was read.
- The commits «Outside …» stay in the clone; make the fixtures again to start from scratch.
