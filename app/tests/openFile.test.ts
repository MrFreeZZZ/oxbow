import assert from "node:assert/strict";
import { test } from "node:test";
import { workingFileAction } from "../src/lib/openFile.ts";

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
