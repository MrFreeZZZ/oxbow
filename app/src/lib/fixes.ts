// Fix a Mistake: the list of typical mistakes, and the sheet of each fix. Every fix is planned
// from how the repository looks right now (`fixState`): the core checks that state again before
// anything runs, and the sheet shows the plan's steps, each with its commands.

import { api } from "./api";
import { checkFailed } from "./commitChecks";
import type { Icon, Part, Recovery, Request, StepLine } from "./confirm.svelte";
import { nameProblem, onNoBranch, type BranchContext } from "./branches";
import type { Action, Failure, Fix, FixState, FixStep, HistoryRow, ResetMode } from "./types";
import { relativeTime, shortId } from "./format";

export type FixId = "move" | "forgot" | "undo" | "revert" | "detached";

/** A row of the list of fixes; `off` says why it can't be used now. */
export interface FixChoice {
  id: FixId;
  title: string;
  sub: string;
  icon: Icon;
  off: string | null;
}

export interface FixContext {
  ctx: BranchContext;
  state: FixState;
  /** A commit the list was opened on, from its menu: the one Undo a Pushed Commit starts with. */
  commit: string | null;
  /** Back to the list of fixes. */
  back: () => void;
  /** The same fix planned again from the repository as it is now. */
  reopen: (id: FixId) => Promise<Request>;
}

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
const chip = (ctx: BranchContext, name: string): Part => ({ branch: name, color: ctx.colorOf(name) });
const here = (ctx: BranchContext): Part => (ctx.history.head.branch ? chip(ctx, ctx.history.head.branch) : { code: "HEAD" });
const hereName = (ctx: BranchContext) => ctx.history.head.branch ?? "HEAD";

/** The checked-out branch's commits, newest first, following first parents, as far as History
 *  has loaded them. */
function chain(ctx: BranchContext): HistoryRow[] {
  const byId = new Map(ctx.history.rows.map((r) => [r.id, r]));
  const out: HistoryRow[] = [];
  let next = ctx.history.head.commit;
  while (next && out.length < 50) {
    const row = byId.get(next);
    if (!row) break;
    out.push(row);
    next = row.parents[0] ?? null;
  }
  return out;
}

/** Whether a remote-tracking branch has the commit. Without any, nothing is pushed. */
function onRemote(ctx: BranchContext, row: HistoryRow): boolean {
  return !row.unpushed && ctx.history.refs.some((r) => r.kind === "remote");
}

const remoteName = (ctx: BranchContext) => ctx.history.tracking?.remote ?? ctx.history.defaultRemote ?? "the remote";

/** `a41f9c2 “Add CSV export button” and 7d03e18 “Export: quote commas”`, two at most. */
function commitList(rows: HistoryRow[]): Part[] {
  const parts: Part[] = [];
  const shown = rows.slice(0, 2);
  shown.forEach((r, i) => {
    if (i) parts.push(rows.length === 2 ? " and " : ", ");
    parts.push({ code: shortId(r.id) }, " ", { quote: r.summary });
  });
  if (rows.length > 2) parts.push(` and ${plural(rows.length - 2, "more commit")}`);
  return parts;
}

/** "a.ts, b.ts and 2 more", as code chips. */
function pathList(paths: string[]): Part[] {
  const parts: Part[] = [];
  const shown = paths.slice(0, 3);
  shown.forEach((p, i) => {
    if (i) parts.push(i === shown.length - 1 && paths.length === shown.length ? " and " : ", ");
    parts.push({ code: p });
  });
  if (paths.length > shown.length) parts.push(` and ${paths.length - shown.length} more`);
  return parts;
}

/** The list of fixes, with why each can't be used now. `busy` is why none can, e.g. a merge
 *  in progress. */
