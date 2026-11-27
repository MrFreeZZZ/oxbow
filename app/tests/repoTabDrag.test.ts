import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { compileModule } from "svelte/compiler";
import * as runtime from "svelte/internal/client";
import ts from "typescript";
import { DRAG_THRESHOLD, draggedTabLeft, pastDragThreshold, tabDragOffsets, tabDropIndex, tabParkedLefts, type TabSlot } from "../src/lib/repoTabDrag.ts";

/** Equal Chrome-sized tabs: 256px wide, 18px of overlap, so each starts 238px after the previous. */
function pitch(count: number): TabSlot[] {
  const step = 256 - 18;
  return Array.from({ length: count }, (_, index) => ({ left: index * step, width: 256 }));
}

test("a drag starts only after Chromium's 8px slop on either axis", () => {
  assert.equal(DRAG_THRESHOLD, 8);
  assert.equal(pastDragThreshold(8, 0), false);
  assert.equal(pastDragThreshold(0, -8), false);
  assert.equal(pastDragThreshold(9, 0), true);
  assert.equal(pastDragThreshold(0, -9), true);
});

test("the dragged tab lands once its center passes another tab's center", () => {
  const slots = pitch(4);
  assert.equal(tabDropIndex(128, slots, 0), 0);
  assert.equal(tabDropIndex(366, slots, 0), 0);
  assert.equal(tabDropIndex(367, slots, 0), 1);
  assert.equal(tabDropIndex(843, slots, 0), 3);
  assert.equal(tabDropIndex(128, slots, 3), 0);
  assert.equal(tabDropIndex(400, slots, 3), 2);
  assert.equal(tabDropIndex(842, slots, 3), 3);
});

test("other tabs slide into the gap while the dragged tab follows the pointer", () => {
  const slots = pitch(3);
  assert.deepEqual(tabParkedLefts(slots, 0, 1, 18), [238, 0, 476]);
  assert.deepEqual(tabDragOffsets(slots, 0, 1, 200, 18), [200, -238, 0]);
  const uneven: TabSlot[] = [
    { left: 0, width: 100 },
    { left: 82, width: 80 },
    { left: 144, width: 80 },
  ];
  assert.deepEqual(tabParkedLefts(uneven, 2, 0, 18), [62, 144, 0]);
});

test("the dragged tab stays inside the strip", () => {
  assert.equal(draggedTabLeft(10, 40, 256, 732), 0);
  assert.equal(draggedTabLeft(400, 20, 256, 732), 380);
  assert.equal(draggedTabLeft(900, 20, 256, 732), 476);
});

test("switching tabs does not swallow the next independent click", async () => {
  const source = readFileSync(new URL("../src/lib/repoTabDrag.svelte.ts", import.meta.url), "utf8");
  const js = ts.transpile(source, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext });
  const compiled = compileModule(js, { generate: "client" }).js.code.replace(/^import [^;]+;$/gm, "").replace(/^export /gm, "");
  const window = new EventTarget();
  const begin = new Function("$", "window", compiled + "\nreturn beginTabDrag;")(runtime, window);
  const pointer = (type: string) => Object.assign(new Event(type), { button: 0, pointerId: 1, clientX: 0, clientY: 0 });
  const click = (detail: number) => window.dispatchEvent(Object.assign(new Event("click", { cancelable: true }), { detail }));
  const press = async () => {
    begin(pointer("pointerdown"), 2, [1, 2], {
      canSwitch: () => true,
      activate: async () => {},
      isCurrent: () => true,
      move: async () => {},
    });
    await Promise.resolve();
    window.dispatchEvent(pointer("pointerup"));
  };

  await press();
  // The old tab strip disappeared between down and up, so WebKit emitted no click.
  window.dispatchEvent(pointer("pointerdown"));
  assert.equal(click(1), true);

  await press();
  assert.equal(click(0), true, "a keyboard click is independent of the pointer press");
  assert.equal(click(1), false, "the click from the tab gesture itself is still suppressed");
  assert.equal(click(1), true, "the guard clears after the gesture click");
});
