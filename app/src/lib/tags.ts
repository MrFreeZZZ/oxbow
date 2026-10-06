// Confirmations for tags: making one, pushing, deleting and fetching them. Git never pushes tags
// with a branch, so a new tag offers to go to the remote right away, and tags the remote doesn't
// have yet are drawn as local until pushed.

import type { Part, Request } from "./confirm.svelte";
import type { BranchContext } from "./branches";
import type { Action, HistoryRow } from "./types";
import { shortId } from "./format";
import { remoteTrouble } from "./remote";

interface NewTag {
  name: string;
  annotated: boolean;
  message: string;
  /** Tag the picked commit rather than HEAD. */
  onCommit: boolean;
  push: boolean;
}

const tagNames = (ctx: BranchContext) => ctx.history.refs.filter((r) => r.kind === "tag").map((r) => r.name);

/** Tags the default remote doesn't have yet. */
export const isLocalTag = (ctx: BranchContext, name: string) => ctx.history.localTags.includes(name);

function nameProblem(ctx: BranchContext, name: string): string | null {
  if (!name) return "Type a name for the tag.";
  if (/\s/.test(name)) return "Tag names can’t contain spaces.";
  if (/[~^:?*[\\\x00-\x1f\x7f]/.test(name)) return "Tag names can’t contain ~ ^ : ? * [ or \\.";
  if (name === "HEAD" || name === "@" || /^[-/.]|[/.]$|\.lock$|\.\.|\/\/|@\{|\/\.|\.lock\//.test(name)) return "That isn’t a valid tag name.";
  if (tagNames(ctx).includes(name)) return `A tag named ${name} already exists.`;
  return null;
}

/** The next patch, minor and major versions after the highest `v1.2.3`-style tag. */
export function nextVersions(names: string[]): { latest: string | null; next: string[] } {
  let best: { name: string; prefix: string; v: number[] } | null = null;
  for (const name of names) {
    const m = /^(v?)(\d+)\.(\d+)\.(\d+)$/.exec(name);
    if (!m) continue;
    const v = [Number(m[2]), Number(m[3]), Number(m[4])];
    if (!best || compare(v, best.v) > 0) best = { name, prefix: m[1], v };
  }
  if (!best) return { latest: null, next: ["v0.1.0", "v1.0.0"].filter((n) => !names.includes(n)) };
  const [a, b, c] = best.v;
  const p = best.prefix;
  return { latest: best.name, next: [`${p}${a}.${b}.${c + 1}`, `${p}${a}.${b + 1}.0`, `${p}${a + 1}.0.0`].filter((n) => !names.includes(n)) };
}

function compare(x: number[], y: number[]): number {
  return x[0] - y[0] || x[1] - y[1] || x[2] - y[2];
}

/** New Tag: on `at` when given (a commit in the graph), else on HEAD. */
export function newTagRequest(ctx: BranchContext, at?: HistoryRow): Request {
  const onCommit = !!at && at.id !== ctx.history.head.commit;
  return newTagSheet(ctx, at ?? null, { name: "", annotated: true, message: "", onCommit, push: !!ctx.history.defaultRemote });
}

function newTagSheet(ctx: BranchContext, at: HistoryRow | null, state: NewTag): Request {
  const again = (change: Partial<NewTag>) => newTagSheet(ctx, at, { ...state, ...change });
  const { head, defaultRemote: remote } = ctx.history;
  const name = state.name.trim();
  const problem = nameProblem(ctx, name);
  const commit = state.onCommit && at ? at.id : head.commit!;
  const where: Part[] = state.onCommit && at ? ["commit ", { code: shortId(at.id) }, " ", { quote: at.summary }] : head.branch ? [{ branch: head.branch, color: ctx.colorOf(head.branch) }, ", commit ", { code: shortId(commit) }] : ["HEAD, commit ", { code: shortId(commit) }];
  const versions = nextVersions(tagNames(ctx));
  const push = state.push && remote ? remote : null;

  const body: Part[] = ["Tags ", ...where, name && !problem ? [" as ", { code: name }] : [], "."].flat() as Part[];
  if (push) body.push(` Then it goes to ${push}: git push never sends tags on its own.`);

  const message = state.annotated ? state.message.trim() || name : null;
  const action: Action = { kind: "createTag", name, commit: shortId(commit), message, push };
  const fields: Request["fields"] = [
    {
      label: "Name",
      text: { value: state.name, placeholder: versions.next[0] ?? "v1.0.0", edit: (value) => again({ name: value }) },
      chips: versions.next.map((v) => ({ label: v, mono: true, on: name === v, pick: () => again({ name: v }) })),
      error: name && problem ? problem : undefined,
      note: versions.latest ? `Latest version tag: ${versions.latest}` : undefined,
    },
    {
      label: "Kind",
      chips: [
        { label: "Annotated", on: state.annotated, pick: () => again({ annotated: true }) },
        { label: "Lightweight", on: !state.annotated, pick: () => again({ annotated: false }) },
      ],
      note: state.annotated ? "Keeps a message, who tagged and when. Best for releases." : "Just a name for the commit, nothing else.",
    },
  ];
  if (state.annotated) fields.push({ label: "Message", text: { value: state.message, placeholder: name ? `Release ${name}` : "What this version brings", multiline: true, edit: (value) => again({ message: value }) } });
  if (at && at.id !== head.commit) {
    fields.push({
      label: "On",
      chips: [
        { label: shortId(at.id), mono: true, on: state.onCommit, pick: () => again({ onCommit: true }) },
        { label: head.branch ? `${head.branch} (HEAD)` : "HEAD", on: !state.onCommit, pick: () => again({ onCommit: false }) },
      ],
    });
  }
  return {
    title: "New Tag",
    body,
    icon: "tag",
    button: push ? "Create and Push" : "Create Tag",
    fields,
    options: remote ? [{ label: `Push the tag to ${remote}`, sub: "So others and CI see it", on: state.push, toggle: () => again({ push: !state.push }) }] : undefined,
    invalid: problem,
    status: `Tagging ${name}…`,
    done: push ? `Created ${name} and pushed it to ${push}.` : `Created ${name}.`,
    // The tag is made; only the push failed, so Try Again pushes it.
    recover: (failure) => (push ? remoteTrouble(failure, { kind: "pushTags", remote: push, names: [name] }, "Pushing to", push, `Pushed ${name} to ${push}.`) : null),
    action,
  };
}

/** Push tags the remote doesn't have: one, or every local tag. */
export function pushTagsRequest(ctx: BranchContext, names: string[]): Request {
  const remote = ctx.history.defaultRemote!;
  const one = names.length === 1;
  const action: Action = { kind: "pushTags", remote, names };
  const list: Part[] = names.slice(0, 4).flatMap((n, i) => [i ? ", " : "", { code: n }]);
  if (names.length > 4) list.push(` and ${names.length - 4} more`);
  const done = one ? `Pushed ${names[0]} to ${remote}.` : `Pushed ${names.length} tags to ${remote}.`;
  return {
    title: one ? `Push ${names[0]} to ${remote}?` : `Push ${names.length} local tags to ${remote}?`,
    body: ["Sends ", ...list, ` to ${remote}, so others and CI see ${one ? "it" : "them"}. Branches are not pushed.`],
    icon: "push",
    button: one ? "Push Tag" : "Push Tags",
    status: `Pushing to ${remote}…`,
    done,
    recover: (failure) => remoteTrouble(failure, action, "Pushing to", remote, done),
    action,
  };
}

/** Delete a tag here, and on the remote when it is there too. */
export function deleteTagRequest(ctx: BranchContext, name: string, onRemote = !isLocalTag(ctx, name)): Request {
  const remote = ctx.history.defaultRemote;
  const pushed = !!remote && !isLocalTag(ctx, name);
  const both = pushed && onRemote;
  const action: Action = { kind: "deleteTag", name, remote: both ? remote : null };
  const done = both ? `Deleted ${name} here and on ${remote}.` : `Deleted ${name}.`;
  return {
    title: `Delete the tag ${name}?`,
    body: [
      "Deletes ",
      { code: name },
      both ? ` here and on ${remote}. If a GitHub release was made from it, the release turns into a draft.` : pushed ? ` here only. ${remote} keeps it, and the next fetch brings it back.` : ". The commit stays.",
    ],
    icon: "drop",
    button: "Delete Tag",
    danger: true,
    options: pushed ? [{ label: `Also delete it on ${remote}`, on: onRemote, toggle: () => deleteTagRequest(ctx, name, !onRemote) }] : undefined,
    status: `Deleting ${name}…`,
    done,
    recover: (failure) => (both ? remoteTrouble(failure, action, "Deleting on", remote!, done) : null),
    action,
  };
}

export function fetchTagsRequest(ctx: BranchContext): Request {
  const remote = ctx.history.defaultRemote!;
  const action: Action = { kind: "fetchTags", remote };
  const done = `Fetched the tags of ${remote}.`;
  return {
    title: `Fetch tags from ${remote}?`,
    body: ["Downloads every tag of ", { code: remote }, ", also ones on commits no branch has. Your local tags stay."],
    icon: "fetch",
    button: "Fetch Tags",
    note: "Your branches and files do not change.",
    status: `Fetching tags from ${remote}…`,
    done,
    recover: (failure) => remoteTrouble(failure, action, "Fetching", remote, done),
    action,
  };
}