export function fixChoices(fc: FixContext, busy: string | null): FixChoice[] {
  const { ctx, state } = fc;
  const h = ctx.history;
  const branch = h.head.branch;
  const commits = chain(ctx);
  const head = commits[0];
  const unpushed = commits.filter((r) => !onRemote(ctx, r)).length;
  const pushed = commits.filter((r) => onRemote(ctx, r));
  const noCommit = !head ? "There are no commits yet." : null;
  const choices: FixChoice[] = [
    {
      id: "move",
      title: "I committed to the wrong branch",
      sub: branch && unpushed && unpushed < commits.length
        ? `Move the last ${unpushed === 1 ? "commit" : `${unpushed} commits`} from ${branch} to another branch, new or existing.`
        : `Move the last commits from ${branch ?? "HEAD"} to another branch, new or existing.`,
      icon: "branch",
      off: noCommit ?? (!branch ? "HEAD isn’t on a branch. Use Save Work from a Detached HEAD instead." : commits.length < 2 ? `${branch} has only its first commit, so there is nothing to move it off.` : null),
    },
    {
      id: "forgot",
      title: "I forgot a file in the last commit",
      sub: head ? `Add some of your ${plural(state.changed.length, "changed file")} to “${head.summary}”.` : "",
      icon: "stage",
      off: noCommit ?? (!state.changed.length ? "There are no changed files to add." : null),
    },
    {
      id: "undo",
      title: "Undo the last commit, keep its changes",
      sub: head ? `“${head.summary}” comes off ${hereName(ctx)}; its changes go back to staged.` : "",
      icon: "undo",
      off: noCommit ?? (head && !head.parents.length ? "This is the first commit; there is nothing before it to go back to." : null),
    },
    {
      id: "revert",
      title: "Undo a commit that is already pushed",
      sub: "Add a commit that reverses it, then push, so nobody’s history breaks.",
      icon: "revert",
      off: noCommit ?? (!branch ? "HEAD isn’t on a branch, so there is nothing to push the revert to. Use Save Work from a Detached HEAD first." : !pushed.length ? `Nothing on ${hereName(ctx)} is pushed yet. Use Undo the Last Commit instead.` : null),
    },
    {
      id: "detached",
      title: "Save work from a detached HEAD",
      sub: !branch
        ? onNoBranch(h).length
          ? `${plural(onNoBranch(h).length, "commit")} made without a branch: keep them on a new branch.`
          : "Make a branch where HEAD is, so new commits belong to it."
        : state.lost.length
          ? `${plural(state.lost.length, "commit")} no branch has any more: bring ${state.lost.length === 1 ? "it" : "them"} back on a new branch.`
          : "",
      icon: "checkout",
      off: branch && !state.lost.length ? `You are on ${branch}, and no commits were left without a branch lately.` : null,
    },
  ];
  return choices.map((c) => (busy ? { ...c, off: busy } : c));
}

/** The sheet of one fix, with its first choices. */
export function fixRequest(fc: FixContext, id: FixId): Request {
  const { ctx } = fc;
  switch (id) {
    case "move": {
      const commits = chain(ctx);
      const unpushed = commits.filter((r) => !onRemote(ctx, r)).length;
      const count = Math.max(1, Math.min(unpushed, 5, commits.length - 1));
      return moveSheet(fc, { count, create: true, to: "", name: "" });
    }
    case "forgot": {
      // Files staged already are the ones made ready for it; otherwise a single changed file.
      const changed = fc.state.changed;
      const staged = changed.filter((f) => f.staged && !f.unstaged).map((f) => f.path);
      return forgotSheet(fc, new Set(staged.length ? staged : changed.length === 1 ? [changed[0].path] : []));
    }
    case "undo":
      return undoSheet(fc, "soft");
    case "revert": {
      const candidates = revertCandidates(fc);
      const t = ctx.history.tracking;
      return revertSheet(fc, candidates[0]?.id ?? "", !!(ctx.history.head.branch && t && !t.gone && !t.behind));
    }
    case "detached":
      return detachedSheet(fc, { name: "", commit: ctx.history.head.branch ? (fc.state.lost[0]?.id ?? null) : null, switchTo: false });
  }
}

function fixAction(fc: FixContext, fix: Fix): Action {
  return { kind: "fix", fix, expect: fc.state.expect };
}

