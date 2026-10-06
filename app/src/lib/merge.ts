// The merge sheet (bring a branch into the checked-out one: merge commit, squash, rebase or
// fast-forward, with a picture of the result and a dry run for conflicts), and the actions that
// finish or abort a merge, rebase, cherry-pick or revert that stopped on conflicts.

import { api } from "./api";
import type { Part, PreviewRow, Recovery, Request } from "./confirm.svelte";
import type { BranchContext } from "./branches";
import type { CommitBrief, Failure, MergeMethod, MergePreview, Operation } from "./types";
import { shortId } from "./format";

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
const chip = (ctx: BranchContext, name: string): Part => ({
  branch: name,
  color: ctx.colorOf(name),
});

/** Commits listed per side before "and N more". */
const SHOWN = 4;

const METHODS: { method: MergeMethod; label: string }[] = [
  { method: "merge", label: "Merge commit" },
  { method: "squash", label: "Squash" },
  { method: "rebase", label: "Rebase" },
  { method: "fastForward", label: "Fast-forward" },
];

/** "a.txt, b.txt and 3 more" */
function listFiles(paths: string[]): Part[] {
  const shown = paths.slice(0, 2);
  const parts: Part[] = [];
  shown.forEach((p, i) => {
    if (i) parts.push(i === shown.length - 1 && paths.length === shown.length ? " and " : ", ");
    parts.push({ code: p });
  });
  if (paths.length > shown.length) parts.push(` and ${paths.length - shown.length} more`);
  return parts;
}

/** Branches offered as chips: the trunk first, then the most recent, always with `picked`. */
function candidates(ctx: BranchContext, picked: string): string[] {
  const { history } = ctx;
  const head = history.head.branch;
  const time = new Map(history.rows.map((r) => [r.id, r.time]));
  const locals = history.refs
    .filter((r) => r.kind === "local" && r.name !== head)
    .sort((a, b) => (b.name === history.trunk ? 1 : 0) - (a.name === history.trunk ? 1 : 0) || (time.get(b.target) ?? 0) - (time.get(a.target) ?? 0))
    .map((r) => r.name);
  const list = locals.slice(0, 3);
  if (!list.includes(picked)) list.push(picked);
  return list;
}

/** The default message of a merge commit, the way git writes it (History reads the branch back from it). */
function mergeMessage(ctx: BranchContext, branch: string): string {
  const remote = ctx.history.refs.some((r) => r.kind === "remote" && r.name === branch);
  const into = ctx.history.head.branch ? ` into ${ctx.history.head.branch}` : "";
  return remote ? `Merge remote-tracking branch '${branch}'${into}` : `Merge branch '${branch}'${into}`;
}

function squashMessage(preview: MergePreview): string {
  return preview.incomingCount === 1 && preview.incoming[0] ? preview.incoming[0].summary : `Squash ${preview.branch} (${preview.incomingCount} commits)`;
}

/** Rows of commits on one line, with "and N more" when there are more than fit. */
function commitRows(commits: CommitBrief[], count: number, lane: 0 | 1, color: number, made: boolean, muted = false): PreviewRow[] {
  const rows: PreviewRow[] = commits.slice(0, SHOWN).map((c) => ({
    lane,
    color,
    summary: c.summary,
    sha: made ? null : shortId(c.id),
    node: made ? "new" : "commit",
    muted,
    links: [],
  }));
  if (count > rows.length)
    rows.push({
      lane,
      color,
      summary: `and ${count - rows.length} more`,
      sha: null,
      node: "more",
      muted: true,
      links: [],
    });
  return rows;
}

/** Chain `rows` down to each other and the last one to `next`. */
function chain(all: PreviewRow[], from: number, length: number, next: number) {
  for (let i = from; i < from + length; i++) all[i].links.push(i + 1 < from + length ? i + 1 : next);
}

