// Bisect: finding the commit that broke something. git checks out commits between one that
// worked and one that is broken; each answer halves what is left. The sheet to start it, the
// answers (from the banner, the commit menu or G / B / S), and going back when it is done.

import type { Part, Request } from "./confirm.svelte";
import type { BranchContext } from "./branches";
import type { Action, Bisect, BisectMark, History, HistoryRow } from "./types";
import { ancestors } from "./commits";
import { shortId } from "./format";

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

/** git's guess of how many steps are left after testing one of `all` commits (bisect.c). */
export function estimateSteps(all: number): number {
  if (all < 3) return 0;
  const n = Math.floor(Math.log2(all));
  const e = 2 ** n;
  const x = all - e;
  return e < 3 * x ? n : n - 1;
}

/** The commit a branch, tag or sha names, as far as History knows. */
function resolve(history: History, name: string): string | null {
  const ref = history.refs.find((r) => (r.kind === "local" || r.kind === "tag" || r.kind === "remote") && r.name === name);
  if (ref) return ref.target;
  return history.rows.find((r) => !r.worktree && r.id.startsWith(name))?.id ?? null;
}

/** What to call a commit: a branch or tag there, else its short sha. */
export function nameOf(history: History, id: string): string {
  for (const kind of ["local", "tag", "remote"] as const) {
    const ref = history.refs.find((r) => r.kind === kind && r.target === id);
    if (ref) return ref.name;
  }
  return shortId(id);
}

const isTag = (history: History, name: string) => history.refs.some((r) => r.kind === "tag" && r.name === name);
const isBranch = (history: History, name: string) => history.refs.some((r) => (r.kind === "local" || r.kind === "remote") && r.name === name);

/** A name as a capsule: a branch in its color, anything else in mono. */
function named(ctx: BranchContext, name: string): Part {
  return isBranch(ctx.history, name) ? { branch: name, color: ctx.colorOf(name) } : { code: name };
}

interface Start {
  good: string;
  bad: string;
  /** The choices offered for each. */
  goods: string[];
  bads: string[];
  stash: boolean;
  test: boolean;
  command: string;
}

/** Commits that may be good: the newest few tags behind `bad`, and where `bad` left the trunk. */
function goodChoices(history: History, bad: string): string[] {
  const badId = resolve(history, bad);
  if (!badId) return [];
  const behind = ancestors(history, badId);
  behind.delete(badId);
  const tagged = history.rows
    .filter((r) => behind.has(r.id))
    .flatMap((r) => r.labels.filter((l) => l.kind === "tag").map((l) => l.name))
    .slice(0, 3)
    .reverse();
  if (tagged.length) return tagged;
  // No tags: where the branch split from the trunk, when it isn't on the trunk itself.
  const trunkTip = history.trunk ? resolve(history, history.trunk) : null;
  if (!trunkTip || trunkTip === badId) return [];
  const onTrunk = ancestors(history, trunkTip);
  const fork = history.rows.find((r) => behind.has(r.id) && onTrunk.has(r.id));
  return fork ? [nameOf(history, fork.id)] : [];
}

/** Find What Broke This… on `bad` (a branch or commit), or Find First Bad Commit After This…
 *  on `good`, an older commit; the rest is filled in from the checked-out branch and tags. */
export function bisectStartRequest(ctx: BranchContext, from: { good?: HistoryRow; bad?: HistoryRow }): Request {
  const { history } = ctx;
  const here = history.head.branch ?? (history.head.commit ? shortId(history.head.commit) : "HEAD");
  const bad = from.bad ? nameOf(history, from.bad.id) : here;
  const bads = [...new Set([bad, here, history.trunk].filter((b): b is string => !!b))].filter((b) => !from.good || ancestors(history, resolve(history, b)).has(from.good.id));
  const goods = from.good ? [...new Set([nameOf(history, from.good.id), ...goodChoices(history, bad)])] : goodChoices(history, bad);
  return startSheet(ctx, {
    good: goods[0] ?? "",
    bad: bads[0] ?? bad,
    goods,
    bads: bads.length ? bads : [bad],
    stash: ctx.uncommitted > 0,
    test: false,
    command: "",
  });
}