/** Words of the plan's steps. `subject` names what a revert reverses, e.g. a commit's sha. */
function stepWords(ctx: BranchContext, opts: { them?: string; subject?: Part[]; summary?: string; createSub?: string } = {}) {
  const them = opts.them ?? "them";
  return (steps: FixStep[]): StepLine[] =>
    steps.map((step): StepLine => {
      switch (step.kind) {
        case "stash":
          return {
            parts: ["Put your ", plural(step.files, "uncommitted file"), " aside"],
            sub: "A later step needs them out of the way. They come back in the last step, staged ones staged.",
            head: "Put your uncommitted files aside",
          };
        case "createBranch":
          if (step.at) {
            return {
              parts: ["Create ", chip(ctx, step.name), " at ", { code: shortId(step.at) }],
              sub: "A branch keeps the commit and everything before it.",
              head: `Create ${step.name} at ${shortId(step.at)}`,
            };
          }
          if (!step.name) return { parts: ["Create the new branch here and switch to it"], sub: opts.createSub, head: "Create the new branch and switch to it" };
          return { parts: ["Create ", chip(ctx, step.name), " here and switch to it"], sub: opts.createSub, head: `Create ${step.name} and switch to it` };
        case "moveBranch":
          return {
            parts: ["Move ", chip(ctx, step.branch), " back to ", { code: shortId(step.to) }, `, where it was before ${them}`],
            head: `Move ${step.branch} back to ${shortId(step.to)}`,
          };
        case "switch":
          return { parts: ["Switch to ", chip(ctx, step.branch)], head: `Switch to ${step.branch}` };
        case "copy":
          return { parts: [step.count === 1 ? "Copy the commit onto it" : `Copy the ${step.count} commits onto it`], head: step.count === 1 ? "Copy the commit" : "Copy the commits" };
        case "revert": {
          const what: Part[] = opts.subject ?? [step.count === 1 ? "it" : them];
          const parts: Part[] = [step.count === 1 ? "Add a commit that reverses " : `Add ${step.count} commits that reverse `, ...what];
          if (step.branch) parts.push(" on ", chip(ctx, step.branch));
          return { parts, head: step.branch ? `Revert ${step.count === 1 ? "it" : them} on ${step.branch}` : "Revert it" };
        }
        case "stage":
          return { parts: ["Stage ", ...pathList(step.paths)], head: step.paths.length === 1 ? `Stage ${step.paths[0]}` : "Stage the files" };
        case "amend":
          return { parts: ["Replace the last commit with one that has them too, same message"], head: "Add them to the last commit" };
        case "reset":
          return {
            parts: ["Take ", ...(opts.summary ? [{ quote: opts.summary } as Part] : ["the last commit"]), " off ", here(ctx), step.mode === "soft" ? "; its changes stay staged" : "; its changes stay in your files"],
            head: "Undo the last commit",
          };
        case "push":
          return { parts: ["Push ", chip(ctx, step.branch), ` to ${step.remote}`], head: `Push ${step.branch}` };
        case "pop":
          return { parts: ["Bring your uncommitted files back"], head: "Bring your files back" };
      }
    });
}

/** The way out when a fix didn't run or stopped part way. */
function fixRecovery(fc: FixContext, id: FixId, failure: Failure): Recovery | null {
  if (failure.kind === "changed") {
    return {
      title: "The repository changed",
      body: [failure.output.replace(/^\w/, (c) => c.toUpperCase())],
      icon: "warn",
      tone: "warn",
      alt: { label: "See New Plan", request: () => fc.reopen(id) },
      close: "Close",
      note: "Nothing ran.",
    };
  }
  if (failure.kind === "conflict") {
    return {
      title: "A step stopped on conflicts",
      body: [
        "The steps before it ran; the rest wait. Pick what goes into each file in the banner above History, then Continue: it runs the steps left too. Skip leaves the commit out of the copy, and the branch it came from keeps all its commits. Abort takes back this step and goes back to the branch you started on. To go back to how it all was, use Undo in the Operation Log.",
      ],
      icon: "warn",
      tone: "warn",
      close: "OK",
      note: "If your uncommitted files were put aside, they come back after Continue or Abort.",
    };
  }
  if (failure.kind === "stashKept") {
    return {
      title: "Your files wait in the stash",
      body: [
        "The fix ran, but your uncommitted files could not come back as they were: their staged changes no longer fit on top of the new commits. Nothing is lost: the stash keeps them, staged and unstaged parts apart. Apply it in Stashes to bring them back unstaged.",
      ],
      icon: "warn",
      tone: "warn",
      close: "OK",
    };
  }
  if (failure.kind === "localChanges") {
    return {
      title: "Your uncommitted changes are in the way",
      body: ["Git stopped before changing those files. Commit or stash them, then try the fix again."],
      icon: "warn",
      tone: "warn",
      close: "OK",
    };
  }
  // The push is the last step of a revert: the revert commit is made by then.
  if (id === "revert" && (failure.kind === "rejected" || failure.kind === "staleLease" || failure.kind === "auth" || failure.kind === "network")) {
    return {
      title: "The revert is made, the push didn’t go through",
      body: ["The new commit is on ", here(fc.ctx), ". Push it once the reason below is sorted out; Pull first if the remote has new commits."],
      icon: "warn",
      tone: "warn",
      close: "OK",
    };
  }
  if (failure.kind === "cancelled") return null;
  // Any other error can come part way: the steps before it ran, and the Operation Log has them.
  return {
    title: "A step stopped with an error",
    body: ["The steps before it ran; the rest did not. Undo in the Operation Log takes back what ran."],
    icon: "warn",
    tone: "err",
    close: "OK",
    note: "If your uncommitted files were put aside, they wait in the stash.",
  };
}