/** The picture of the result for each method. */
function previewRows(ctx: BranchContext, p: MergePreview, method: MergeMethod, message: string): PreviewRow[] {
  const here = ctx.history.head.branch ?? "HEAD";
  const mine = ctx.colorOf(here);
  const theirs = ctx.colorOf(p.branch);
  const base: PreviewRow | null = p.base
    ? {
        lane: 0,
        color: mine,
        summary: p.base.summary,
        sha: shortId(p.base.id),
        node: "base",
        muted: true,
        links: [],
      }
    : null;
  const incoming = (lane: 0 | 1, muted = false) => commitRows(p.incoming, p.incomingCount, lane, theirs, false, muted);
  const ours = (made: boolean, color = mine) => commitRows(p.ours, p.oursCount, 0, color, made);
  const rows: PreviewRow[] = [];
  const title = message.split("\n")[0] || "New commit";
  if (method === "merge") {
    rows.push({
      lane: 0,
      color: mine,
      other: theirs,
      summary: title,
      sha: null,
      node: "merge",
      links: [],
    });
    const a = incoming(1);
    const b = ours(false);
    rows.push(...a, ...b);
    if (base) rows.push(base);
    const baseAt = rows.length - 1;
    rows[0].links.push(1, b.length ? 1 + a.length : baseAt);
    chain(rows, 1, a.length, baseAt);
    chain(rows, 1 + a.length, b.length, baseAt);
  } else if (method === "squash") {
    rows.push({
      lane: 0,
      color: mine,
      summary: title,
      sha: null,
      node: "new",
      links: [],
    });
    const a = incoming(1, true);
    const b = ours(false);
    rows.push(...a, ...b);
    if (base) rows.push(base);
    const baseAt = rows.length - 1;
    rows[0].links.push(b.length ? 1 + a.length : baseAt);
    chain(rows, 1, a.length, baseAt);
    chain(rows, 1 + a.length, b.length, baseAt);
  } else {
    // Rebase replays your commits on top of the other branch; fast-forward just moves up to it.
    const b = method === "rebase" ? ours(true) : [];
    const a = incoming(0);
    rows.push(...b, ...a);
    if (base) rows.push(base);
    chain(rows, 0, rows.length - (base ? 1 : 0), rows.length - 1);
  }
  return rows;
}

function verdict(ctx: BranchContext, p: MergePreview, method: MergeMethod): { tone: "ok" | "warn" | "info"; parts: Part[] } {
  const here = ctx.history.head.branch ?? "HEAD";
  if (method === "fastForward")
    return {
      tone: "ok",
      parts: [{ quote: "Fast-forward." }, ` ${here} moves up to ${p.branch}. No new commit, nothing to combine.`],
    };
  if (method === "rebase" && p.oursCount === 0)
    return {
      tone: "ok",
      parts: [{ quote: "Nothing to replay." }, ` ${here} has no commits of its own, so it just moves up to ${p.branch}.`],
    };
  const done = method === "rebase" ? "Rebases" : method === "squash" ? "Squashes" : "Merges";
  if (p.conflicts === null)
    return {
      tone: "info",
      parts: ["This version of git can’t check for conflicts ahead of time. If there are any, you resolve them in Conflicts."],
    };
  if (!p.conflicts.length) {
    const touched = p.touchedHere ? `${p.touchedHere} of them also changed on ${here}; git combines the changes.` : `none of them touched on ${here}.`;
    return {
      tone: "ok",
      parts: [{ quote: `${done} cleanly.` }, ` ${plural(p.files, "file")} change, ${touched}`],
    };
  }
  const finish = method === "rebase" ? "You resolve them commit by commit, then the rebase goes on." : "You resolve them in Conflicts, then finish the merge.";
  return {
    tone: "warn",
    parts: [{ quote: `${plural(p.conflicts.length, "conflict")} in ` }, ...listFiles(p.conflicts), `. Both branches change the same lines. ${finish}`],
  };
}

/** The merge sheet for bringing `branch` into the checked-out branch. */
export async function mergeRequest(ctx: BranchContext, branch: string, method: MergeMethod = "merge", message?: string): Promise<Request> {
  const preview = await api.mergePreview(branch);
  return mergeSheet(ctx, preview, method, message, new Map([[branch, preview]]));
}

