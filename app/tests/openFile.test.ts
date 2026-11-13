import assert from "node:assert/strict";
import { test } from "node:test";
import {
  keepWorkingFileStatuses, loadWorkingFileStatuses, workingFileAction, type WorkingFileStatus,
} from "../src/lib/openFile.ts";

test("an unresolved working copy offers no app, and an absent copy gives its reason", () => {
  assert.deepEqual(workingFileAction(undefined, false), {
    label: "Open Working Copy", reason: "Checking the file on disk…",
  });
  const reason = "The working copy of photo.png is missing from disk.";
  assert.deepEqual(workingFileAction({ kind: "unavailable", reason }, true), {
    label: "Open Working Copy", reason,
  });
});

test("historical actions name the app for the working copy and identify the version opened", () => {
  assert.deepEqual(workingFileAction({ kind: "text" }, true), {
    label: "Open in Editor (Working Copy)", reason: undefined,
  });
  assert.deepEqual(workingFileAction({ kind: "binary" }, true), {
    label: "Open in Default App (Working Copy)", reason: undefined,
  });
});

test("the next page retries a failed batch without rechecking known paths", async () => {
  const unavailable = { kind: "unavailable", reason: "File missing" } as const;
  const known = new Map<string, WorkingFileStatus>([["ready", { kind: "text" }], ["absent", unavailable]]);
  const calls: string[][] = [];
  const read = async (paths: string[]): Promise<WorkingFileStatus[]> => {
    calls.push(paths);
    if (calls.length === 1) throw new Error("Temporary request failure");
    return paths.map(() => ({ kind: "binary" }));
  };
  await assert.rejects(loadWorkingFileStatuses(["ready", "absent", "retry"], known, read), /Temporary request failure/);
  assert.deepEqual(calls, [["retry"]]);
  assert.equal(known.has("retry"), false);
  const next = await loadWorkingFileStatuses(["ready", "absent", "retry", "new"], known, read);
  assert.deepEqual(calls, [["retry"], ["retry", "new"]]);
  assert.equal(next.get("absent"), unavailable);
  assert.deepEqual(next.get("retry"), { kind: "binary" });
  assert.deepEqual(next.get("new"), { kind: "binary" });
});

test("a change on disk rechecks every file but keeps the last statuses until the answer", async () => {
  const last = new Map<string, WorkingFileStatus>([["a.png", { kind: "binary" }], ["b.txt", { kind: "text" }]]);
  const files = new Set(["a.png", "b.txt"]);
  const kept = keepWorkingFileStatuses({ shown: last, checked: last }, files, "disk");
  assert.deepEqual(kept.shown, last);
  assert.equal(kept.checked.size, 0);
  const calls: string[][] = [];
  const next = await loadWorkingFileStatuses(files, kept.checked, async (paths) => {
    calls.push(paths);
    return paths.map(() => ({ kind: "text" }));
  });
  assert.deepEqual(calls, [["a.png", "b.txt"]]);
  assert.deepEqual(next.get("a.png"), { kind: "text" });
});

test("a smaller list keeps what it still shows, and another repository shows nothing", () => {
  const last = new Map<string, WorkingFileStatus>([["a.png", { kind: "binary" }], ["b.txt", { kind: "text" }]]);
  const files = new Set(["b.txt"]);
  const kept = keepWorkingFileStatuses({ shown: last, checked: last }, files, "none");
  assert.deepEqual([...kept.shown.keys()], ["b.txt"]);
  assert.deepEqual([...kept.checked.keys()], ["b.txt"]);
  const other = keepWorkingFileStatuses({ shown: last, checked: last }, files, "repo");
  assert.equal(other.shown.size, 0);
  assert.equal(other.checked.size, 0);
});

test("a page loaded during a recheck asks again for the files only shown from before", () => {
  const stale = new Map<string, WorkingFileStatus>([["a.png", { kind: "binary" }]]);
  const fresh = new Map<string, WorkingFileStatus>();
  const kept = keepWorkingFileStatuses({ shown: stale, checked: fresh }, new Set(["a.png", "c.txt"]), "none");
  assert.deepEqual(kept.shown, stale);
  assert.equal(kept.checked.size, 0);
});
