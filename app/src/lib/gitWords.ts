// What the words of a git command mean, for the popover a click on a subcommand or flag opens
// in the terminal blocks. A flag's meaning depends on its command (`-d` deletes a branch but
// lets `clean` take folders), so `sub flag` is looked up before the flag alone.

export interface Meaning {
  /** A few words, e.g. "Push safely over the server’s branch". */
  title: string;
  /** One or two sentences. */
  text: string;
}

const SUBCOMMANDS: Record<string, Meaning> = {
  add: { title: "Stage changes", text: "Copies the files as they are now into the staging area (the index), so the next commit takes them." },
  apply: { title: "Apply a patch", text: "Applies a diff to the files or, with --cached, to the staging area only. Oxbow uses it to stage single lines and hunks." },
  bisect: { title: "Find the commit that broke something", text: "Checks out commits halfway between a good and a bad one, so a few steps find the first bad commit." },
  "bisect start": { title: "Start a bisect", text: "Remembers where you are, so bisect reset can bring you back when it’s over." },
  "bisect good": { title: "This commit works", text: "The bug came later: bisect looks only at newer commits now." },
  "bisect bad": { title: "This commit has the bug", text: "The bug is here or earlier: bisect looks only at older commits now." },
  "bisect skip": { title: "Can’t test this commit", text: "Bisect picks a commit next to it instead." },
  "bisect reset": { title: "End the bisect", text: "Checks out the branch you were on before it started." },
  "bisect run": { title: "Test every step with a command", text: "Runs the command on each commit bisect picks: exit code 0 means good, 125 skip, anything else bad." },
  branch: { title: "Create, rename or delete a branch", text: "Works on the branch name only. Your files and the checked-out branch stay as they are." },
  checkout: { title: "Take files from a commit", text: "With paths after --, writes those files as the commit has them, over your changes." },
  "cherry-pick": { title: "Copy a commit", text: "Makes a new commit on your branch with the same changes and message. The original stays where it was." },
  clean: { title: "Delete untracked files", text: "Removes files git doesn’t track. They were never committed, so git has no copy of them." },
  clone: { title: "Copy a repository", text: "Downloads every commit and branch into a new folder and checks out the default branch." },
  commit: { title: "Save the staged changes", text: "Makes a new commit from what’s in the staging area. Unstaged changes stay in your files." },
  config: { title: "Read or change a setting", text: "Git’s settings live in .git/config for this repository and in ~/.gitconfig for all of them." },
  fetch: { title: "Download, change nothing", text: "Brings new commits and branches from the server into origin/… names. Your branches and files stay as they are." },
  gc: { title: "Tidy the repository", text: "Packs loose objects into fewer files and drops what nothing points to anymore. Commits you can reach stay." },
  init: { title: "Make a new repository", text: "Creates the .git folder that holds history. Nothing is committed yet." },
  lfs: { title: "Git Large File Storage", text: "Keeps big files on an LFS server; the repository holds a small pointer file in their place." },
  "lfs track": { title: "Store files like these in LFS", text: "Adds the pattern to .gitattributes, so matching files you commit from now on go to LFS." },
  "lfs untrack": { title: "Stop storing these in LFS", text: "Removes the pattern from .gitattributes. Files already in LFS stay there." },
  "lfs install": { title: "Turn LFS on", text: "Adds the LFS filters to your git settings, once per computer." },
  "lfs prune": { title: "Free disk space", text: "Deletes local copies of LFS files that old commits use and the server already has." },
  merge: { title: "Join another branch in", text: "Brings that branch’s commits into yours: a fast-forward when yours has nothing new, else a merge commit with two parents." },
  pull: { title: "Fetch, then bring the commits in", text: "Downloads the upstream’s new commits and merges or rebases your branch onto them." },
  push: { title: "Upload your commits", text: "Sends the commits the server doesn’t have and moves its branch to yours. Only fast-forwards, unless forced." },
  rebase: { title: "Replay commits on a new base", text: "Takes your commits off, moves the branch to the new base and makes each commit again there. They get new SHAs." },
  reflog: { title: "Where HEAD and branches have been", text: "A local diary of every move. It finds commits that no branch keeps anymore." },
  remote: { title: "Manage servers", text: "Names the servers this repository pushes to and fetches from, like origin." },
  "remote add": { title: "Add a server", text: "Remembers the URL under a name. Nothing is downloaded until you fetch." },
  "remote remove": { title: "Forget a server", text: "Drops the name and its origin/… branches here. The server itself is not touched." },
  "remote set-url": { title: "Change a server’s address", text: "Fetch and push use the new URL from now on." },
  reset: { title: "Move the branch", text: "Points the checked-out branch at another commit. --soft, --mixed and --hard decide what happens to your files." },
  restore: { title: "Put files back", text: "Writes files as HEAD or the staging area has them, over your changes." },
  revert: { title: "Undo a commit with a new one", text: "Makes a new commit with the opposite changes. History stays as it is, so it’s safe for pushed commits." },
  rm: { title: "Stop tracking a file", text: "Removes the file from the staging area, and from the folder unless --cached." },
  stash: { title: "Put changes aside", text: "Saves uncommitted changes in a stash and cleans the files, so you can switch branches." },
  "stash push": { title: "Make a stash", text: "Saves your changes and puts the files back as HEAD has them." },
  "stash pop": { title: "Bring a stash back and delete it", text: "Applies the changes to your files; the stash goes away only if they applied without conflicts." },
  "stash apply": { title: "Bring a stash back and keep it", text: "Applies the changes to your files. The stash stays, so you can apply it again elsewhere." },
  "stash drop": { title: "Delete a stash", text: "Its changes are gone from the list, though git keeps the commit for a while." },
  "stash store": { title: "Add a stash", text: "Puts an existing stash commit back on the list." },
  switch: { title: "Check out a branch", text: "Makes it the current branch and updates your files to its latest commit. Uncommitted changes come along if they don’t clash." },
  tag: { title: "Name a commit for good", text: "A tag stays on its commit, unlike a branch that moves on with each new commit." },
  worktree: { title: "More working folders", text: "Checks out another branch in a second folder that shares this repository’s history." },
  "worktree add": { title: "Make a working folder", text: "Creates the folder and checks out the branch there. A branch can be open in one folder at a time." },
  "worktree remove": { title: "Delete a working folder", text: "Deletes the folder. The branch and its commits stay in the repository." },
  "worktree prune": { title: "Forget missing folders", text: "Drops worktrees whose folder was deleted by hand." },
  "worktree lock": { title: "Keep a folder from prune", text: "For a folder on a disk that is sometimes unplugged." },
  "worktree unlock": { title: "Allow prune again", text: "The folder can be pruned once it’s gone." },
  "worktree repair": { title: "Reconnect a moved folder", text: "Fixes the links between the repository and a worktree folder that was moved." },
};