interface Move {
  count: number;
  create: boolean;
  /** The existing branch picked, when not `create`. */
  to: string;
  name: string;
}

const PREFIXES = ["feature/", "fix/", "chore/"];

function moveSheet(fc: FixContext, s: Move): Request {
  const { ctx } = fc;
  const h = ctx.history;
  const again = (change: Partial<Move>) => moveSheet(fc, { ...s, ...change });
  const branch = h.head.branch ?? "HEAD";
  const all = chain(ctx);
  const commits = all.slice(0, s.count);
  const base = all[s.count];
  const pushedRows = commits.filter((r) => onRemote(ctx, r));
  const pushed = pushedRows.length > 0;
  const remote = remoteName(ctx);
  const target = s.create ? s.name.trim() : s.to;
  const problem = s.create ? nameProblem(ctx, target) : null;
  const copies = (n: number) => all.slice(0, n).some((r) => onRemote(ctx, r)) || !s.create;
  const hasMerge = (n: number) => all.slice(0, n).some((r) => r.parents.length > 1);
  const mergeBlock = hasMerge(s.count) && copies(s.count) ? "A merge commit is among them, and merges can’t be copied one by one. Use Edit Stack instead." : null;
  const them = s.count === 1 ? "it" : "them";

  // Branches the commits could go to: the latest ones first.
  const time = new Map(h.rows.map((r) => [r.id, r.time]));
  const others = h.refs
    .filter((r) => r.kind === "local" && r.name !== h.head.branch)
    .sort((a, b) => (time.get(b.target) ?? 0) - (time.get(a.target) ?? 0))
    .map((r) => r.name);
  const shown = others.slice(0, 4);
  if (s.to && !shown.includes(s.to)) shown[shown.length - 1] = s.to;

  const targetPart: Part = target && !problem ? chip(ctx, target) : "the branch you pick";
  const body: Part[] = pushed
    ? ["Copies ", ...commitList(commits), " onto ", targetPart, ` and takes ${s.count === 1 ? "its" : "their"} changes back out of `, chip(ctx, branch), ` with ${s.count === 1 ? "a revert commit" : "revert commits"}.`]
    : ["Takes ", ...commitList(commits), " off ", chip(ctx, branch), " and puts ", them, " on ", targetPart, "."];
  if (ctx.uncommitted) body.push(` Your ${plural(ctx.uncommitted, "uncommitted file")} stay as they are.`);

  const max = Math.min(5, all.length - 1);
  const countNote = !pushed
    ? s.count === 1
      ? "It is not pushed yet."
      : s.count === 2
        ? "Both are not pushed yet."
        : "None of them is pushed yet."
    : pushedRows.length === commits.length
      ? `Already on ${remote}: they are reverted on ${branch}, not rewritten.`
      : `${pushedRows.length} of them ${pushedRows.length === 1 ? "is" : "are"} already on ${remote}.`;
  const fields: Request["fields"] = [
    {
      label: "Commits",
      chips: Array.from({ length: max }, (_, i) => i + 1).map((n) => ({
        label: `Last ${n}`,
        on: n === s.count,
        off: hasMerge(n) && copies(n) ? "A merge commit is among them, and merges can’t be copied one by one." : undefined,
        pick: () => again({ count: n }),
      })),
      note: countNote,
    },
    {
      label: "To",
      chips: [{ label: "New Branch", on: s.create, pick: () => again({ create: true }) }, ...shown.map((name) => ({ label: name, on: !s.create && s.to === name, pick: () => again({ create: false, to: name }) }))],
    },
  ];
  if (s.create) {
    const withPrefix = (prefix: string) => {
      const bare = PREFIXES.reduce((n, p) => (n.startsWith(p) ? n.slice(p.length) : n), s.name);
      return s.name.startsWith(prefix) ? bare : prefix + bare;
    };
    fields.push({
      label: "Name",
      text: { value: s.name, placeholder: "feature/csv-export", focus: true, edit: (value) => again({ name: value }) },
      chips: PREFIXES.map((p) => ({ label: p, on: s.name.startsWith(p), pick: () => again({ name: withPrefix(p) }) })),
      error: target && problem ? problem : undefined,
    });
  }

  const n = plural(s.count, "commit");
  return {
    // The same title while the name is typed: a new title puts the cursor back and selects the name.
    title: `Move ${n} to ${s.create ? "a new branch" : target}?`,
    body,
    icon: "branch",
    tone: pushed ? "warn" : undefined,
    button: s.count === 1 ? "Move Commit" : "Move Commits",
    fields,
    warning: pushed
      ? {
          title: pushedRows.length === commits.length ? `${s.count === 1 ? "It is" : "They are"} already on ${remote}` : `${pushedRows.length} of them ${pushedRows.length === 1 ? "is" : "are"} already on ${remote}`,
          body: [`Taking ${them} off ${branch} would need a force push and break ${branch} for anyone who pulled. ${s.count === 1 ? "A revert keeps" : "Reverts keep"} ${branch}’s history as it is; push ${branch} when you’re ready.`],
        }
      : undefined,
    invalid: (s.create && !target ? "Type a name for the new branch." : problem) ?? (!s.create && !target ? "Pick a branch." : null) ?? mergeBlock,
    note: pushed ? "Nothing is pushed. If a step stops on conflicts, the steps before it stay done and Undo takes them back." : "Nothing is pushed. Undo takes it back, from the toast or the Operation Log.",
    stepWords: stepWords(ctx, {
      them,
      createSub: !pushed && base ? `Your files don’t change: the new branch starts where ${branch} is now.` : undefined,
    }),
    back: { label: "Other Fixes", run: fc.back },
    always: true,
    status: `Moving ${n} to ${target}…`,
    done: `Moved ${n} to ${target}.`,
    recover: (failure) => fixRecovery(fc, "move", failure),
    action: fixAction(fc, { kind: "moveCommits", count: s.count, to: target, create: s.create }),
  };
}

