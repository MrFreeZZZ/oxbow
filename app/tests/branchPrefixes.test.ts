import { BRANCH_PREFIXES, cleanName, onlyPrefix, prefixProblem, prefixesProblem, repoPrefixes, similarBranch, splitPrefix, typedAfter, withPrefix } from "../src/lib/branchPrefixes.ts";
import assert from "node:assert/strict";

// A known prefix comes off the front; any other start stays in the name.
{
  assert.deepEqual(splitPrefix("bugfix/login-timeout"), { prefix: "bugfix/", rest: "login-timeout" });
  assert.deepEqual(splitPrefix("feature/"), { prefix: "feature/", rest: "" });
  assert.deepEqual(splitPrefix("pr/12"), { prefix: null, rest: "pr/12" });
  assert.deepEqual(splitPrefix(""), { prefix: null, rest: "" });
  // A list of one's own knows only its prefixes.
  const own = [{ prefix: "spike/", words: "Trying an idea out" }];
  assert.deepEqual(splitPrefix("spike/cache", own), { prefix: "spike/", rest: "cache" });
  assert.deepEqual(splitPrefix("feature/x", own), { prefix: null, rest: "feature/x" });
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

// A prefix alone is no name yet.
{
  assert.ok(onlyPrefix(""));
  assert.ok(onlyPrefix("feature/"));
  assert.ok(onlyPrefix("team/feature/"));
  assert.ok(!onlyPrefix("feature/x"));
  assert.ok(!onlyPrefix("main"));
}

// Typed names become names git takes: dashes for spaces, refused characters gone, lowercase on request.
{
  assert.equal(cleanName("CSV Export", true), "csv-export");
  assert.equal(cleanName("CSV Export", false), "CSV-Export");
  assert.equal(cleanName("fix: login~bug?", true), "fix-loginbug");
  assert.equal(cleanName("a..b//c @{x}", false), "a.b/c-@x}");
  assert.equal(cleanName("payout-", true), "payout-");
  assert.equal(cleanName(" -lead", false), "lead");
}

// The repository's own prefixes, the most used first, without those of the list.
{
  const names = ["auth/1-models", "auth/2-api", "auth/3-ui", "renovate/serde", "feature/x", "main", "auth/2-api"];
  assert.deepEqual(repoPrefixes(names, BRANCH_PREFIXES), [
    { prefix: "auth/", words: "3 branches here", repo: true },
    { prefix: "renovate/", words: "1 branch here", repo: true },
  ]);
}

// Almost the same name: other capitals or dashes, one longer by a little, or a letter apart.
{
  const names = ["hotfix/payout-rounding-v2", "feature/csv-export", "main"];
  assert.equal(similarBranch("hotfix/payout-rounding", names), "hotfix/payout-rounding-v2");
  assert.equal(similarBranch("bugfix/CSV_Export", names), "feature/csv-export");
  assert.equal(similarBranch("feature/csv-exprot", names), "feature/csv-export");
  assert.equal(similarBranch("feature/csv-export", names), null);
  assert.equal(similarBranch("feature/login", names), null);
  assert.equal(similarBranch("fix/ma", names), null);
}

// settings.json takes a list of prefixes, each a word and a slash.
{
  assert.equal(prefixesProblem(BRANCH_PREFIXES), null);
  assert.match(prefixesProblem([{ prefix: "feature", words: "x" }]) ?? "", /not a prefix/);
  assert.match(prefixesProblem([{ prefix: "a/" }]) ?? "", /Expected items/);
  assert.match(prefixesProblem([BRANCH_PREFIXES[0], BRANCH_PREFIXES[0]]) ?? "", /twice/);
}

// The Settings sheet and settings.json take the same prefixes: words git can start a name with.
{
  assert.equal(prefixProblem("team/feature/"), null);
  for (const bad of ["feature", "/x/", "a//", ".wip/", "a b/", "x..y/", "fix:/"]) assert.ok(prefixProblem(bad), bad);
}