function startSheet(ctx: BranchContext, state: Start): Request {
  const again = (change: Partial<Start>) => startSheet(ctx, { ...state, ...change });
  const { history } = ctx;
  const here = history.head.branch ?? (history.head.commit ? shortId(history.head.commit) : "HEAD");
  const goodId = state.good ? resolve(history, state.good) : null;
  const badId = resolve(history, state.bad);
  const range = goodId && badId ? [...ancestors(history, badId)].filter((id) => !ancestors(history, goodId).has(id)).length : 0;
  const command = state.command.trim();

  let invalid: string | null = null;
  if (!goodId) invalid = "Pick a commit where it still worked: right-click an older commit and choose Find First Bad Commit After This…";
  else if (!badId) invalid = `${state.bad} isn’t in the history.`;
  else if (goodId === badId || !ancestors(history, badId).has(goodId)) invalid = `${state.good} isn’t older than ${state.bad}: pick a commit ${state.bad} grew from.`;
  else if (range < 2) invalid = `There is nothing between ${state.good} and ${state.bad} to test: ${state.bad} is the first bad commit.`;
  else if (state.test && !command) invalid = "Type the command that tests a commit.";

  const body: Part[] = ["Git checks out commits between ", named(ctx, state.good || "a good commit"), ", which worked, and ", named(ctx, state.bad), ", which is broken."];
  if (range >= 2) body.push(` Each answer halves the range: ${plural(range, "commit")}, about ${plural(estimateSteps(range) + 1, "step")}.`);
  body.push(state.test ? " The test command runs at every step: exit code 0 means good, 125 skip, anything else bad." : " At each step you build and test, then mark the commit Good or Bad.");

  const fields: Request["fields"] = [
    {
      label: "Good",
      chips: state.goods.map((name) => ({ label: name, mono: !isBranch(history, name) && !isTag(history, name), on: state.good === name, pick: () => again({ good: name }) })),
      note: state.goods.length ? "A commit where it still worked." : "No tags behind it. Right-click an older commit in History and choose Find First Bad Commit After This…",
    },
    {
      label: "Bad",
      chips: state.bads.map((name) => ({ label: name, mono: !isBranch(history, name) && !isTag(history, name), on: state.bad === name, pick: () => again({ bad: name, goods: state.goods }) })),
      note: "A commit where it is broken.",
    },
  ];
  if (state.test) {
    fields.push({ label: "Test", text: { value: state.command, placeholder: "cargo test --test retry", edit: (value) => again({ command: value }) } });
  }
  const options: Request["options"] = [];
  if (ctx.uncommitted) {
    options.push({ label: `Stash the ${plural(ctx.uncommitted, "uncommitted file")} first`, sub: "Every step then builds exactly that commit", on: state.stash, toggle: () => again({ stash: !state.stash }) });
  }
  options.push({ label: "Answer each step with a test command", sub: "git bisect run: no clicking, the log shows every result", on: state.test, toggle: () => again({ test: !state.test }) });

  const action: Action = { kind: "bisectStart", good: state.good, bad: state.bad, stash: state.stash && ctx.uncommitted > 0, command: state.test ? command : null };
  return {
    title: "Find the commit that broke it",
    body,
    icon: "bisect",
    button: "Start Bisect",
    fields,
    options,
    invalid,
    note: `Stop any time: Oxbow takes you back to ${here}.`,
    status: state.test ? "Running bisect…" : "Starting bisect…",
    done: state.test ? "Bisect done: see the banner for the first bad commit." : "Bisecting. Test this commit, then mark it Good or Bad.",
    action,
  };
}