const FLAGS: Record<string, Meaning> = {
  // Before the subcommand.
  "-c": { title: "A setting for this command only", text: "Like a line in .git/config, but just for this one run." },
  "--": { title: "Paths follow", text: "Everything after it is a file name, even if it looks like a flag or a branch." },
  "-q": { title: "Quiet", text: "Prints only errors." },
  "--quiet": { title: "Quiet", text: "Prints only errors." },
  "--no-verify": { title: "Skip the hooks", text: "Runs without the repository’s pre-commit, commit-msg or pre-push scripts." },
  "--abort": { title: "Give up and go back", text: "Stops the operation and puts the branch and files back as they were before it started." },
  "--continue": { title: "Go on after conflicts", text: "Uses the files you resolved and staged, and carries on with the next step." },
  "--skip": { title: "Leave this commit out", text: "Drops the commit that stopped with conflicts and carries on with the next one." },
  "--no-edit": { title: "Keep the message", text: "Uses the prepared message without opening an editor." },
  "-m": { title: "The message", text: "The text that follows is the message, so no editor opens." },

  "push --force-with-lease": {
    title: "Force, but only over what you saw",
    text: "Replaces the server’s branch only if it’s still where your last fetch saw it. If someone pushed since, git refuses instead of wiping their commits.",
  },
  "push --force": { title: "Replace the server’s branch", text: "Moves it to yours even if that drops commits it had. Anyone else’s new commits there are lost." },
  "push -f": { title: "Replace the server’s branch", text: "Moves it to yours even if that drops commits it had. Anyone else’s new commits there are lost." },
  "push -u": { title: "Remember the upstream", text: "Links your branch to the server’s, so Pull, Push and ahead/behind work without naming it." },
  "push --set-upstream": { title: "Remember the upstream", text: "Links your branch to the server’s, so Pull, Push and ahead/behind work without naming it." },
  "push --delete": { title: "Delete on the server", text: "Removes the branch or tag there. Your local copy stays." },
  "push --tags": { title: "Push every tag", text: "Sends all local tags the server doesn’t have." },
  "push --follow-tags": { title: "Push tags too", text: "Sends annotated tags on the commits being pushed." },

  "commit --amend": {
    title: "Replace the last commit",
    text: "Makes a new commit in place of the last one, with your staged changes added. It gets a new SHA, so a pushed one then needs a force push.",
  },
  "commit --allow-empty": { title: "Commit with no changes", text: "Makes a commit even though the files are the same as in the last one." },
  "commit --fixup": { title: "A fix for an older commit", text: "Its message starts with fixup!, so rebase --autosquash folds it into that commit." },
  "commit -a": { title: "Stage tracked files too", text: "Commits every change to files git already tracks, staged or not." },
  "commit --only": { title: "Just these files", text: "Commits only the listed paths and leaves the rest of the staging area as it was." },

  "rebase --autostash": { title: "Stash first, bring back after", text: "Puts uncommitted changes aside before the rebase and applies them again when it ends." },
  "rebase --update-refs": { title: "Move stacked branches too", text: "Branches pointing at commits being replayed move to the new copies, so a stack stays together." },
  "rebase --onto": { title: "Put the commits on this base", text: "Replays the commits after the next argument onto the commit given here, leaving the rest out." },
  "rebase -i": { title: "Edit the list of commits", text: "Follows a todo list: pick, reword, squash, fixup or drop each commit. Oxbow writes the list instead of opening an editor." },
  "rebase --interactive": { title: "Edit the list of commits", text: "Follows a todo list: pick, reword, squash, fixup or drop each commit. Oxbow writes the list instead of opening an editor." },
  "rebase --autosquash": { title: "Fold the fixups in", text: "Moves fixup! commits next to the commit they fix and squashes them into it." },

  "merge --ff-only": { title: "Fast-forward or nothing", text: "Only moves your branch ahead. If both sides have new commits, git stops instead of making a merge commit." },
  "merge --no-ff": { title: "Always a merge commit", text: "Makes a merge commit even when a fast-forward would do, so the branch shows in history." },
  "merge --squash": { title: "All changes, one commit", text: "Stages the branch’s changes as one diff. You commit it; the result has one parent, so the branch doesn’t look merged." },

  "pull --rebase": { title: "Rebase instead of merge", text: "Replays your new commits on top of what the server has, so history stays a straight line." },
  "pull --no-rebase": { title: "Merge what comes in", text: "Joins the server’s commits with a merge commit when both sides have new ones." },
  "pull --ff-only": { title: "Fast-forward or nothing", text: "Pulls only if your branch has no commits the server lacks." },
  "pull --autostash": { title: "Stash first, bring back after", text: "Puts uncommitted changes aside while pulling and applies them again after." },

  "fetch --prune": { title: "Drop deleted branches", text: "Removes origin/… names of branches deleted on the server. Your own branches stay." },
  "fetch --all": { title: "Every server", text: "Fetches from all remotes, not only one." },
  "fetch --tags": { title: "Every tag", text: "Downloads all tags, even on commits no branch has." },
  "fetch --unshallow": { title: "Download the whole history", text: "This clone stops at a depth; this brings the older commits too." },
  "fetch --depth": { title: "Only the latest commits", text: "Downloads history only this many commits deep." },

  "reset --soft": { title: "Move the branch only", text: "The branch points at the commit; the changes of the commits it leaves stay staged, ready to commit again." },
  "reset --mixed": { title: "Move the branch, unstage", text: "The branch points at the commit; the changes stay in your files, unstaged." },
  "reset --hard": { title: "Move the branch, reset the files", text: "The branch and your files become exactly the commit. Uncommitted changes to tracked files are lost." },
  "reset --merge": { title: "Back out of a merge", text: "Resets the files a merge touched and keeps your other uncommitted changes." },

  "switch --detach": { title: "No branch, just the commit", text: "Checks out the commit itself (detached HEAD). New commits belong to no branch until you make one." },
  "switch -c": { title: "Create the branch and switch", text: "Makes a new branch at the commit and checks it out." },
  "switch --create": { title: "Create the branch and switch", text: "Makes a new branch at the commit and checks it out." },
  "switch --track": { title: "A local copy of a server branch", text: "Creates the branch from origin/… and remembers it as the upstream." },
  "switch --discard-changes": { title: "Lose uncommitted changes", text: "Switches even if your changes clash, throwing them away." },

  "stash --include-untracked": { title: "New files too", text: "Stashes untracked files as well, not only changes to tracked ones." },
  "stash -u": { title: "New files too", text: "Stashes untracked files as well, not only changes to tracked ones." },
  "stash --keep-index": { title: "Keep the staged changes", text: "Stashes everything but leaves what is staged in place." },
  "stash --index": { title: "Restore what was staged", text: "Brings the staged part back staged, not mixed in with the rest." },

  "clean -f": { title: "Really delete", text: "Git refuses to delete untracked files without it." },
  "clean -d": { title: "Folders too", text: "Deletes untracked folders as well as files." },
  "clean -fd": { title: "Delete files and folders", text: "Deletes untracked files and folders. They were never committed, so git has no copy." },
  "clean -x": { title: "Ignored files too", text: "Deletes files .gitignore hides as well, like build output or a .env file." },

  "revert -m": { title: "Which parent to keep", text: "For a merge commit: 1 keeps the branch it was merged into and undoes what the merge brought in." },
  "cherry-pick -x": { title: "Note the original", text: "Adds “cherry picked from commit …” to the message." },
  "cherry-pick --empty": { title: "Commits with no changes", text: "keep makes them anyway; by default git stops on them." },
  "cherry-pick -m": { title: "Which parent to compare with", text: "For a merge commit: the changes are taken against this parent." },

  "branch -d": { title: "Delete a merged branch", text: "Refuses if the branch has commits no other branch has." },
  "branch -D": { title: "Delete the branch anyway", text: "Deletes it even with commits no other branch has. They stay in the reflog for a while." },
  "branch --delete": { title: "Delete a merged branch", text: "Refuses if the branch has commits no other branch has." },
  "branch -m": { title: "Rename", text: "Gives the branch a new name; its commits stay as they are." },
  "branch -f": { title: "Move the branch", text: "Points an existing branch at another commit." },
  "branch -u": { title: "Set the upstream", text: "Links the branch to a server branch for Pull, Push and ahead/behind." },
  "branch --set-upstream-to": { title: "Set the upstream", text: "Links the branch to a server branch for Pull, Push and ahead/behind." },
  "branch --unset-upstream": { title: "Forget the upstream", text: "The branch stops following a server branch." },

  "tag -a": { title: "An annotated tag", text: "Stores who tagged, when and a message, like a release note." },
  "tag -d": { title: "Delete the tag", text: "Removes it here only; the server keeps its copy until you delete it there." },
  "tag -f": { title: "Move the tag", text: "Points an existing tag at another commit." },

  "add -A": { title: "Everything", text: "Stages new, changed and deleted files in the whole repository." },
  "add --all": { title: "Everything", text: "Stages new, changed and deleted files in the whole repository." },
  "add -u": { title: "Tracked files only", text: "Stages changes and deletions, but no new files." },
  "add -N": { title: "Will be added", text: "Lists a new file in the staging area without its content, so diff shows it." },

  "apply --cached": { title: "Staging area only", text: "Applies the patch to the index, not to your files: that’s how lines get staged." },
  "apply -R": { title: "Backwards", text: "Applies the opposite of the patch, e.g. to unstage or discard those lines." },
  "apply --reverse": { title: "Backwards", text: "Applies the opposite of the patch, e.g. to unstage or discard those lines." },
  "apply --unidiff-zero": { title: "Patches without context", text: "Accepts hunks that have no unchanged lines around them." },
  "apply --recount": { title: "Count the lines again", text: "Trusts the patch’s lines over the numbers in its @@ headers." },

  "restore --staged": { title: "Unstage", text: "Puts the staging area back as HEAD has it. Your files keep the changes." },
  "restore --worktree": { title: "Your files", text: "Writes the files back as the staging area has them, losing unstaged changes." },
  "restore --source": { title: "From this commit", text: "Takes the files as this commit has them." },

  "checkout --ours": { title: "Your side", text: "During conflicts: takes the file as the checked-out branch has it." },
  "checkout --theirs": { title: "Their side", text: "During conflicts: takes the file as the branch coming in has it." },

  "rm --cached": { title: "Untrack, keep the file", text: "Git forgets the file; it stays in your folder as an untracked file." },

  "worktree --force": { title: "Even with changes", text: "Removes the folder even though it has uncommitted changes." },
  "worktree -b": { title: "With a new branch", text: "Creates the branch and checks it out in the new folder." },
  "worktree --verbose": { title: "Say what it does", text: "Lists each worktree it prunes." },

  "clone --depth": { title: "Only the latest commits", text: "A shallow clone: faster, with older history left on the server." },
  "clone --branch": { title: "Check out this branch", text: "Instead of the server’s default branch." },
  "clone -b": { title: "Check out this branch", text: "Instead of the server’s default branch." },
  "clone --recurse-submodules": { title: "Submodules too", text: "Clones the repositories this one includes, too." },
  "init -b": { title: "Name of the first branch", text: "The branch the first commit goes on, e.g. main." },
  "init --initial-branch": { title: "Name of the first branch", text: "The branch the first commit goes on, e.g. main." },

  "lfs prune --verify-remote": { title: "Check the server first", text: "Deletes a local copy only after making sure the LFS server has it." },
  "lfs --verify-remote": { title: "Check the server first", text: "Deletes a local copy only after making sure the LFS server has it." },
  "lfs --dry-run": { title: "Only say what it would do", text: "Lists what it would delete, and deletes nothing." },
};

