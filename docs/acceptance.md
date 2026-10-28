# Acceptance: the scenarios users get stuck in

The scenarios the competitor review (report of 21 October, D-12) found other clients get wrong:
staging after a change made outside, conflicts over a restart, two windows, a folder that went
away, a status, snapshot or setting that fails, the keyboard and the sheets. Each is checked on
the real app in two ways: in the cloud by `scripts/acceptance.sh`, and on a Mac by the checklist
below, for what only a Mac shows (FSEvents, ⌘Q, its windows and keyboard layouts, VoiceOver).

| Scenario | In the cloud (`scripts/acceptance.sh`) | Also covered by | Mac |
|---|---|---|---|
| A commit's diff | commit diff | `tests/commit.rs` | M1 |
| Staging after an outside edit | staging after an outside edit | `tests/worktree.rs`, `tests/watch.rs`, `changesView.test.ts` | M2 |
| Conflicts over a restart | conflicts kept over a restart; skip and abort | `tests/merge.rs`, `conflicts.test.ts`, `draftSaves.test.ts` | M3 |
| Two windows | two windows | `session.rs`, `windowTitle.test.ts` | M4 |
| A folder that went away | missing folder | `tests/worktrees.rs`, `gone.test.ts` | M5 |
| Submodules, one fails | submodules, one fails | `tests/submodules.rs`, `submoduleActions.test.ts` | M6 |
| Undo, and a snapshot that fails | undo a discard; an Undo that can't be kept | `tests/oplog.rs`, `tests/notkept.rs` | M7 |
| A status that fails | a status git can't read | `tests/worktrees.rs`, `tests/history.rs` | M8 |
| A setting that can't be saved | a setting that can't be saved | `settings.rs`, `settingSaves.test.ts` | M8 |
| Keys and sheets | keys and modals | `shortcuts.test.ts`, `focus.test.ts` | M9, M10 |
| A file of a commit, the message box | a file of a commit | `tests/edit.rs`, `tests/worktree.rs`, `commitFiles.test.ts`, `messageHistory.test.ts` | — |

## In the cloud

```sh
scripts/acceptance.sh                  # build the app, then run every scenario (about 3 minutes)
scripts/acceptance.sh staging windows  # only the scenarios whose name has one of these words
OXBOW_APP=target/release/oxbow scripts/acceptance.sh   # with a build already made
```

Linux only: the release build runs under Xvfb, driven through `tauri-driver` and WebKitWebDriver
(`sudo apt-get install webkit2gtk-driver xvfb`, `cargo install tauri-driver --locked`), with real
git on repositories made afresh for each scenario (`app/e2e/fixtures.mjs`) and a home folder of
its own, so nothing of yours is read or changed. Clicks and keys are real input; the Russian
layout is typed through `xdotool` when it is there. It prints one line a scenario; a failure's
log and a picture of the window go to `target/acceptance/`. The scenarios are in
`app/e2e/scenarios.mjs`, one object each, in the order of the table.

It isn't part of `scripts/check.sh`: it needs a release build and takes minutes. Run it after a
change to any of the scenarios above.

## On a Mac

Make the repositories (each run makes them afresh), then open each one in Oxbow with ⌘O:

```sh
cd ~/oxbow   # the oxbow folder
node app/e2e/make-repos.mjs ~/oxbow-acceptance
```

Note what doesn't happen as written: the step's number, what you saw, and a screenshot (⇧⌘4).

**M1. A commit's diff** (`history`). Click a commit, then ↓ and ↑ through ten of them.
The diff follows at once; the list scrolls with the selection; nothing flickers.

**M2. Staging after an outside edit** (`staging`). Open Changes and `src/f1.txt`; it has three
changed lines. Stage the line `X3` by its box. Then in Terminal:
`echo 'outside' >> ~/oxbow-acceptance/staging/src/f1.txt`. Within about a second the diff shows
the new line, without clicking anything, and `X3` stays staged
(`git -C ~/oxbow-acceptance/staging diff --cached` shows only it). Pick `X4`, change the file in
Terminal again before staging it, then Stage: the picks are cleared and nothing outdated is staged.

**M3. Conflicts over a restart** (`conflicts`, a rebase stopped on its first step). Open the
conflicts. In `a.txt` take the feature side; in `b.txt` choose Edit and type a few words. Quit
with ⌘Q, start Oxbow again: both files are as you left them and «Commit 1 of 2» shows. Mark both
Resolved and Continue Rebase: step 2 starts, its files undecided. Abort: the branch `feature` is
as before the rebase, with nothing left over (`git status` clean).

**M4. Two windows** (`worktrees` and its worktree `worktrees-side`). Double-click the worktree in
the sidebar: it opens in a second window. ⌘\` moves between them and the Window menu lists both.
Commit in the second window: the first shows the commit in its history within a second. ⇧⌘P
opens the palette only in the window in front.

**M5. A folder that went away** (`gone-side`, the worktree of `gone`). Open `gone-side`, then
move its folder to the Trash in Finder. The window says it «isn’t there any more» and that the
branch `side` and its commits are safe in `gone`; Open Main Folder opens it. Put the folder back
from the Trash and open it again: it reads as before. If you have a USB disk: clone a repository
onto it, open it, eject the disk: the same message, and no error loop.

**M6. Submodules, one fails** (`submodules`). Submodule actions › Update All: «Updated 2 of 3»,
and `lib-c` named as the one that couldn’t be updated. Then
`mv ~/oxbow-acceptance/submodules-gone ~/oxbow-acceptance/submodules-src/lib-c` (its source back) and
Try lib-c Again: it updates.

**M7. Undo** (`undo`: a changed, a new and a staged file). Discard all changes, then ⌘Z: all three
come back as they were, the staged one staged. Then make Undo's folder unusable:
`cd ~/oxbow-acceptance/undo && mv .git/oxbow .git/oxbow-away && touch .git/oxbow`, change a file
and discard it: «Undo isn’t ready for this step», with Continue Without Undo, and the file left as
it was until you choose. `rm .git/oxbow && mv .git/oxbow-away .git/oxbow` afterwards.

**M8. Failures say so** (`history`). Run `chmod a-r ~/oxbow-acceptance/history/.git/index`:
Changes shows «Changes unknown», never «No changes»; `chmod u+r` brings the changes back. Then
`chmod a-w ~/Library/Application\ Support/app.oxbow.desktop` and change any setting: «Couldn’t
save», and the switch goes back. `chmod u+w` the folder afterwards.

**M9. Keys** (`keys`). Switch the input to Russian. ⇧⌘P (⇧⌘З on the keys) opens the palette, ⌘O
opens a folder, ⌘Enter in the commit message commits; type a commit message in Russian, with
ё and «quotes». Back to English: the same keys work.

**M10. Sheets and focus** (`keys`). Discard Changes: the confirmation names the git commands; Tab
stays inside it and Escape cancels, with the focus back on the file. Open and close the palette
with Escape: the focus is where it was. With VoiceOver on (⌘F5), move through the history, the
files and a confirmation: each says what it is. The first click on a commit after opening shows
its diff at once (on Linux under Xvfb the first paint after a window opens can wait 1–2 s; see
docs/speed.md).
