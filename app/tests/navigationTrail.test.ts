import assert from "node:assert/strict";
import { test } from "node:test";
import {
  historyPosition, historyScrollTop, moveToPlace, newNavigationTrail, recordPlace, resetNavigationTrail,
  type HistoryPosition, type Place,
} from "../src/lib/backStack.ts";

const history = (commit: string, scroll: HistoryPosition | null = null): Place => ({ view: "history", commit, scroll });
const file: Place = { view: "file", path: "a.txt", commit: "visible", mode: "changes", at: null, line: null };

function navigation() {
  const state = newNavigationTrail();
  const warnings: string[] = [];
  const frames: (() => boolean)[] = [];
  let shown: Place | null = null;
  const graph = {
    load: (_id: string): boolean | Promise<boolean> => true,
    current: () => true,
    missing: (id: string) => { warnings.push(id); },
    show: (place: Place, current: () => boolean) => {
      shown = place;
      frames.push(current);
      recordPlace(state, place);
    },
  };
  return { state, warnings, frames, graph, shown: () => shown };
}

test("Back and Forward keep each History stop's top row and pixel offset", () => {
  const { state, graph, shown } = navigation();
  const rows = ["head", "visible", "selected", "old"].map((id) => ({ id }));
  recordPlace(state, history("selected"));
  state.scroll = historyPosition(rows, 54, 44, 4);
  recordPlace(state, file);
  recordPlace(state, history("head"));
  state.scroll = historyPosition(rows, 0, 44, 4);

  const before = state.scroll;
  moveToPlace(state, 0, graph);
  const place = shown();
  assert.deepEqual(place, history("selected", { id: "visible", offset: -6 }));
  assert.ok(place?.view === "history" && place.scroll);
  assert.equal(historyScrollTop(place.scroll, 1, 44, 4), 54);
  const updated = [{ id: "newer" }, { id: "new" }, ...rows];
  const index = updated.findIndex((r) => r.id === place.scroll?.id);
  assert.equal(historyScrollTop(place.scroll, index, 44, 4), 142);
  assert.deepEqual(historyPosition(updated, 142, 44, 4), place.scroll);
  assert.equal(historyScrollTop(place.scroll, undefined, 44, 4), null);
  moveToPlace(state, 2, graph);
  assert.deepEqual(shown(), history("head", before));
});

for (const direction of ["Back", "Forward"]) {
  test(`${direction} to a commit removed by a rewrite keeps the current screen and trail`, async () => {
    const { state, graph, shown, warnings } = navigation();
    if (direction === "Back") {
      recordPlace(state, history("rewritten"));
      recordPlace(state, file);
    } else {
      recordPlace(state, file);
      recordPlace(state, history("rewritten"));
      moveToPlace(state, 0, graph);
    }
    const trail = state.trail;
    const screen = shown();
    moveToPlace(state, direction === "Back" ? 0 : 1, {
      ...graph,
      load: async () => false,
    });
    assert.equal(state.trail, trail);
    await Promise.resolve();
    assert.equal(state.trail, trail);
    assert.equal(shown(), screen);
    assert.deepEqual(warnings, ["rewritten"]);
  });
}

test("the current History stop is saved at the moment the destination finishes loading", async () => {
  const { state, graph, shown } = navigation();
  recordPlace(state, history("older"));
  recordPlace(state, file);
  recordPlace(state, history("newer"));
  let finish!: (loaded: boolean) => void;
  const load = new Promise<boolean>((resolve) => { finish = resolve; });
  moveToPlace(state, 0, { ...graph, load: () => load });
  assert.equal(state.trail.at, 2);
  assert.equal(shown(), null);
  state.scroll = { id: "latest-visible", offset: -12 };
  finish(true);
  await load;
  assert.equal(state.trail.at, 0);
  assert.deepEqual(shown(), history("older"));
  assert.deepEqual(state.trail.places[2], history("newer", { id: "latest-visible", offset: -12 }));
});

test("a delayed load cannot replace a newer transition or warn after a repository switch", async () => {
  const { state, graph, shown, warnings, frames } = navigation();
  recordPlace(state, history("older"));
  recordPlace(state, file);
  recordPlace(state, history("newer"));
  let finish!: (loaded: boolean) => void;
  const load = new Promise<boolean>((resolve) => { finish = resolve; });
  moveToPlace(state, 0, { ...graph, load: () => load });
  assert.equal(state.trail.at, 2);
  moveToPlace(state, 1, graph);
  finish(true);
  await load;
  assert.equal(state.trail.at, 1);
  assert.deepEqual(shown(), file);

  moveToPlace(state, 0, graph);
  assert.equal(frames.at(-1)?.(), true);
  moveToPlace(state, 2, { ...graph, load: async () => false });
  resetNavigationTrail(state);
  recordPlace(state, history("older"));
  await Promise.resolve();
  assert.deepEqual(warnings, []);
  assert.equal(frames.at(-1)?.(), false);
  assert.deepEqual(state.trail, { at: 0, places: [history("older")] });
});
