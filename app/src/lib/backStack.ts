// Back and Forward, as in a browser or VS Code: the places visited, each a screen with what was
// picked on it, so a step back returns to where you were rather than to the commit graph.

import type { FileMode } from "./nav.svelte";
import type { SeriesQuery } from "./series";
import type { CompareMode } from "./types";

/** A screen and what was picked on it. */
export type Place =
  /** History: the selected commit, and how far the graph was scrolled when you left it. */
  | { view: "history"; commit: string | null; scroll: number | null }
  /** File History: the file, its selected commit, Changes or Blame, Blame's revision and a line followed. */
  | { view: "file"; path: string; commit: string | null; mode: FileMode; at: { rev: string; path: string } | null; line: number | null }
  | { view: "stashes"; stash: string | null }
  | { view: "oplog" }
  | { view: "compare"; base: string; target: string; mode: CompareMode }
  | { view: "series"; query: SeriesQuery }
  | { view: "pull"; branch: string }
  | { view: "stack"; branch: string | null };

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
      return `pull\0${p.branch}`;
    case "stack":
      return `stack\0${p.branch ?? ""}`;
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
