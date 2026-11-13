// Back and Forward, as in a browser or VS Code: the places visited, each a screen with what was
// picked on it, so a step back returns to where you were rather than to the commit graph.

import { compareLabel } from "./compareRefs.ts";
import type { FileMode } from "./nav.svelte";
import type { SeriesQuery } from "./series";
import type { CompareMode } from "./types";
import { wordsOf, type ForgeKind } from "./forge.ts";

export type HistoryPosition = { id: string; offset: number };

/** A screen and what was picked on it. */
export type Place =
  /** History: the selected commit, and the first visible row and its offset when you left it. */
  | { view: "history"; commit: string | null; scroll: HistoryPosition | null }
  /** File History: the file, its selected commit, Changes or Blame, Blame's revision and a line followed. */
  | { view: "file"; path: string; commit: string | null; mode: FileMode; at: { rev: string; path: string } | null; line: number | null }
  | { view: "stashes"; stash: string | null }
  | { view: "oplog" }
  | { view: "local" }
  | { view: "compare"; base: string; target: string; mode: CompareMode }
  | { view: "series"; query: SeriesQuery }
  /** A pull request: of a branch here, or by its number when it is another account's or a fork's;
   *  `forge` "gitlab": the branch's merge request. */
  | { view: "pull"; branch: string; number?: number | null; forge?: ForgeKind }
  | { view: "reviews" }
  | { view: "stack"; branch: string | null }
  /** Browse Files at This Commit: the commit, the branch or tag it was opened from, the file shown. */
  | { view: "browse"; rev: string; label: string | null; path: string | null; snapshot?: boolean };

export type Trail = { places: Place[]; at: number };

/** How many places are kept; the oldest go first. */
export const LIMIT = 50;

export const emptyTrail = (): Trail => ({ places: [], at: -1 });

/** What makes a place another one: a different screen, file, followed line, branch or range.
 *  The rest (the selected commit, Changes or Blame, the scroll) is how you left it. */
export function placeKey(p: Place): string {
  switch (p.view) {
    case "file":
      return `file\0${p.path}\0${p.line ?? ""}`;
    case "compare":
      return `compare\0${p.base}\0${p.target}`;
    case "series":
      return `series\0${p.query.branch}\0${p.query.old}\0${p.query.new}`;
    case "pull":
      return `pull\0${p.forge ?? "github"}\0${p.number ?? p.branch}`;
    case "stack":
      return `stack\0${p.branch ?? ""}`;
    case "browse":
      return `browse\0${p.rev}`;
    default:
      return p.view;
  }
}

/** Where you are now. `jump` is a leap to another commit (a search result, a parent, a branch
 *  in the sidebar), which History keeps as a place of its own; a click or an arrow key in the
 *  graph only changes how you leave the place. A new place drops the ones ahead of it. */
export function visit(trail: Trail, place: Place, jump = false): Trail {
  const current = trail.places[trail.at];
  if (current && placeKey(current) === placeKey(place)) {
    const leap = jump && place.view === "history" && current.view === "history" && !!current.commit && !!place.commit && current.commit !== place.commit;
    if (!leap) {
      if (same(current, place)) return trail;
      const places = trail.places.slice();
      places[trail.at] = place;
      return { places, at: trail.at };
    }
  }
  const places = [...trail.places.slice(0, trail.at + 1), place].slice(-LIMIT);
  return { places, at: places.length - 1 };
}

function same(a: Place, b: Place): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/** The place you are at, with a change to how you leave it, such as History's scroll. */
export function amend(trail: Trail, change: (p: Place) => Place): Trail {
  const current = trail.places[trail.at];
  if (!current) return trail;
  const places = trail.places.slice();
  places[trail.at] = change(current);
  return { places, at: trail.at };
}

/** A step back or forward, or to any place on the trail. */
export function go(trail: Trail, to: number): Trail | null {
  if (to < 0 || to >= trail.places.length || to === trail.at) return null;
  return { places: trail.places, at: to };
}

export const canBack = (t: Trail) => t.at > 0;
export const canForward = (t: Trail) => t.at < t.places.length - 1;

type NavigationTrail = {
  trail: Trail;
  scroll: HistoryPosition | null;
  jumping: boolean;
  revision: number;
};

export function newNavigationTrail(): NavigationTrail {
  return { trail: emptyTrail(), scroll: null, jumping: false, revision: 0 };
}

export function resetNavigationTrail(state: NavigationTrail) {
  state.trail = emptyTrail();
  state.scroll = null;
  state.jumping = false;
  state.revision++;
}

function saveScroll(state: NavigationTrail): Trail {
  return amend(state.trail, (place) => place.view === "history" ? { ...place, scroll: state.scroll } : place);
}