function mergeSheet(ctx: BranchContext, p: MergePreview, method: MergeMethod, typed: string | undefined, cache: Map<string, MergePreview>): Request {
  const here = ctx.history.head.branch ?? "HEAD";
  const branch = p.branch;
  const diverged = p.oursCount > 0;
  // Fast-forward is only possible while the checked-out branch has nothing of its own.
  if (method === "fastForward" && diverged) method = "merge";
  const message = typed ?? (method === "squash" ? squashMessage(p) : mergeMessage(ctx, branch));
  const again = (next: Partial<{ branch: string; method: MergeMethod; message: string }>) => {
    const name = next.branch ?? branch;
    const keep = next.message ?? (next.branch || next.method ? undefined : message);
    const cached = cache.get(name);
    if (cached) return mergeSheet(ctx, cached, next.method ?? method, keep, cache);
    return api.mergePreview(name).then((fresh) => {
      cache.set(name, fresh);
      return mergeSheet(ctx, fresh, next.method ?? method, keep, cache);
    });
  };
  const conflicts = p.conflicts?.length ?? 0;
  const upToDate = p.incomingCount === 0;
  const verb = method === "rebase" ? "Rebase" : method === "squash" ? "Squash and Merge" : method === "fastForward" ? "Fast-Forward" : "Merge";
  const fields: Request["fields"] = [
    {
      label: "Bring in",
      chips: candidates(ctx, branch).map((name) => ({
        label: name,
        on: name === branch,
        pick: () => again({ branch: name }),
      })),
    },
    {
      label: "How",
      chips: METHODS.map((m) => ({
        label: m.label,
        on: m.method === method,
        off: m.method === "fastForward" && diverged ? `${here} has commits of its own, so it can’t just move up to ${branch}.` : undefined,
        pick: () => again({ method: m.method }),
      })),
      note:
        method === "merge"
          ? `A merge commit joins both branches; History keeps ${branch}’s commits as they are.`
          : method === "squash"
            ? `All of ${branch}’s changes become one new commit on ${here}. ${branch} stays as it is.`
            : method === "rebase"
              ? `${here}’s ${plural(p.oursCount, "commit")} are replayed on top of ${branch} as new commits. ${here} gets new SHAs, so push it with force if it was pushed before.`
              : undefined,
    },
  ];
  if (method === "merge" || method === "squash") {
    fields.push({
      label: "Message",
      text: {
        value: message,
        placeholder: "Commit message",
        edit: (value) => mergeSheet(ctx, p, method, value, cache),
      },
    });
  }
  return {
    title: method === "rebase" ? `Rebase ${here} onto ${branch}` : `Merge into ${here}`,
    body: ["from ", chip(ctx, branch), ` · ${upToDate ? "nothing new" : plural(p.incomingCount, "new commit")}`],
    icon: method === "rebase" ? "rebase" : "merge",
    button: conflicts && !upToDate ? `${verb} and Resolve…` : verb,
    fields,
    preview: upToDate
      ? {
          rows: [],
          verdict: {
            tone: "info",
            parts: [{ quote: "Already up to date." }, ` ${here} has every commit of ${branch}.`],
          },
        }
      : {
          rows: previewRows(ctx, p, method, message),
          verdict: verdict(ctx, p, method),
        },
    invalid: upToDate
      ? `${here} already has everything on ${branch}.`
      : (method === "merge" || method === "squash") && !message.trim()
        ? "Type a commit message."
        : null,
    note: "Nothing is pushed. Until you push, Abort or a reset puts everything back.",
    status: method === "rebase" ? `Rebasing ${here} onto ${branch}…` : `Merging ${branch} into ${here}…`,
    done: method === "rebase" ? `Rebased ${here} onto ${branch}.` : `Merged ${branch} into ${here}.`,
    resolveConflicts: conflicts > 0,
    recover: (failure) => stoppedOnConflicts(failure, method === "rebase" ? "rebase" : "merge"),
    // git writes the default merge message itself with --no-edit, without quotes to escape.
    action: {
      kind: "merge",
      branch,
      method,
      message: method === "squash" || (method === "merge" && message.trim() !== mergeMessage(ctx, branch)) ? message : null,
    },
  };
}

