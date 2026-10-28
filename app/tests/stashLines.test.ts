import { quoted, stashLinesRequest, stashStagedRequest } from "../src/lib/stashLines.ts";
import type { Hunk } from "../src/lib/types.ts";
import assert from "node:assert/strict";
import { test } from "node:test";

const line = (kind: "context" | "added" | "removed", text: string) => ({ kind, oldLine: null, newLine: null, text, words: null });
const hunk = {
  header: "@@ -8,3 +8,3 @@",
  check: "c0ffee",
  lines: [line("context", "n8"), line("removed", "n9"), line("removed", "n10"), line("added", "N9 stays"), line("added", "N10 picked")],
} as unknown as Hunk;

test("Stash Staged stashes only what is staged, with the message typed", () => {
  const sheet = stashStagedRequest(2, " WIP: banner ");
  assert.equal(sheet.title, "Stash 2 staged files?");
  assert.equal(sheet.button, "Stash Staged");
  assert.deepEqual(sheet.action, { kind: "stashPush", message: "WIP: banner", untracked: false, staged: true });
  assert.equal(sheet.invalid, null);
  assert.equal(stashStagedRequest(0, "").invalid, "Nothing is staged.");
  // The message box makes the sheet anew, through the sheet that holds it when there is one.
  const typed = sheet.fields?.[0].text?.edit("Next");
  assert.deepEqual((typed as { action: unknown }).action, { kind: "stashPush", message: "Next", untracked: false, staged: true });
  let asked = "";
  stashStagedRequest(1, "", (m) => ((asked = m), stashStagedRequest(1, m))).fields?.[0].text?.edit("Own");
  assert.equal(asked, "Own");
});

test("a hunk or its picked lines go into a stash", () => {
  const whole = stashLinesRequest("src/a.txt", hunk, null, "");
  assert.equal(whole.title, "Stash this hunk of a.txt?");
  assert.deepEqual(whole.action, { kind: "stashHunk", path: "src/a.txt", header: "@@ -8,3 +8,3 @@", check: "c0ffee", lines: null, message: null });

  const picked = stashLinesRequest("src/a.txt", hunk, [4, 2], "Bottom line");
  assert.equal(picked.title, "Stash 2 lines of a.txt?");
  assert.deepEqual(picked.action, { kind: "stashHunk", path: "src/a.txt", header: "@@ -8,3 +8,3 @@", check: "c0ffee", lines: [2, 4], message: "Bottom line" });
  assert.ok(picked.body.some((p) => typeof p === "object" && "quote" in p && p.quote === "N10 picked"));
  assert.equal(picked.done, "Stashed 2 lines of a.txt as “Bottom line”.");
});

test("picked lines are quoted three at most", () => {
  assert.deepEqual(quoted(["a", " b "]), [{ quote: "a" }, " and ", { quote: "b" }]);
  assert.deepEqual(quoted(["a", "b", "c", "d"]).at(-1), " and 1 more");
  assert.deepEqual(quoted(["x".repeat(60)]), [{ quote: `${"x".repeat(47)}…` }]);
});
