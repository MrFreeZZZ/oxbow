import { prefixWords, splitPrefix, typedAfter, withPrefix } from "../src/lib/branchPrefixes.ts";
import assert from "node:assert/strict";

// A known prefix comes off the front; any other start stays in the name.
{
  assert.deepEqual(splitPrefix("bugfix/login-timeout"), { prefix: "bugfix/", rest: "login-timeout" });
  assert.deepEqual(splitPrefix("feature/"), { prefix: "feature/", rest: "" });
  assert.deepEqual(splitPrefix("pr/12"), { prefix: null, rest: "pr/12" });
  assert.deepEqual(splitPrefix(""), { prefix: null, rest: "" });
}

// Picking a prefix keeps what was typed after the old one; No prefix takes it off.
{
  assert.equal(withPrefix("feature/csv-export", "hotfix/"), "hotfix/csv-export");
  assert.equal(withPrefix("csv-export", "docs/"), "docs/csv-export");
  assert.equal(withPrefix("feature/csv-export", null), "csv-export");
}

// A known prefix typed or pasted after the pop-up takes its place; anything else follows it.
{
  assert.equal(typedAfter("feature/", "csv"), "feature/csv");
  assert.equal(typedAfter("feature/", "bugfix/login"), "bugfix/login");
  assert.equal(typedAfter(null, "pr/12"), "pr/12");
  assert.equal(typedAfter("feature/", ""), "feature/");
}

// The words under the box say what the prefix is for.
{
  assert.equal(prefixWords("bugfix/x"), "Fixing a bug");
  assert.equal(prefixWords("x"), null);
}