function forgotSheet(fc: FixContext, picked: Set<string>): Request {
  const { ctx, state } = fc;
  const head = chain(ctx)[0];
  // Staged files first: they are the likely ones, and the list shows 15 at most.
  const pickable = [...state.changed].sort((a, b) => Number(b.staged) - Number(a.staged));
  const paths = pickable.filter((f) => picked.has(f.path)).map((f) => f.path);
  const toggle = (path: string) => {
    const next = new Set(picked);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    return forgotSheet(fc, next);
  };
  const body: Part[] = ["Puts the files you pick into ", { code: shortId(head.id) }, " ", { quote: head.summary }, " on ", here(ctx), ". Its message stays; the commit gets a new SHA."];
  if (paths.length < pickable.length) body.push(" The other changed files stay as they are, staged or not.");
  const pushed = onRemote(ctx, head);
  const remote = remoteName(ctx);
  const shown = pickable.slice(0, 15);
  const action: Action = fixAction(fc, { kind: "addToLast", paths });
  return {
    title: "Add files to the last commit?",
    body,
    icon: "stage",
    tone: pushed ? "warn" : undefined,
    button: "Add to Commit",
    options: shown.map((f) => ({
      label: f.path,
      sub: f.staged && f.unstaged ? `${f.status}, partly staged: all of it goes in` : f.staged ? `${f.status}, staged` : f.status,
      on: picked.has(f.path),
      toggle: () => toggle(f.path),
    })),
    warning: pushed
      ? {
          title: `It is already on ${remote}`,
          body: [`The commit gets a new SHA, so the next push has to replace the old one with a force push. Anyone who pulled it keeps the old version.`],
        }
      : pickable.length > shown.length
        ? { title: `${pickable.length - shown.length} more changed files`, body: ["Only the first 15 are listed, staged files first. To add others, stage them in Changes first."] }
        : undefined,
    invalid: paths.length ? null : "Pick the files to add.",
    note: "Nothing is pushed. Undo takes it back, from the toast or the Operation Log.",
    stepWords: stepWords(ctx),
    back: { label: "Other Fixes", run: fc.back },
    always: true,
    status: "Adding to the last commit…",
    done: `Added ${paths.length === 1 ? paths[0] : plural(paths.length, "file")} to “${head.summary}”.`,
    recover: (failure) => forgotRecovery(fc, failure, paths, {}, head.summary),
    action,
  };
}

