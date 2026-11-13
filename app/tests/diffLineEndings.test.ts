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

test("a one-sided version shows its own format, and an unchanged format shows once", () => {
  assert.equal(diffLineEndingLabel({ file: { status: "deleted" }, lineEndings: ["CRLF", null] }), "CRLF");
  assert.equal(diffLineEndingLabel({ file: { status: "deleted" }, lineEndings: ["Empty", null] }), null);
  for (const status of ["added", "untracked", "copied"] as const) {
    assert.equal(diffLineEndingLabel({ file: { status }, lineEndings: ["LF", "CR"] }), "CR");
  }
  assert.equal(diffLineEndingLabel({ file: { status: "untracked" }, lineEndings: [null, "No EOL"] }), "No EOL");
  assert.equal(diffLineEndingLabel({ file: { status: "modified" }, lineEndings: ["CR", "CR"] }), "CR");
  assert.equal(diffLineEndingLabel({ file: { status: "modified" }, lineEndings: ["Empty", "Empty"] }), null);
  assert.equal(diffLineEndingLabel({ file: { status: "modified" }, lineEndings: ["No EOL", "Mixed"] }), "No EOL → Mixed");
});
