// The right-click menu of a commit in the graph: what can be done with it, from where the
// repository is now. While bisecting it answers about the commit instead.

import type { History, HistoryRow } from "./types";
import type { BisectMark } from "./types";
import type { Request } from "./confirm.svelte";
import { menuIcons, type MenuEntry } from "./Menu.svelte";
import { detachRequest, keepRequest, newBranchRequest, switchRequest, type BranchContext } from "./branches";
import { mergeRequest } from "./merge";
import { savePatchesRequest, type PatchTarget } from "./patches";
import { stashMenu, stashRequest } from "./stash";
import { ancestors, cherryPickRequest, editMessageRequest, resetModes, resetRequest, revertRequest, undoCommitRequest } from "./commits";
import { deleteTagRequest, isLocalTag, newTagRequest, pushTagsRequest } from "./tags";
import { addToCommitRequest, dropCommitRequest, rewordRequest, squashRequest } from "./stack";
import { bisectMarkRequest, bisectResetRequest, bisectStartRequest } from "./bisect";
import { keys } from "./keys";
import { github } from "./github.svelte";
import { branchMergeRequest, gitlabOf, signedInTo } from "./mergeRequest";
import { dropsPushed, lockedCommit, lockedNote, protectedHead } from "./protect";

/** What the menu needs from the window: the repository as shown, and what its items do. */
export interface CommitMenuApp {
  history: History | null;
  ctx: BranchContext | null;
  /** The checked-out branch's own commits, above the trunk, which can be rewritten in place. */
  headStack: Set<string>;
  run: (request: Request | Promise<Request>) => unknown;
  copyText: (text: string, done: string) => void;
  copyMessage: (id: string) => void;
  openStack: (branch: string | null) => void;
  /** Split Commit's sheet on a commit of `headStack`. */
  openSplit: (row: HistoryRow) => void;
  openFixes: (commit: string) => void;
  openCompare: (base: string, target: string) => void;
  /** Browse Files at This Commit; `label` the branch or tag that ends there. */
  openBrowse: (rev: string, label: string | null) => void;
  /** The commits Save as Patches writes for a row. */
  patchTarget: (row: HistoryRow) => PatchTarget;
  /** The Pull Request screen of a branch: its pull request, or the form for a new one. */
  /** The Pull Request screen of `branch`; `forge` "gitlab": its merge request. */
  openPull: (branch: string, number?: number | null, forge?: "github" | "gitlab") => void;
}