/** The way out when the secrets check or a check before commit stopped adding forgotten files;
 *  going on anyway keeps what was allowed before (`allowed`). */
function forgotRecovery(fc: FixContext, failure: Failure, paths: string[], allowed: { allowSecrets?: boolean; skipChecks?: boolean }, summary: string): Recovery | null {
  const anyway = (more: typeof allowed) => {
    const next = { ...allowed, ...more };
    return {
      label: "Add Anyway",
      danger: true,
      action: fixAction(fc, { kind: "addToLast", paths, ...next }),
      status: "Adding to the last commit…",
      done: `Added to “${summary}”.`,
      recover: (again: Failure) => forgotRecovery(fc, again, paths, next, summary),
    };
  };
  if (failure.kind === "secrets")
    return {
      title: "Possible secrets in the files",
      body: ["Oxbow checked the staged lines before changing the commit and stopped: these look like a password or key. Once pushed, a secret has to be revoked."],
      icon: "key",
      tone: "warn",
      secrets: failure.secrets ?? [],
      button: anyway({ allowSecrets: true }),
      note: "The files stay staged. A line with oxbow:allow is never flagged.",
      close: "Close",
    };
  if (failure.kind === "check") return { ...checkFailed(failure, "changing the commit"), button: anyway({ skipChecks: true }), note: "The files stay staged.", close: "Close" };
  return fixRecovery(fc, "forgot", failure);
}

function undoSheet(fc: FixContext, mode: ResetMode): Request {
  const { ctx } = fc;
  const head = chain(ctx)[0];
  const pushed = onRemote(ctx, head);
  const remote = remoteName(ctx);
  const staged = ctx.history.changes?.staged.length ?? 0;
  const body: Part[] = ["Takes ", { code: shortId(head.id) }, " ", { quote: head.summary }, " off ", here(ctx), "."];
  body.push(mode === "soft" ? " Its changes go back to staged, so you can change them and commit again." : " Its changes stay in your files, unstaged.");
  if (mode === "mixed" && staged) body.push(` Your ${plural(staged, "staged file")} become${staged === 1 ? "s" : ""} unstaged too.`);
  if (head.parents.length > 1) body.push(" It is a merge: the branch goes back to its first parent.");
  const t = ctx.history.tracking;
  const canPush = !!(ctx.history.head.branch && t && !t.gone && !t.behind);
  return {
    title: "Undo the last commit?",
    body,
    icon: "undo",
    tone: pushed ? "warn" : undefined,
    button: "Undo Commit",
    fields: [
      {
        label: "Changes",
        chips: [
          { label: "Keep Staged", on: mode === "soft", pick: () => undoSheet(fc, "soft") },
          { label: "Keep Unstaged", on: mode === "mixed", pick: () => undoSheet(fc, "mixed") },
        ],
      },
    ],
    warning: pushed
      ? {
          title: `It is already on ${remote}`,
          body: [`Undoing it here means the next push has to be a force push, and ${remote} keeps it until then. To take it back for everyone, revert it instead.`],
        }
      : undefined,
    alt: pushed ? { label: "Revert Instead", request: () => revertSheet(fc, head.id, canPush) } : undefined,
    note: "Nothing is pushed. Undo takes it back, from the toast or the Operation Log.",
    stepWords: stepWords(ctx, { summary: head.summary }),
    back: { label: "Other Fixes", run: fc.back },
    always: true,
    status: `Undoing ${shortId(head.id)}…`,
    done: `Undid ${shortId(head.id)}. Its changes are ${mode === "soft" ? "staged" : "in your files"}.`,
    recover: (failure) => fixRecovery(fc, "undo", failure),
    action: fixAction(fc, { kind: "undoLast", mode }),
  };
}

