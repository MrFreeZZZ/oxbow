import { filesBrief, filterWords, stashMatches, type StashFacts } from "../src/lib/stashFilter.ts";
import assert from "node:assert/strict";

const pool: StashFacts = {
  name: "stash@{1}",
  title: "Try a smaller pool",
  branch: "auth/3-ui",
  base: "0d4a9b6e1f2c3d4e5f60718293a4b5c6d7e8f901",
  paths: ["src/db/pool.rs", "src/config.toml", "README.md", "web/login.ts"],
};

// Every word must be found, anywhere: title, branch, name, files.
assert.equal(stashMatches(pool, filterWords("")), true);
assert.equal(stashMatches(pool, filterWords("smaller")), true);
assert.equal(stashMatches(pool, filterWords("AUTH pool")), true);
assert.equal(stashMatches(pool, filterWords("stash@{1}")), true);
assert.equal(stashMatches(pool, filterWords("login.ts")), true);
assert.equal(stashMatches(pool, filterWords("pool main")), false);
// The base commit only from its start, as a short SHA is typed.
assert.equal(stashMatches(pool, filterWords("0d4a9b6")), true);
assert.equal(stashMatches(pool, filterWords("9b6e1f")), false);
// Files not loaded yet: the rest still matches.
assert.equal(stashMatches({ ...pool, paths: null }, filterWords("pool")), true);
assert.equal(stashMatches({ ...pool, paths: null }, filterWords("login")), false);
// A detached HEAD has no branch.
assert.equal(stashMatches({ ...pool, branch: null }, filterWords("auth")), false);

// Three names, the rest counted.
assert.deepEqual(filesBrief(pool.paths!, []), { names: ["pool.rs", "config.toml", "README.md"], more: 1 });
assert.deepEqual(filesBrief(["a.rs"], []), { names: ["a.rs"], more: 0 });
assert.deepEqual(filesBrief([], []), { names: [], more: 0 });
// The files the filter found come first.
assert.deepEqual(filesBrief(pool.paths!, filterWords("login")), { names: ["login.ts", "pool.rs", "config.toml"], more: 1 });