const MARK = {
  good: { button: "Mark as Good", says: "works here, so the first bad commit is newer", done: "good" },
  bad: { button: "Mark as Bad", says: "is broken here, so the first bad commit is this one or older", done: "bad" },
  skip: { button: "Skip Commit", says: "can’t be tested, so git picks a commit next to it", done: "skipped" },
} as const;

/** Good, Bad or Skip for the commit under test, or for `row` from its menu. */
export function bisectMarkRequest(ctx: BranchContext, mark: BisectMark, row?: HistoryRow): Request {
  const { history } = ctx;
  const id = row?.id ?? history.head.commit ?? "";
  const summary = row?.summary ?? history.rows.find((r) => r.id === id)?.summary ?? "";
  const testing = !row || row.id === history.head.commit;
  const words = MARK[mark];
  return {
    title: mark === "skip" ? `Skip ${shortId(id)}?` : `Mark ${shortId(id)} as ${mark}?`,
    body: [{ code: shortId(id) }, " ", { quote: summary }, ` ${words.says}.`, history.bisect?.found ? "" : " Git then checks out the next commit to test."],
    icon: "bisect",
    button: words.button,
    status: "Bisecting…",
    done: `Marked ${shortId(id)} as ${words.done}.`,
    action: { kind: "bisectMark", mark, commit: testing ? null : shortId(id) },
  };
}

/** Stop bisecting, or Finish once the first bad commit is found. */
export function bisectResetRequest(ctx: BranchContext, bisect: Bisect): Request {
  const { history } = ctx;
  const back: Part = bisect.startIsBranch ? named(ctx, bisect.start) : { code: shortId(bisect.start) };
  const backName = bisect.startIsBranch ? bisect.start : shortId(bisect.start);
  const stash = bisect.stashed ? " and brings back the uncommitted changes stashed before it" : "";
  const found = bisect.found ? history.rows.find((r) => r.id === bisect.found) : undefined;
  const body: Part[] = found
    ? ["Goes back to ", back, `${stash}. The first bad commit was `, { code: shortId(found.id) }, " ", { quote: found.summary }, ". It stays in History, and Copy Log in the banner keeps the whole bisect."]
    : ["Goes back to ", back, `${stash}. The answers so far are dropped; Copy Log in the banner keeps them for git bisect replay.`];
  return {
    title: bisect.found ? "Finish the bisect?" : "Stop bisecting?",
    body,
    icon: "bisect",
    button: bisect.found ? "Finish" : "Stop Bisect",
    status: "Going back…",
    done: `Back on ${backName}.`,
    action: { kind: "bisectReset" },
  };
}

/** How a commit stands in the bisect, for its pill in the graph. */
export type BisectPill = "good" | "bad" | "skipped" | "testing" | "found" | "possible";

export function bisectPills(bisect: Bisect | null): Map<string, BisectPill> {
  const pills = new Map<string, BisectPill>();
  if (!bisect) return pills;
  for (const id of bisect.skipped) pills.set(id, "skipped");
  for (const id of bisect.good) pills.set(id, "good");
  for (const id of bisect.bad) pills.set(id, "bad");
  for (const id of bisect.possible) pills.set(id, "possible");
  if (bisect.found) pills.set(bisect.found, "found");
  else if (bisect.testing && !bisect.possible.length && !pills.has(bisect.testing)) pills.set(bisect.testing, "testing");
  return pills;
}

/** Commits that matter to the bisect: the ones still possible and the ones answered. Empty
 *  while the range isn't known, so nothing fades. */
export function bisectRange(bisect: Bisect | null): Set<string> | null {
  if (!bisect || !bisect.candidates.length) return null;
  const found = bisect.found;
  const range = new Set(found ? [found] : bisect.candidates);
  for (const id of [...bisect.good, ...bisect.bad, ...bisect.skipped]) range.add(id);
  if (bisect.testing) range.add(bisect.testing);
  return range;
}
