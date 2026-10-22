// Go to Branch… (⌘T): the repository's branches, tags, stashes, worktrees and submodules, one
// type at a time, each with its full path and its state, and what can be done with it. The
// lists, the search and the actions are here; ObjectPicker.svelte shows them.

import { fit } from "./palette";
import { checking, stale } from "./folders";
import { submoduleNote, submoduleWhere } from "./submodules";
import { shortId, shortTime } from "./format";
import type { History, HistoryRow, RefInfo, StashInfo, Submodule, Worktree } from "./types";

export type ObjectKind = "branches" | "tags" | "stashes" | "worktrees" | "submodules";

export const KINDS: { kind: ObjectKind; title: string; one: string }[] = [
  { kind: "branches", title: "Branches", one: "branch" },
  { kind: "tags", title: "Tags", one: "tag" },
  { kind: "stashes", title: "Stashes", one: "stash" },
  { kind: "worktrees", title: "Worktrees", one: "worktree" },
  { kind: "submodules", title: "Submodules", one: "submodule" },
];

/** A word about an object's state, at the end of its row. */
export type Note = { text: string; tone?: "accent" | "warn" | "dim" };

export type ObjectItem = {
  key: string;
  kind: ObjectKind;
  /** What the row is called, and what the search looks at first. */
  name: string;
  /** Where it is in full: its ref, its folder, its path in the repository. Searched too. */
  path: string;
  /** The commit it points at, in a few words; empty when History doesn't have it yet. */
  about: string;
  /** The commit Show in History selects; null when there is none to show. */
  commit: string | null;
  /** The branch whose color the row's dot takes; null for none. */
  branch: string | null;
  notes: Note[];
  ref?: RefInfo;
  stash?: StashInfo;
  worktree?: Worktree;
  submodule?: Submodule;
};

export type ObjectAction = { id: "show" | "open" | "checkout" | "stashes" | "details"; label: string };

/** The commit in a few words: its short SHA, and its subject when History has it. */
function aboutOf(id: string | null, rows: Map<string, HistoryRow>): string {
  if (!id) return "";
  const row = rows.get(id);
  return row ? `${shortId(id)} ${row.summary}` : shortId(id);
}

/** Every object of every type, in the order the picker lists them. `tilde` writes a folder the
 *  way people read it. */
