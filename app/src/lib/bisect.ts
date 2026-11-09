// Bisect: finding the commit that broke something. git checks out commits between one that
// worked and one that is broken; each answer halves what is left. The sheet to start it, the
// answers (from the banner, the commit menu or G / B / S), and going back when it is done.

import type { Part, Request } from "./confirm.svelte";
import type { BranchContext } from "./branches";
import type { Action, Bisect, BisectMark, History, HistoryRow } from "./types";
import { ancestors } from "./historyAncestry.ts";
import { shortId } from "./format.ts";

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

/** git's guess of how many steps are left after testing one of `all` commits (bisect.c). */
export function estimateSteps(all: number): number {
  if (all < 3) return 0;
  const n = Math.floor(Math.log2(all));
  const e = 2 ** n;
  const x = all - e;
  return e < 3 * x ? n : n - 1;
}

interface Boundary {
  name: string;
  id: string;
}

/** A branch choice, excluding tags that happen to share its name. */
function branchTip(history: History, name: string | null): Boundary | null {
  const ref = history.refs.find((r) => r.kind === "local" && r.name === name)
    ?? history.refs.find((r) => r.kind === "remote" && r.name === name);
  return ref ? { name: ref.name, id: ref.target } : null;
}

/** Keep the first choice for each label, as the sheet keys its chips by label. */
function distinct(choices: (Boundary | null)[]): Boundary[] {
  return choices.filter((choice, i): choice is Boundary =>
    choice !== null && choices.findIndex((other) => other?.name === choice.name) === i);
}

/** What to call a commit: a branch or tag there, else its short sha. */
function nameOf(history: History, id: string): string {
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
  good: Boundary | null;
  bad: Boundary | null;
  /** The choices offered for each. */
  goods: Boundary[];
  bads: Boundary[];
  stash: boolean;
  test: boolean;
  command: string;
}

/** Commits that may be good: the newest few tags behind `bad`, and where `bad` left the trunk. */
function goodChoices(history: History, badId: string | null): Boundary[] {
  if (!badId) return [];
  const behind = ancestors(history, badId);
  behind.delete(badId);
  const tagged = history.rows
    .filter((r) => behind.has(r.id))
    .flatMap((r) => r.labels.filter((l) => l.kind === "tag").map((l) => ({ name: l.name, id: r.id })))
    .slice(0, 3)
    .reverse();
  if (tagged.length) return distinct(tagged);
  // No tags: where the branch split from the trunk, when it isn't on the trunk itself.
  const trunkTip = branchTip(history, history.trunk)?.id ?? null;
  if (!trunkTip || trunkTip === badId) return [];
  const onTrunk = ancestors(history, trunkTip);
  const fork = history.rows.find((r) => behind.has(r.id) && onTrunk.has(r.id));
  return fork ? [{ name: nameOf(history, fork.id), id: fork.id }] : [];
}

/** Find What Broke This… on `bad` (a branch or commit), or Find First Bad Commit After This…
 *  on `good`, an older commit; the rest is filled in from the checked-out branch and tags. */
export function bisectStartRequest(ctx: BranchContext, from: { good?: HistoryRow; bad?: HistoryRow }): Request {
  const { history } = ctx;
  const { head, trunk } = history;
  const here = head.commit ? { name: head.branch ?? shortId(head.commit), id: head.commit } : null;
  const bad = from.bad ? { name: nameOf(history, from.bad.id), id: from.bad.id } : here;
  const bads = distinct([bad, here, branchTip(history, trunk)])
    .filter((choice) => !from.good || ancestors(history, choice.id).has(from.good.id));
  const initialBad = bads[0] ?? bad;
  const suggestions = goodChoices(history, initialBad?.id ?? null);
  const goods = from.good ? distinct([{ name: nameOf(history, from.good.id), id: from.good.id }, ...suggestions]) : suggestions;
  return startSheet(ctx, {
    good: goods[0] ?? null,
    bad: initialBad,
    goods,
    bads: bads.length ? bads : distinct([bad]),
    stash: ctx.uncommitted > 0,
    test: false,
    command: "",
  });
}

function startSheet(ctx: BranchContext, state: Start): Request {
  const again = (change: Partial<Start>) => startSheet(ctx, { ...state, ...change });
  const { history } = ctx;
  const here = history.head.branch ?? (history.head.commit ? shortId(history.head.commit) : "HEAD");
  const good = state.good?.name ?? "a good commit";
  const bad = state.bad?.name ?? here;
  const goodId = state.good?.id ?? null;
  const badId = state.bad?.id ?? null;
  const beforeBad = ancestors(history, badId);
  const beforeGood = ancestors(history, goodId);
  const range = goodId && badId ? [...beforeBad].filter((id) => !beforeGood.has(id)).length : 0;
  const command = state.command.trim();

  let invalid: string | null = null;
  if (!goodId) invalid = "Pick a commit where it still worked: right-click an older commit and choose Find First Bad Commit After This…";
  else if (!badId) invalid = `${bad} isn’t in the history.`;
  else if (goodId === badId || !beforeBad.has(goodId)) invalid = `${good} isn’t older than ${bad}: pick a commit ${bad} grew from.`;
  else if (range < 2) invalid = `There is nothing between ${good} and ${bad} to test: ${bad} is the first bad commit.`;
  else if (state.test && !command) invalid = "Type the command that tests a commit.";

  const body: Part[] = ["Git checks out commits between ", named(ctx, good), ", which worked, and ", named(ctx, bad), ", which is broken."];
  if (range >= 2) body.push(` Each answer halves the range: ${plural(range, "commit")}, about ${plural(estimateSteps(range) + 1, "step")}.`);
  body.push(state.test ? " The test command runs at every step: exit code 0 means good, 125 skip, anything else bad." : " At each step you build and test, then mark the commit Good or Bad.");

  const fields: Request["fields"] = [
    {
      label: "Good",
      chips: state.goods.map((choice) => ({ label: choice.name, mono: !isBranch(history, choice.name) && !isTag(history, choice.name), on: state.good === choice, pick: () => again({ good: choice }) })),
      note: state.goods.length ? "A commit where it still worked." : "No tags behind it. Right-click an older commit in History and choose Find First Bad Commit After This…",
    },
    {
      label: "Bad",
      chips: state.bads.map((choice) => ({ label: choice.name, mono: !isBranch(history, choice.name) && !isTag(history, choice.name), on: state.bad === choice, pick: () => again({ bad: choice }) })),
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

  const action: Action = { kind: "bisectStart", good: goodId ?? "", bad: badId ?? "", stash: state.stash && ctx.uncommitted > 0, command: state.test ? command : null };
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
    ? ["Goes back to ", back, `${stash}. The first bad commit was `, { code: shortId(found.id) }, " ", { quote: found.summary }, ". It stays in History. To keep the steps, use Copy Report in the banner before finishing."]
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