type Stopped = "merge" | "rebase" | "pull" | "cherry-pick" | "revert" | "stash";
const NOUNS = { merge: "Merge", rebase: "Rebase", pull: "Pull", "cherry-pick": "Cherry-Pick", revert: "Revert", stash: "Apply" };

/** What stopped, for the operation in progress. */
function stoppedBy(op: Operation): Stopped {
  return op.kind === "rebase" ? "rebase" : op.kind === "cherryPick" ? "cherry-pick" : op.kind === "revert" ? "revert" : op.kind === "stashApply" ? "stash" : "merge";
}

/** After an action stopped on conflicts: resolve them, or give it up. */
export function stoppedOnConflicts(failure: Failure, what: Stopped): Recovery | null {
  if (failure.kind !== "conflict") return null;
  const undo = what === "pull" ? "Undo Pull" : what === "stash" ? "Undo Apply" : `Abort ${NOUNS[what]}`;
  return {
    title: what === "stash" ? "Applying the stash stopped on conflicts" : `The ${what} stopped on conflicts`,
    body: [
      what === "stash"
        ? "The stash and your branch change the same lines, so git needs you to choose what goes into each file. Resolve the conflicts, then "
        : "Both sides change the same lines, so git needs you to choose what goes into each file. Resolve the conflicts, then ",
      what === "merge" ? "commit the merge." : what === "pull" ? "continue the rebase." : what === "stash" ? "finish the apply. The stash stays until then." : `continue the ${what}.`,
    ],
    icon: "warn",
    tone: "warn",
    button: {
      label: undo,
      action: { kind: "abort" },
      status: `${undo.replace("Undo", "Undoing").replace("Abort", "Aborting")}…`,
      done: "Everything is back where it was.",
    },
    close: "Resolve Conflicts",
    note: "Nothing is lost while you decide.",
  };
}

/** A cherry-pick whose changes are already on the branch stops with nothing to commit. */
export function nothingToPick(ctx: BranchContext, failure: Failure): Recovery | null {
  if (!failure.output.includes("is now empty")) return null;
  return {
    title: "Nothing left to pick",
    body: ["The changes of this commit are already on ", chip(ctx, ctx.history.head.branch ?? "HEAD"), ", so cherry-picking it would make an empty commit. Skip it to finish."],
    icon: "warn",
    tone: "warn",
    button: { label: "Skip It", action: { kind: "skip" }, status: "Skipping the commit…", done: "Nothing changed." },
    close: "Later",
  };
}

/** Words for the operation in progress. */
export function describe(op: Operation): {
  title: string;
  noun: string;
  verb: string;
} {
  const branch = op.branch ?? "HEAD";
  const incoming = op.incoming ?? "a commit";
  switch (op.kind) {
    case "merge":
      return {
        title: `Merging ${incoming} into ${branch}`,
        noun: "Merge",
        verb: "merge",
      };
    case "squash":
      return {
        title: op.incoming ? `Squashing ${op.incoming} into ${branch}` : `Squash merge into ${branch}`,
        noun: "Squash",
        verb: "squash merge",
      };
    case "rebase":
      return {
        title: `Rebasing ${branch} onto ${incoming}`,
        noun: "Rebase",
        verb: "rebase",
      };
    case "cherryPick":
      return {
        title: `Picking ${incoming} onto ${branch}`,
        noun: "Cherry-pick",
        verb: "cherry-pick",
      };
    case "revert":
      return {
        title: `Reverting ${incoming} on ${branch}`,
        noun: "Revert",
        verb: "revert",
      };
    case "stashApply":
      return {
        title: `Applying ${op.incoming ?? "a stash"} to ${branch}`,
        noun: "Apply",
        verb: "stash apply",
      };
  }
}