/** Pushed commits of the checked-out branch, newest first; the one the list was opened on first. */
function revertCandidates(fc: FixContext): HistoryRow[] {
  const commits = chain(fc.ctx).filter((r) => onRemote(fc.ctx, r));
  const first = commits.find((r) => r.id === fc.commit);
  const rest = commits.filter((r) => r !== first).slice(0, first ? 5 : 6);
  return first ? [first, ...rest] : rest;
}

function revertSheet(fc: FixContext, commit: string, push: boolean): Request {
  const { ctx } = fc;
  const candidates = revertCandidates(fc);
  const extra = !candidates.some((r) => r.id === commit) ? chain(ctx).find((r) => r.id === commit) : undefined;
  const rows = extra ? [extra, ...candidates] : candidates;
  const row = rows.find((r) => r.id === commit) ?? rows[0];
  const t = ctx.history.tracking;
  const canPush = !!(ctx.history.head.branch && t && !t.gone);
  const pushing = canPush && push;
  const body: Part[] = ["Adds a commit on ", here(ctx), " that reverses ", { code: shortId(row.id) }, " ", { quote: row.summary }, "."];
  body.push(row.parents.length > 1 ? " It is a merge: the new commit takes out everything it brought in." : " The original stays in history, so everyone who pulled it keeps a working branch.");
  return {
    title: "Undo a pushed commit?",
    body,
    icon: "revert",
    button: pushing ? "Revert and Push" : "Revert",
    fields: [
      {
        label: "Commit",
        chips: rows.map((r) => ({ label: shortId(r.id), mono: true, on: r.id === row.id, pick: () => revertSheet(fc, r.id, push) })),
        note: `“${row.summary}” · ${row.authorName} · ${relativeTime(row.time)}`,
      },
    ],
    options: canPush
      ? [
          {
            label: `Push the revert to ${t!.remote}/${t!.branch}`,
            sub: t!.behind
              ? `${t!.remote}/${t!.branch} has ${plural(t!.behind, "commit")} you don’t have yet: pull first, or the push is refused`
              : t!.ahead
                ? `So the change is undone for everyone. It also sends your ${plural(t!.ahead, "commit")} not pushed yet`
                : "So the change is undone for everyone",
            on: pushing,
            toggle: () => revertSheet(fc, row.id, !push),
          },
        ]
      : [],
    note: pushing ? "Undo takes the revert back here, but not the push." : "Nothing is pushed. Undo takes it back, from the toast or the Operation Log.",
    stepWords: stepWords(ctx, { subject: [{ code: shortId(row.id) }] }),
    back: { label: "Other Fixes", run: fc.back },
    always: true,
    status: `Reverting ${shortId(row.id)}…`,
    done: pushing ? `Reverted ${shortId(row.id)} and pushed ${hereName(ctx)}.` : `Reverted ${shortId(row.id)} on ${hereName(ctx)}.`,
    recover: (failure) => fixRecovery(fc, "revert", failure),
    action: fixAction(fc, { kind: "revertPushed", commit: row.id, push: pushing }),
  };
}

interface Rescue {
  name: string;
  /** A commit only the reflog keeps; null for HEAD. */
  commit: string | null;
  switchTo: boolean;
}

const KEEP_PREFIXES = ["experiment/", "feature/", "fix/"];

