# Speed: what was measured, and against what

Three kinds of numbers, each from its own script:

- **Reads** (`examples/reads.rs`, `examples/steps.rs`; docs/measuring-reads.md): what Oxbow's
  core takes to read a repository and to stage, without a window.
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