/** The words of a git command, each with the subcommand it belongs to. `sub` is e.g. "push" or
 *  "stash push"; `kind` says what the word is. */
export interface Word {
  text: string;
  kind: "program" | "sub" | "flag" | "value" | "string" | "arg";
  sub: string | null;
}

const MULTI = new Set(["stash", "worktree", "lfs", "remote", "bisect"]);

/** Split a shell-quoted command into words and tell subcommands, flags and arguments apart. */
export function words(display: string): Word[] {
  const parts = display.match(/'(?:[^']|'\\'')*'|"[^"]*"|\S+/g) ?? [];
  const out: Word[] = [];
  let sub: string | null = null;
  let paths = false;
  let afterC = false;
  for (const [i, text] of parts.entries()) {
    if (i === 0) {
      out.push({ text, kind: "program", sub: null });
      continue;
    }
    if (parts[0] !== "git") {
      out.push({ text, kind: text.startsWith("-") ? "flag" : /^['"]/.test(text) ? "string" : "arg", sub: null });
      continue;
    }
    if (afterC) {
      out.push({ text, kind: "value", sub });
      afterC = false;
    } else if (!sub && text.startsWith("-")) {
      out.push({ text, kind: "flag", sub: null });
      afterC = text === "-c" || text === "-C";
    } else if (!sub) {
      sub = text;
      out.push({ text, kind: "sub", sub: null });
    } else if (MULTI.has(sub) && !sub.includes(" ") && !text.startsWith("-") && out[out.length - 1].kind === "sub") {
      // `stash push`, `worktree add`: the second word names the subcommand's own command.
      sub = `${sub} ${text}`;
      out.push({ text, kind: "sub", sub: null });
    } else if (!paths && text.startsWith("-")) {
      out.push({ text, kind: "flag", sub });
      if (text === "--") paths = true;
    } else {
      out.push({ text, kind: /^['"]/.test(text) ? "string" : "arg", sub });
    }
  }
  return out;
}

/** What a subcommand or flag means, or null when the popover has nothing to say. */
export function meaning(word: Word, all: Word[]): Meaning | null {
  if (word.kind === "sub") {
    const index = all.indexOf(word);
    const next = all[index + 1];
    // The first of two words (`stash` of `stash push`) gets the general meaning.
    const own = all.slice(1, index + 1).filter((w) => w.kind === "sub").map((w) => w.text).join(" ");
    return SUBCOMMANDS[own] ?? (next?.kind === "sub" ? null : SUBCOMMANDS[own.split(" ")[0]]) ?? null;
  }
  if (word.kind !== "flag") return null;
  const flag = word.text.replace(/=.*$/, "");
  const sub = word.sub ?? "";
  const top = sub.split(" ")[0];
  return FLAGS[`${sub} ${flag}`] ?? FLAGS[`${top} ${flag}`] ?? (word.sub === null || GENERAL.has(flag) ? FLAGS[flag] : null) ?? combined(top, flag);
}

/** Flags that mean the same for every command. */
const GENERAL = new Set(["--", "-q", "--quiet", "--no-verify", "--abort", "--continue", "--skip", "--no-edit", "-m"]);

/** `-fd` is `-f` and `-d` together. */
function combined(sub: string, flag: string): Meaning | null {
  if (!/^-[a-zA-Z]{2,}$/.test(flag)) return null;
  const parts = [...flag.slice(1)].map((c) => FLAGS[`${sub} -${c}`]);
  if (parts.some((p) => !p)) return null;
  return { title: parts.map((p) => p!.title).join(" + "), text: parts.map((p) => p!.text).join(" ") };
}

/** Where the command's manual is, as typed in a terminal. */
export function manual(all: Word[]): string | null {
  const top = all.find((w) => w.kind === "sub")?.text;
  return all[0]?.text === "git" && top ? `git help ${top}` : null;
}