/** The right-click menu of a commit in the graph. */
export function commitMenu(row: HistoryRow, app: CommitMenuApp): MenuEntry[] {
  const { history, ctx, headStack, run, copyText, copyMessage } = app;
  if (!ctx || !history) return [];
  if (row.worktree) {
    return [
      { kind: "header", label: row.summary },
      { kind: "item", label: "Stash Changes…", icon: menuIcons.stash, run: () => run(stashRequest(ctx)) },
    ];
  }
  const stash = history.stashes.find((s) => s.id === row.id);
  if (stash) return stashMenu(ctx, stash, run, (text) => copyText(text, `Copied ${text.length > 40 ? "it" : text}.`));
  const item = (label: string, icon: string, act: () => void, danger = false): MenuEntry => ({ kind: "item", label, icon, run: act, danger });
  if (history.bisect) return bisectMenu(row, ctx, item, history, app);
  const isHead = row.id === history.head.commit;
  const here = history.head.branch ?? "HEAD";
  const entries: MenuEntry[] = [{ kind: "header", label: `${row.id.slice(0, 7)} · ${row.summary}` }];
  const group = (items: MenuEntry[]) => {
    if (items.length) entries.push(...(entries.length > 1 ? [{ kind: "sep" } as MenuEntry] : []), ...items);
  };
  // A commit many branches point at lists the first few; the rest are in the branch list.
  const others = row.labels.filter((l) => l.kind === "local" && l.name !== history!.head.branch);
  const switching: MenuEntry[] = others.slice(0, 3).map((label) => item(`Check Out ${label.name}`, menuIcons.checkout, () => run(switchRequest(ctx, label.name))));
  if (others.length > 3) switching.push({ kind: "note", label: `${others.length - 3} more branches here, in the branch list.` });
  if (!isHead) switching.push(item("Check Out This Commit…", menuIcons.checkout, () => run(detachRequest(ctx, row))));
  group(switching);

  // Branches at this commit can be merged or rebased onto right here, as from the branch list;
  // the commit itself can be copied, undone or made the branch's tip. None of it while a merge
  // or rebase waits to be finished.
  if (!history.operation) {
    const locals = new Set(row.labels.filter((l) => l.kind === "local").map((l) => l.name));
    const mergeable = row.labels
      .filter((l) => (l.kind === "local" || (l.kind === "remote" && !locals.has(l.name.slice(l.name.indexOf("/") + 1)))) && l.name !== here)
      .slice(0, 2);
    // A protected branch isn't rebased, reset or edited where that would rewrite pushed commits.
    const inHead = ancestors(history, history.head.commit).has(row.id);
    const locked = inHead && lockedCommit(history, row);
    const dropping = dropsPushed(history, row.id);
    const merging: MenuEntry[] = [];
    for (const label of mergeable) {
      merging.push(item(`Merge ${label.name} into ${here}…`, menuIcons.merge, () => run(mergeRequest(ctx, label.name))));
      if (history.head.branch && !dropping) merging.push(item(`Rebase ${here} onto ${label.name}…`, menuIcons.rebase, () => run(mergeRequest(ctx, label.name, "rebase"))));
    }
    group(merging);

    const editing: MenuEntry[] = [];
    if (isHead && row.parents.length === 1 && !locked) editing.push(item("Undo Commit", menuIcons.undo, () => run(undoCommitRequest(ctx, row))));
    if (isHead && !locked) editing.push(item("Edit Message…", menuIcons.edit, () => run(editMessageRequest(ctx, row))));
    // The checked-out branch's own commits, above the trunk, can be rewritten in place.
    const own = headStack.has(row.id);
    if (own && !isHead) {
      editing.push(item("Edit Message…", menuIcons.edit, () => run(rewordRequest(ctx, row))));
      const staged = history.rows[0]?.worktree?.staged ?? 0;
      if (staged) editing.push(item("Add Staged Changes to This Commit…", menuIcons.stage, () => run(addToCommitRequest(ctx, row))));
    }
    if (own && row.parents.length === 1) editing.push(item("Split Commit…", menuIcons.split, () => app.openSplit(row)));
    if (own && headStack.has(row.parents[0] ?? "")) editing.push(item("Squash into Previous…", menuIcons.merge, () => run(squashRequest(ctx, row))));
    if (own) editing.push(item("Drop Commit…", menuIcons.drop, () => run(dropCommitRequest(ctx, row)), true));
    if (own) editing.push(item("Edit Stack…", menuIcons.rebase, () => app.openStack(null)));
    if (!inHead && history.head.commit) editing.push(item(`Cherry-Pick onto ${here}`, menuIcons.cherry, () => run(cherryPickRequest(ctx, row))));
    if (inHead) editing.push(item("Revert Commit…", menuIcons.revert, () => run(revertRequest(ctx, row))));
    if (inHead && history.head.branch) editing.push(item("Fix a Mistake…", menuIcons.fix, () => app.openFixes(row.id)));
    if (!isHead && history.head.commit && !dropping) {
      editing.push({
        kind: "sub",
        label: `Reset ${here} to Here`,
        icon: menuIcons.reset,
        entries: resetModes.map((m) => ({ label: m.label, hint: m.keeps, danger: m.mode === "hard", run: () => run(resetRequest(ctx, row, m.mode)) })),
      });
    }
    const branch = protectedHead(history);
    if (branch && (locked || dropping)) editing.push({ kind: "note", label: lockedNote(branch) });
    group(editing);

    // Bisect: from a branch tip (or HEAD) back to a tag, or from an older commit up to HEAD.
    const finding: MenuEntry[] = [];
    if (isHead || row.labels.some((l) => l.kind === "local")) finding.push(item("Find What Broke This…", menuIcons.bisect, () => run(bisectStartRequest(ctx, { bad: row }))));
    else if (inHead) finding.push(item("Find First Bad Commit After This…", menuIcons.bisect, () => run(bisectStartRequest(ctx, { good: row }))));
    group(finding);
  }

  // Pull and merge requests of the branches that end here, as from the branch list.
  const reviewing: MenuEntry[] = [];
  for (const ref of history.refs.filter((r) => r.kind === "local" && r.target === row.id && r.name !== history!.trunk).slice(0, 2)) {
    if (github.repo) {
      const pull = github.pullOf(ref.name);
      reviewing.push(item(pull ? `Pull Request #${pull.number} of ${ref.name}` : `Create Pull Request for ${ref.name}…`, menuIcons.pull, () => app.openPull(ref.name)));
    }
    const project = gitlabOf(history, github.gitlab, ref);
    const mr = project ? github.mergeRequestOf(ref.name) : null;
    if (mr) reviewing.push(item(`Merge Request !${mr.number} of ${ref.name}`, menuIcons.pull, () => app.openPull(ref.name, null, "gitlab")));
    else if (project) reviewing.push(item(`Create Merge Request for ${ref.name}…`, menuIcons.pull, () => (signedInTo(project) ? app.openPull(ref.name, null, "gitlab") : run(branchMergeRequest(ctx, ref, project)))));
  }
  // A branch only a GitLab remote has, e.g. a teammate's: its New merge request page.
  const locals = new Set(history.refs.filter((r) => r.kind === "local").map((r) => r.name));
  for (const ref of history.refs.filter((r) => r.kind === "remote" && r.target === row.id).slice(0, 2)) {
    const project = github.gitlab.find((p) => ref.name.startsWith(`${p.remote}/`));
    const name = project ? ref.name.slice(project.remote.length + 1) : "";
    if (!project || name === "HEAD" || name === history.trunk || locals.has(name)) continue;
    const mr = github.mergeRequestOf(name);
    if (mr) reviewing.push(item(`Merge Request !${mr.number} of ${ref.name}`, menuIcons.pull, () => app.openPull(name, null, "gitlab")));
    else reviewing.push(item(`Create Merge Request for ${ref.name}…`, menuIcons.pull, () => run(branchMergeRequest(ctx, ref, project))));
  }
  group(reviewing);

  group([isHead && !history.head.branch ? item("Create Branch…", menuIcons.branch, () => run(keepRequest(ctx))) : item("New Branch from Here…", menuIcons.branch, () => run(newBranchRequest(ctx, undefined, row)))]);
  // Tags: a new one here, and pushing or deleting the ones on this commit.
  // Push Tag and Delete Tag list the commit's tags in submenus, like Reset and Copy.
  const tagging: MenuEntry[] = [item("New Tag Here…", menuIcons.tag, () => run(newTagRequest(ctx, row)))];
  const tagsHere = row.labels.filter((l) => l.kind === "tag").map((l) => l.name);
  const unpushed = history.defaultRemote ? tagsHere.filter((name) => isLocalTag(ctx, name)) : [];
  if (unpushed.length) {
    tagging.push({
      kind: "sub",
      label: `Push Tag to ${history.defaultRemote}`,
      icon: menuIcons.push,
      entries: [
        ...unpushed.map((name) => ({ label: name, run: () => run(pushTagsRequest(ctx, [name])) })),
        ...(unpushed.length > 1 ? [{ label: `All ${unpushed.length}`, run: () => run(pushTagsRequest(ctx, unpushed)) }] : []),
      ],
    });
  }
  if (tagsHere.length) {
    tagging.push({
      kind: "sub",
      label: "Delete Tag",
      icon: menuIcons.drop,
      entries: tagsHere.map((name) => ({ label: `${name}…`, danger: true, run: () => run(deleteTagRequest(ctx, name)) })),
    });
  }
  group(tagging);
  const saving = app.patchTarget(row);
  group([item(saving.since ? `Save ${saving.count} Commits as Patches…` : "Save as Patch…", menuIcons.patch, () => run(savePatchesRequest(saving)))]);
  // Compare this commit, named by its branch when one ends here, with HEAD or the default branch.
  const name = row.labels.find((l) => l.kind === "local")?.name ?? row.id.slice(0, 7);
  const against = [...new Set([isHead ? null : here, history.trunk])].filter((b): b is string => !!b && b !== name);
  const looking: MenuEntry[] = [browseItem(row, item, app)];
  if (against.length) looking.push({ kind: "sub", label: "Compare", icon: menuIcons.compare, entries: against.map((b) => ({ label: `With ${b}`, run: () => app.openCompare(b, name) })) });
  group(looking);
  group([
    {
      kind: "sub",
      label: "Copy",
      icon: menuIcons.copy,
      entries: [
        { label: "SHA", hint: row.id.slice(0, 7) + "…", run: () => copyText(row.id, `Copied ${row.id.slice(0, 7)}.`) },
        { label: "Short SHA", hint: row.id.slice(0, 7), run: () => copyText(row.id.slice(0, 7), `Copied ${row.id.slice(0, 7)}.`) },
        { label: "Message", run: () => copyMessage(row.id) },
      ],
    },
  ]);
  return entries;
}