function detachedSheet(fc: FixContext, s: Rescue): Request {
  const { ctx, state } = fc;
  const again = (change: Partial<Rescue>) => detachedSheet(fc, { ...s, ...change });
  const name = s.name.trim();
  const problem = nameProblem(ctx, name);
  const nameChip: Part = name && !problem ? chip(ctx, name) : "a new branch";
  const withPrefix = (prefix: string) => {
    const bare = KEEP_PREFIXES.reduce((n, p) => (n.startsWith(p) ? n.slice(p.length) : n), s.name);
    return s.name.startsWith(prefix) ? bare : prefix + bare;
  };
  const nameField = {
    label: "Name",
    text: { value: s.name, placeholder: "experiment/new-cache", focus: true, edit: (value: string) => again({ name: value }) },
    chips: KEEP_PREFIXES.map((p) => ({ label: p, on: s.name.startsWith(p), pick: () => again({ name: withPrefix(p) }) })),
    error: name && problem ? problem : undefined,
  };
  const common = {
    icon: "branch" as Icon,
    button: "Create Branch",
    invalid: problem,
    note: "Nothing is pushed. Undo takes it back, from the toast or the Operation Log.",
    stepWords: stepWords(ctx),
    back: { label: "Other Fixes", run: fc.back },
    always: true,
    status: `Creating ${name}…`,
    recover: (failure: Failure) => fixRecovery(fc, "detached", failure),
  };
  if (!ctx.history.head.branch) {
    const lost = onNoBranch(ctx.history);
    const commit = ctx.history.head.commit;
    const body: Part[] = ["Creates ", nameChip, " at ", { code: commit ? shortId(commit) : "HEAD" }, ", where HEAD is now, and switches to it. "];
    body.push(lost.length ? `The ${plural(lost.length, "commit")} you made without a branch now belong to it, so they show up in the sidebar and can be pushed. Your files do not change.` : "New commits go to this branch. Your files do not change.");
    return {
      ...common,
      title: lost.length ? `Keep ${plural(lost.length, "commit")} on a new branch?` : "Put HEAD on a new branch?",
      body,
      fields: [nameField],
      done: `Created ${name}. HEAD is on a branch again.`,
      action: fixAction(fc, { kind: "saveDetached", name, commit: null, switch: true }),
    };
  }
  const entry = state.lost.find((e) => e.id === s.commit) ?? state.lost[0];
  const body: Part[] = ["Creates ", nameChip, " at ", { code: shortId(entry.id) }, " ", { quote: entry.summary }, ", a commit no branch has any more."];
  body.push(entry.expires ? ` Only git’s reflog keeps it, until about ${new Date(entry.expires * 1000).toLocaleDateString("en-US", { month: "long", day: "numeric" })}.` : " Only git’s reflog keeps it.");
  if (s.switchTo) body.push(ctx.uncommitted ? ` Then switches to it; your ${plural(ctx.uncommitted, "uncommitted file")} come along.` : " Then switches to it.");
  return {
    ...common,
    title: "Bring back a commit left without a branch?",
    body,
    fields: [
      {
        label: "Commit",
        chips: state.lost.map((e) => ({ label: shortId(e.id), mono: true, on: e.id === entry.id, pick: () => again({ commit: e.id }) })),
        note: `“${entry.summary}” · ${relativeTime(entry.time)}`,
      },
      nameField,
    ],
    options: [{ label: "Switch to it", sub: `Without it you stay on ${ctx.history.head.branch}`, on: s.switchTo, toggle: () => again({ switchTo: !s.switchTo }) }],
    done: `Created ${name} at ${shortId(entry.id)}.`,
    action: fixAction(fc, { kind: "saveDetached", name, commit: entry.id, switch: s.switchTo }),
  };
}

/** What the list says about the repository under its title. */
export function fixStateLine(fc: FixContext): Part[] {
  const { ctx, state } = fc;
  const h = ctx.history;
  const parts: Part[] = [];
  if (h.head.branch) parts.push(chip(ctx, h.head.branch));
  else parts.push("detached HEAD at ", { code: h.head.commit ? shortId(h.head.commit) : "HEAD" });
  const unpushed = chain(ctx).filter((r) => !onRemote(ctx, r)).length;
  if (h.refs.some((r) => r.kind === "remote")) parts.push(unpushed ? ` · ${plural(unpushed, "commit")} not pushed` : " · everything pushed");
  parts.push(state.changed.length ? ` · ${plural(state.changed.length, "changed file")}` : " · no changed files");
  return parts;
}

/** The repository's fix state, for opening the list. */
export const loadFixState = () => api.fixState();
