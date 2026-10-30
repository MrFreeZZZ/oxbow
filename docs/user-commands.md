# Custom commands

Settings › Custom Commands keeps personal scripts in `oxbow.commands` in Oxbow’s
`settings.json`. Create, edit or delete them there, or choose **Custom: Manage Commands…**
in the Command Palette. The repository toolbar’s **Commands** button opens the same
palette filtered to custom commands. Narrow windows include them in **More**.

Give each command a name and a shell script. Leave **Repository path** empty to offer it
in every repository, or enter the exact worktree root path to offer it only there.
Commands are personal: Oxbow does not import executable settings from committed files.
They are never run on opening a repository. Assign individual keys in Settings › Keyboard
Shortcuts › Custom Commands; the existing conflict checks and chord bindings apply.

A command runs in the original repository root in your login shell (`cmd /C` on Windows).
It receives these environment variables, even when their values are empty:

| Variable | Value |
| --- | --- |
| `OXBOW_REPOSITORY` | Absolute worktree root |
| `OXBOW_BRANCH` | First local branch label on the selected commit, otherwise the checked-out branch; empty for detached HEAD without a selected branch label |
| `OXBOW_COMMIT` | Selected history commit, otherwise HEAD; empty in an unborn repository |

Context describes the selection. Running a command does not check out that branch or
commit. Quote environment variables as data, for example on macOS:

```sh
printf '%s\n' "$OXBOW_BRANCH" "$OXBOW_COMMIT"
git show "$OXBOW_COMMIT" --
```

Oxbow passes context through process environment values without inserting it into shell
source. Your script itself is shell source and can run any program, change files, or access
the network. In Restricted mode commands are blocked; trust the repository first using
Oxbow’s existing trust controls. Arbitrary script changes have no Oxbow Undo.

Commands take a repository turn alongside other changes. **Stop** cancels a queued run
or kills the running process group, including its children. A run captures its repository
before waiting; switching or closing that session never retargets it. Closing the application
window is not a substitute for Stop.

**Show Console** opens the existing Console with live output, command, exit code and stopped
state. Its normal output limits and masking apply. Masking is best effort: do not put secrets
in names, scripts or output. Nonzero exit codes are reported as failures. A damaged
`settings.json` is never overwritten by saving a command; fix it in **Edit in settings.json**.
