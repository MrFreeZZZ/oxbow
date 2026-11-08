import assert from "node:assert/strict";
import { test } from "node:test";
import { api } from "../src/lib/api.ts";
import type { BranchContext } from "../src/lib/branches.ts";
import { mergeRequest } from "../src/lib/merge.ts";
import type { MergePreview } from "../src/lib/types.ts";

test("merge sends the displayed default and edited messages to Git", async (t) => {
  const branch = { kind: "local", name: "feature", target: "tip" } as const;
  const ctx = {
    history: { refs: [branch], rows: [], head: { branch: "main" }, protected: [] },
    colorOf: () => 0,
  } as unknown as BranchContext;
  const preview = {
    branch: branch.name, incoming: [], incomingCount: 1, ours: [], oursCount: 0,
    base: null, conflicts: [], files: 1, touchedHere: 0,
  } satisfies MergePreview;
  const fetch = t.mock.method(api, "mergePreview", async () => preview);
  const request = await mergeRequest(ctx, branch);
  assert.deepEqual(fetch.mock.calls[0].arguments, ["refs/heads/feature"]);
  const message = request.fields?.find((field) => field.label === "Message")?.text;
  assert.ok(message);
  assert.equal(message.value, "Merge branch 'feature' into main");
  assert.deepEqual(request.action, {
    kind: "merge", branch: "refs/heads/feature", method: "merge", message: message.value,
  });
  const custom = "Bring in feature\n\nKeep its history.";
  const edited = await message.edit(custom);
  assert.deepEqual(edited.action, {
    kind: "merge", branch: "refs/heads/feature", method: "merge", message: custom,
  });
});