export function abortRequest(ctx: BranchContext, op: Operation): Request {
  const { noun, verb } = describe(op);
  const branch = op.branch ?? "HEAD";
  if (op.kind === "stashApply") {
    return {
      title: "Undo applying the stash?",
      body: [
        "Puts the files the stash changed back to how ",
        chip(ctx, branch),
        " has them and removes the untracked files it brought back. Your other changes stay. ",
        { code: op.incoming ?? "The stash" },
        " stays in the list.",
      ],
      icon: "discard",
      danger: true,
      button: "Undo Apply",
      status: "Undoing the apply…",
      done: "Undone. The stash is still in the list.",
      action: { kind: "abort" },
    };
  }
  return {
    title: `Abort the ${verb}?`,
    body: [chip(ctx, branch), ` goes back to how it was before the ${verb}, and so do its files. Choices you made in conflicts are thrown away.`],
    icon: "discard",
    danger: true,
    button: `Abort ${noun}`,
    status: `Aborting the ${verb}…`,
    done: `Aborted. ${branch} is back where it was.`,
    action: { kind: "abort" },
  };
}

export function skipRequest(ctx: BranchContext, op: Operation): Request {
  const commit = op.commit;
  return {
    title: "Skip this commit?",
    body: [
      "Leaves ",
      ...(commit ? [{ code: shortId(commit.id) } as Part, " ", { quote: commit.summary } as Part] : ["the commit"]),
      " out of ",
      chip(ctx, op.branch ?? "HEAD"),
      ` and goes on with the next one. Its changes are dropped from the ${op.kind === "rebase" ? "rebased branch" : "branch"}.`,
    ],
    icon: "warn",
    tone: "warn",
    button: "Skip Commit",
    status: "Skipping the commit…",
    done: "Skipped.",
    recover: (failure) => stoppedOnConflicts(failure, stoppedBy(op)),
    resolveConflicts: true,
    action: { kind: "skip" },
  };
}

/** Commit the merge or go on with the rebase, once every conflict is resolved. */
export function continueRequest(ctx: BranchContext, op: Operation, message?: string): Request {
  const { noun, verb } = describe(op);
  const branch = op.branch ?? "HEAD";
  const commits = op.kind === "merge" || op.kind === "squash";
  const text = message ?? op.message ?? "";
  const step = op.step ? ` (commit ${op.step[0]} of ${op.step[1]})` : "";
  if (op.kind === "stashApply") {
    return {
      title: "Finish applying the stash?",
      body: [
        "Keeps your resolved files as uncommitted changes on ",
        chip(ctx, branch),
        ". After a pop that hit a conflict git keeps the stash in the list, so Oxbow deletes it now that it is applied.",
      ],
      icon: "stash",
      button: "Finish",
      status: "Finishing…",
      done: "Applied the stash. The changes are uncommitted.",
      action: { kind: "continue", message: null },
    };
  }
  return {
    title: commits ? `Commit the ${verb}?` : `Continue the ${verb}?`,
    body: commits
      ? ["Makes the commit that finishes the ", verb, " on ", chip(ctx, branch), "."]
      : op.kind === "rebase"
        ? ["Commits the resolved files", step, " and goes on replaying the rest onto ", chip(ctx, op.incoming ?? branch), "."]
        : [`Makes the ${verb} commit on `, chip(ctx, branch), ` with the resolved files${op.commit ? `, keeping the message of ${shortId(op.commit.id)}` : ""}.`],
    icon: commits ? "commit" : op.kind === "rebase" ? "rebase" : op.kind === "cherryPick" ? "cherry" : op.kind === "revert" ? "revert" : "merge",
    button: commits ? `Commit ${noun}` : `Continue ${noun}`,
    fields: commits
      ? [
          {
            label: "Message",
            text: {
              value: text,
              placeholder: "Commit message",
              edit: (value) => continueRequest(ctx, op, value),
            },
          },
        ]
      : undefined,
    invalid: commits && !text.trim() ? "Type a commit message." : null,
    status: commits ? `Committing the ${verb}…` : `Continuing the ${verb}…`,
    done: commits || op.kind !== "rebase" ? `Committed the ${verb} on ${branch}.` : `Done.`,
    resolveConflicts: !commits,
    recover: (failure) => nothingToPick(ctx, failure) ?? stoppedOnConflicts(failure, stoppedBy(op)),
    // An untouched message stays git's, so the command shows plain --no-edit.
    action: {
      kind: "continue",
      message: commits && text.trim() !== (op.message ?? "").trim() ? text : null,
    },
  };
}
