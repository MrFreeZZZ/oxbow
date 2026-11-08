import assert from "node:assert/strict";
import { test } from "node:test";
import {
  moveToPlace, newNavigationTrail, recordPlace, resetNavigationTrail, restoreHistoryPosition, type Place,
} from "../src/lib/backStack.ts";

const history = (commit: string, scroll: number | null = null): Extract<Place, { view: "history" }> =>
  ({ view: "history", commit, scroll });

test("Back and Forward restore the commit and scroll saved at each History stop", async () => {
  const state = newNavigationTrail();
  recordPlace(state, history("older"));
  state.scroll = 200;
  state.jumping = true;
  recordPlace(state, history("newer"));
  state.scroll = 900;

  const older = moveToPlace(state, 0, true);
  assert.deepEqual(older, history("older", 200));
  if (older?.view !== "history") throw new Error("Expected a History stop");
  recordPlace(state, older);

  const scrolled: number[] = [];
  await restoreHistoryPosition(state, older, older.commit, {
    loaded: () => true,
    load: async () => assert.fail("The commit is already loaded"),
    frame: (restore) => restore(),
    current: () => true,
    scrollTo: (top) => {
      scrolled.push(top);
      state.scroll = top;
    },
    reveal: () => assert.fail("The saved scroll takes precedence"),
  });
  assert.deepEqual(scrolled, [200]);
  assert.deepEqual(moveToPlace(state, 1, true), history("newer", 900));
  assert.deepEqual(moveToPlace(state, 0, false), history("older", 200));
});

for (const cancelled of ["load failed", "frame expired"]) {
  test(`History keeps the actual scroll when restoration is cancelled: ${cancelled}`, async () => {
    const state = newNavigationTrail();
    recordPlace(state, history("older"));
    state.scroll = 200;
    state.jumping = true;
    recordPlace(state, history("newer"));
    state.scroll = 900;

    const place = moveToPlace(state, 0, true);
    if (place?.view !== "history") throw new Error("Expected a History stop");
    const frames: (() => void)[] = [];
    let current = true;
    await restoreHistoryPosition(state, place, place.commit, {
      loaded: () => false,
      load: async () => cancelled !== "load failed",
      frame: (restore) => { frames.push(restore); },
      current: () => current,
      scrollTo: () => assert.fail("A cancelled restore must not scroll"),
      reveal: () => assert.fail("A cancelled restore must not reveal"),
    });

    // Picking a row already in view changes the selection without producing a scroll event.
    current = false;
    recordPlace(state, history("visible"));
    for (const frame of frames) frame();
    assert.equal(state.scroll, 900);
    assert.deepEqual(moveToPlace(state, 1, true), history("newer", 900));
    assert.deepEqual(moveToPlace(state, 0, true), history("visible", 900));
  });
}

test("switching repositories clears the trail and cancels pending History restoration", async () => {
  const state = newNavigationTrail();
  recordPlace(state, history("shared-commit"));
  recordPlace(state, { view: "stashes", stash: null });
  const place = moveToPlace(state, 0, false);
  if (place?.view !== "history") throw new Error("Expected a History stop");
  let finishLoad!: (loaded: boolean) => void;
  const load = new Promise<boolean>((resolve) => { finishLoad = resolve; });
  const frames: (() => void)[] = [];
  const restored: string[] = [];
  const graph = {
    loaded: () => false,
    load: () => load,
    frame: (restore: () => void) => { frames.push(restore); },
    current: () => true,
    scrollTo: () => assert.fail("There is no saved scroll"),
    reveal: (id: string) => restored.push(id),
  };
  const pendingLoad = restoreHistoryPosition(state, place, place.commit, graph);
  await restoreHistoryPosition(state, place, place.commit, { ...graph, loaded: () => true });
  state.jumping = true;
  resetNavigationTrail(state);
  recordPlace(state, history("shared-commit"));

  finishLoad(true);
  await pendingLoad;
  for (const frame of frames) frame();
  assert.deepEqual(restored, []);
  assert.deepEqual(state.trail, { at: 0, places: [history("shared-commit")] });
  assert.equal(state.jumping, false);
});
