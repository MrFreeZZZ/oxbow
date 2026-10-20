import { nextTab } from "../src/lib/tabs.ts";
import assert from "node:assert/strict";

// Tabs step by the whole list: from Summary back to the last file, even of hundreds, and round.
for (const count of [1, 3, 101, 200]) {
  const paths = Array.from({ length: count }, (_, i) => `file-${i}`);
  const last = paths[count - 1];
  assert.equal(nextTab(paths, null, -1), last);
  assert.equal(nextTab(paths, last, 1), null);
  assert.equal(nextTab(paths, null, 1), "file-0");
  assert.equal(nextTab(paths, "file-0", -1), null);
  // The arrow keys stop at the ends.
  assert.equal(nextTab(paths, null, -1, false), undefined);
  assert.equal(nextTab(paths, last, 1, false), undefined);
}
assert.equal(nextTab(["a", "b", "c"], "b", 1), "c");
assert.equal(nextTab([], null, 1), null);