export function objectsOf(history: History, tilde: (path: string) => string = (path) => path, now = Date.now() / 1000): Record<ObjectKind, ObjectItem[]> {
  const rows = new Map(history.rows.filter((r) => !r.worktree).map((r) => [r.id, r]));
  const time = (id: string) => rows.get(id)?.time ?? 0;
  const head = history.head;
  const elsewhere = (branch: string) => history.worktrees.find((t) => !t.current && t.branch === branch) ?? null;

  const locals = history.refs.filter((r) => r.kind === "local");
  // A remote branch a local one tracks, or one of the same name, is listed as that local branch.
  const tracked = new Set(locals.flatMap((r) => [...history.remotes.map((remote) => `${remote}/${r.name}`), ...(r.tracking ? [`${r.tracking.remote}/${r.tracking.branch}`] : [])]));
  const byTime = (a: RefInfo, b: RefInfo) => time(b.target) - time(a.target) || a.name.localeCompare(b.name);

  const branches: ObjectItem[] = [
    ...locals
      .slice()
      .sort((a, b) => (b.name === head.branch ? 1 : 0) - (a.name === head.branch ? 1 : 0) || byTime(a, b))
      .map((ref): ObjectItem => {
        const notes: Note[] = [];
        const t = ref.tracking;
        const wt = elsewhere(ref.name);
        if (ref.name === head.branch) notes.push({ text: "checked out", tone: "accent" });
        else if (wt) notes.push(wt.missing ? { text: `in ${wt.name}, folder gone`, tone: "warn" } : { text: `in ${wt.name}` });
        if (t?.gone) notes.push({ text: `deleted on ${t.remote}`, tone: "warn" });
        else if (t) {
          if (t.ahead) notes.push({ text: `↑${t.ahead}` });
          if (t.behind) notes.push({ text: `↓${t.behind}` });
        } else if (history.remotes.length) notes.push({ text: "not pushed", tone: "dim" });
        const upstream = t && !t.gone ? ` → ${t.remote}/${t.branch}` : "";
        return { key: `branch:${ref.name}`, kind: "branches", name: ref.name, path: `refs/heads/${ref.name}${upstream}`, about: aboutOf(ref.target, rows), commit: ref.target, branch: ref.name, notes, ref };
      }),
    ...history.refs
      .filter((r) => r.kind === "remote" && !tracked.has(r.name))
      .sort(byTime)
      .map((ref): ObjectItem => {
        const short = ref.name.slice((ref.remote ?? "").length + 1);
        return { key: `remote:${ref.name}`, kind: "branches", name: ref.name, path: `refs/remotes/${ref.name}`, about: aboutOf(ref.target, rows), commit: ref.target, branch: short, notes: [{ text: `only on ${ref.remote}`, tone: "dim" }], ref };
      }),
  ];

  const tags = history.refs
    .filter((r) => r.kind === "tag")
    .sort((a, b) => time(b.target) - time(a.target) || b.name.localeCompare(a.name))
    .map((ref): ObjectItem => {
      const notes: Note[] = [];
      if (ref.target === head.commit) notes.push({ text: "at HEAD", tone: "accent" });
      if (history.defaultRemote && history.localTags.includes(ref.name)) notes.push({ text: "not pushed", tone: "dim" });
      return { key: `tag:${ref.name}`, kind: "tags", name: ref.name, path: `refs/tags/${ref.name}`, about: aboutOf(ref.target, rows), commit: ref.target, branch: null, notes, ref };
    });

  const stashes = history.stashes.map(
    (s): ObjectItem => ({
      key: `stash:${s.id}`,
      kind: "stashes",
      name: s.title || s.message,
      path: `stash@{${s.index}} · made on ${s.branch ?? "no branch"} at ${shortId(s.base)}`,
      about: "",
      commit: s.id,
      branch: s.branch,
      notes: [{ text: shortTime(s.time, now), tone: "dim" }],
      stash: s,
    }),
  );

  const worktrees = history.worktrees.map((wt): ObjectItem => {
    const notes: Note[] = [];
    if (wt.current) notes.push({ text: "this window", tone: "accent" });
    if (wt.missing) notes.push({ text: "folder gone", tone: "warn" });
    else if (!wt.current) {
      if (checking(wt)) notes.push({ text: "checking…", tone: "dim" });
      else if (wt.changed === null) notes.push({ text: "status unknown", tone: "warn" });
      else if (stale(wt)) notes.push({ text: "may be stale", tone: "warn" });
      else if (wt.changed) notes.push({ text: `${wt.changed} changed` });
    }
    if (wt.locked !== null) notes.push({ text: "locked", tone: "dim" });
    const at = wt.branch ?? (wt.head ? `detached at ${shortId(wt.head)}` : "no commits");
    return { key: `worktree:${wt.path}`, kind: "worktrees", name: wt.name, path: tilde(wt.path), about: at, commit: wt.head, branch: wt.branch, notes, worktree: wt };
  });

  const submodules = history.submodules.map((sub): ObjectItem => {
    const note = submoduleNote(sub);
    const notes: Note[] = note ? [{ text: note.text, tone: note.warn ? "warn" : note.muted ? "dim" : undefined }] : [];
    return { key: `submodule:${sub.path}`, kind: "submodules", name: sub.title, path: sub.url ? `${sub.path} · ${sub.url}` : sub.path, about: submoduleWhere(sub), commit: null, branch: null, notes, submodule: sub };
  });

  return { branches, tags, stashes, worktrees, submodules };
}

/** Where the query was found: the letters of the name or of the path it matched. */
export type Found = { item: ObjectItem; name: number[]; path: number[] };

/** The items a query finds, best first: in the name before in the path, at a word's start
 *  before inside one. All of them, in their order, for no query. */
