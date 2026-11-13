import assert from "node:assert/strict";
import { test } from "node:test";
import { diffLineEndingLabel } from "../src/lib/diffLineEndings.ts";

test("the diff badge distinguishes a known empty version from an unknown format", () => {
  for (const status of ["modified", "renamed"] as const) {
    const file = { status };
    assert.equal(diffLineEndingLabel({ file, lineEndings: ["LF", "Empty"] }), "LF → Empty");
    assert.equal(diffLineEndingLabel({ file, lineEndings: ["Empty", "LF"] }), "Empty → LF");
    assert.equal(diffLineEndingLabel({ file, lineEndings: ["LF", null] }), null);
    assert.equal(diffLineEndingLabel({ file, lineEndings: [null, "LF"] }), null);
  }
  assert.equal(diffLineEndingLabel({ file: { status: "added" }, lineEndings: [null, "Empty"] }), null);
  assert.equal(diffLineEndingLabel({ file: { status: "modified" } }), null);
});
