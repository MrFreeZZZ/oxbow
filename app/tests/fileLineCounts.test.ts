import assert from "node:assert/strict";
import test from "node:test";
import { fileDeletionWidth } from "../src/lib/format.ts";

test("file counts leave space for the minus sign and the widest deletion count", () => {
  assert.equal(fileDeletionWidth([null, { deletions: 0 }]), "2ch");
  assert.equal(fileDeletionWidth([{ deletions: 9 }, { deletions: 1234 }]), "5ch");
  assert.equal(fileDeletionWidth([{ deletions: 999 }], 1000), "5ch");
});