export function search(items: ObjectItem[], query: string): Found[] {
  if (!query.trim()) return items.map((item) => ({ item, name: [], path: [] }));
  return items
    .flatMap((item, at): { found: Found; score: number; at: number }[] => {
      const byName = fit(item.name, query);
      if (byName) return [{ found: { item, name: byName.hits, path: [] }, score: 100 + byName.score, at }];
      const byPath = fit(item.path, query);
      return byPath ? [{ found: { item, name: [], path: byPath.hits }, score: byPath.score, at }] : [];
    })
    .sort((a, b) => b.score - a.score || a.at - b.at)
    .map((r) => r.found);
}

/** Branches, tags and the rest whose name has the query, for the bar under History's search:
 *  the best `limit` of them, how many there are in all, and the type most of them are. */
export function namedLike(all: Record<ObjectKind, ObjectItem[]>, query: string, limit = 3): { items: ObjectItem[]; total: number; kind: ObjectKind } | null {
  if (query.trim().length < 2) return null;
  const found = KINDS.map((k) => ({ kind: k.kind, items: search(all[k.kind], query).filter((f) => f.name.length).map((f) => f.item) }));
  const total = found.reduce((n, f) => n + f.items.length, 0);
  if (!total) return null;
  const kind = found.reduce((best, f) => (f.items.length > best.items.length ? f : best)).kind;
  return { items: found.flatMap((f) => f.items).slice(0, limit), total, kind };
}

/** What can be done with an object, the first being what Return does. */
export function actionsOf(item: ObjectItem, history: History): ObjectAction[] {
  const out: ObjectAction[] = [];
  const show: ObjectAction = { id: "show", label: "Show in History" };
  switch (item.kind) {
    case "branches": {
      const ref = item.ref!;
      out.push(show);
      if (ref.kind === "local") {
        const wt = history.worktrees.find((t) => !t.current && t.branch === ref.name);
        if (wt && !wt.missing) out.push({ id: "open", label: "Open Window" });
        else if (!wt && ref.name !== history.head.branch) out.push({ id: "checkout", label: "Check Out" });
      } else {
        const short = item.branch!;
        if (!history.refs.some((r) => r.kind === "local" && r.name === short)) out.push({ id: "checkout", label: "Check Out" });
      }
      break;
    }
    case "tags":
      out.push(show);
      if (item.commit !== history.head.commit) out.push({ id: "checkout", label: "Check Out" });
      break;
    case "stashes":
      out.push(show, { id: "stashes", label: "Show in Stashes" });
      break;
    case "worktrees": {
      const wt = item.worktree!;
      if (!wt.current && !wt.missing) out.push({ id: "open", label: "Open Window" });
      if (wt.head) out.push(show);
      break;
    }
    case "submodules":
      if (item.submodule!.cloned) out.push({ id: "open", label: "Open Window" });
      out.push({ id: "details", label: "Show Details" });
      break;
  }
  return out;
}

/** What the list says when a type has nothing, or nothing that matches. */
export function emptyText(kind: ObjectKind, query: string): string {
  if (query.trim()) return `No ${KINDS.find((k) => k.kind === kind)!.title.toLowerCase()} match “${query.trim()}”.`;
  switch (kind) {
    case "branches":
      return "No branches yet: the first commit makes one.";
    case "tags":
      return "No tags yet.";
    case "stashes":
      return "No stashes.";
    case "worktrees":
      return "Only this folder.";
    case "submodules":
      return "No submodules in this repository.";
  }
}

/** Why History can't show a commit: no branch or tag reaches it, as when a worktree made
 *  commits on no branch. */
export function notInHistory(commit: string, worktree?: Worktree): string {
  const there = worktree && !worktree.current ? ` Open ${worktree.name}’s window to see it in its history.` : "";
  return `${shortId(commit)} is on no branch or tag here, so History can’t show it.${there}`;
}

/** "3 branches", "1 stash", for the tabs' tooltips. */
export function countOf(kind: ObjectKind, n: number): string {
  const what = KINDS.find((k) => k.kind === kind)!;
  return `${n} ${n === 1 ? what.one : what.title.toLowerCase()}`;
}