/** The commit menu while bisecting: answers about the commit, Stop or Finish, and the
 *  things that don't move HEAD. */
function bisectMenu(
  row: HistoryRow,
  ctx: BranchContext,
  item: (label: string, icon: string, act: () => void, danger?: boolean) => MenuEntry,
  history: History,
  app: CommitMenuApp,
): MenuEntry[] {
  const { run, copyText, copyMessage } = app;
  const bisect = history.bisect!;
  const entries: MenuEntry[] = [{ kind: "header", label: `${row.id.slice(0, 7)} · ${row.summary}` }];
  if (!bisect.found) {
    const testing = row.id === history.head.commit;
    const mark = (label: string, icon: string, kind: BisectMark, key: string): MenuEntry => ({ ...item(label, icon, () => run(bisectMarkRequest(ctx, kind, row))), ...(testing ? { keys: keys(key) } : {}) });
    entries.push(mark("Mark as Good", menuIcons.good, "good", "G"), mark("Mark as Bad", menuIcons.bad, "bad", "B"), mark("Skip This Commit", menuIcons.skip, "skip", "S"), { kind: "sep" });
  }
  entries.push(item(bisect.found ? "Finish Bisect…" : "Stop Bisect…", menuIcons.bisect, () => run(bisectResetRequest(ctx, bisect))), { kind: "sep" });
  entries.push(item("New Branch from Here…", menuIcons.branch, () => run(newBranchRequest(ctx, undefined, row))), item("New Tag Here…", menuIcons.tag, () => run(newTagRequest(ctx, row))));
  const name = row.labels.find((l) => l.kind === "local")?.name ?? row.id.slice(0, 7);
  const against = [bisect.startIsBranch ? bisect.start : null, history.trunk].filter((b, i, all): b is string => !!b && b !== name && all.indexOf(b) === i);
  entries.push({ kind: "sep" }, browseItem(row, item, app));
  if (against.length) entries.push({ kind: "sub", label: "Compare", icon: menuIcons.compare, entries: against.map((b) => ({ label: `With ${b}`, run: () => app.openCompare(b, name) })) });
  entries.push({
    kind: "sub",
    label: "Copy",
    icon: menuIcons.copy,
    entries: [
      { label: "SHA", hint: row.id.slice(0, 7) + "…", run: () => copyText(row.id, `Copied ${row.id.slice(0, 7)}.`) },
      { label: "Short SHA", hint: row.id.slice(0, 7), run: () => copyText(row.id.slice(0, 7), `Copied ${row.id.slice(0, 7)}.`) },
      { label: "Message", run: () => copyMessage(row.id) },
    ],
  });
  return entries;
}

/** Browse Files at This Commit, named by the branch or tag that ends there. */
function browseItem(row: HistoryRow, item: (label: string, icon: string, act: () => void) => MenuEntry, app: CommitMenuApp): MenuEntry {
  const label = row.labels.find((l) => l.kind === "local" || l.kind === "tag")?.name ?? null;
  return item("Browse Files at This Commit", menuIcons.folder, () => app.openBrowse(row.id, label));
}