export function recordPlace(state: NavigationTrail, place: Place) {
  const current = saveScroll(state);
  const before = current.places[current.at];
  const next = visit(current, place.view === "history" ? { ...place, scroll: state.scroll } : place, state.jumping);
  const changedScreen = !before || placeKey(before) !== placeKey(place);
  const changedCommit = before?.view === "history" && place.view === "history" && before.commit !== place.commit;
  if (next.at !== current.at || changedScreen || changedCommit) {
    state.revision++;
  }
  state.trail = next;
  state.jumping = false;
}

export function moveToPlace(
  state: NavigationTrail,
  index: number,
  graph: {
    load: (id: string) => boolean | Promise<boolean>;
    current: () => boolean;
    missing: (id: string) => void;
    show: (place: Place, current: () => boolean) => void;
  },
) {
  const target = state.trail.places[index];
  if (!target || index === state.trail.at) return;
  const commit = target.view === "history" ? target.commit : null;
  const revision = ++state.revision;
  const current = () => revision === state.revision && graph.current();

  function finish(loaded: boolean) {
    if (!current()) return;
    if (!loaded && commit) {
      graph.missing(commit);
      return;
    }
    const next = go(saveScroll(state), index);
    if (!next) return;
    const place = next.places[next.at];
    state.trail = next;
    state.jumping = false;
    if (place.view === "history") state.scroll = place.scroll;
    graph.show(place, () => revision === state.revision);
  }

  const loaded = commit ? graph.load(commit) : true;
  if (typeof loaded === "boolean") finish(loaded);
  else void loaded.then(finish, () => finish(false));
}

export function historyPosition(rows: readonly { id: string }[], top: number, rowHeight: number, padding: number): HistoryPosition | null {
  const index = Math.max(0, Math.floor((top - padding) / rowHeight));
  const row = rows[index];
  return row ? { id: row.id, offset: padding + index * rowHeight - top } : null;
}

export function historyScrollTop(position: HistoryPosition, index: number | undefined, rowHeight: number, padding: number): number | null {
  return index === undefined ? null : Math.max(0, padding + index * rowHeight - Math.max(1 - rowHeight, position.offset));
}

/** The places that led here, oldest first, for the path in the header: a run of places with the
 *  same name shows once, by its latest one, and only the last `count` are kept. */
export function crumbs(trail: Trail, label: (p: Place) => string, count = 3): { label: string; at: number }[] {
  const out: { label: string; at: number }[] = [];
  const here = trail.places[trail.at] ? label(trail.places[trail.at]) : "";
  for (let i = 0; i < trail.at; i++) {
    const name = label(trail.places[i]);
    if (out.length && out[out.length - 1].label === name) out[out.length - 1].at = i;
    else out.push({ label: name, at: i });
  }
  // The place just before is the same screen: the path names it once, as where you are.
  if (out.length && out[out.length - 1].label === here) out.pop();
  return out.slice(-count);
}

/** What a place is called, in the header's path and the list of places. */
export function nameOfPlace(p: Place, pullOf: (branch: string, forge: ForgeKind) => { number: number } | null | undefined): string {
  switch (p.view) {
    case "history":
      return "History";
    case "file":
      return p.line ? `Line ${p.line}` : p.path.slice(p.path.lastIndexOf("/") + 1);
    case "stashes":
      return "Stashes";
    case "oplog":
      return "Operation Log";
    case "local":
      return "Local History";
    case "compare":
      return "Compare";
    case "series":
      return "Compare Series";
    case "pull": {
      const forge = p.forge ?? "github";
      const number = p.number ?? pullOf(p.branch, forge)?.number;
      if (number != null) return `${forge === "gitlab" ? "MR" : "PR"} ${wordsOf(forge).mark}${number}`;
      return `${forge === "gitlab" ? "Merge Request" : "Pull Request"} ${p.branch}`;
    }
    case "reviews":
      return "Reviews";
    case "stack":
      return "Edit Stack";
    case "browse":
      return p.label ? `Files of ${p.label}` : `Files at ${p.rev.slice(0, 7)}`;
  }
}

/** The grey line under a place's name in the list of places. `rowOf` finds a commit of the
 *  history; an Edit Stack of no branch in particular is the checked-out one's, `headBranch`. */
export function detailOfPlace(
  p: Place,
  rowOf: (id: string) => { summary: string; worktree?: unknown } | undefined,
  headBranch: string | null,
): string | undefined {
  switch (p.view) {
    case "history": {
      const row = p.commit ? rowOf(p.commit) : undefined;
      return row ? (row.worktree ? "Uncommitted changes" : row.summary) : undefined;
    }
    case "file":
      return p.line ? `${p.path} · Blame` : `${p.path} · ${p.mode === "blame" ? "Blame" : "Changes"}`;
    case "compare":
      return `${compareLabel(p.target)} with ${compareLabel(p.base)}`;
    case "series":
      return p.query.branch;
    case "pull":
      return p.branch;
    case "stack":
      return p.branch ?? headBranch ?? undefined;
    case "browse":
      return p.path ?? rowOf(p.rev)?.summary;
    default:
      return undefined;
  }
}
