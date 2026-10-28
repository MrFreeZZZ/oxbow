import { languageOf } from "../src/lib/syntax.ts";
import assert from "node:assert/strict";
import test from "node:test";

test("a language's colors load the first time a file in it is shown", async () => {
  assert.equal(languageOf("notes.txt"), null);
  // Not loaded yet: plain text for now, and the load starts.
  assert.equal(languageOf("src/main.rs"), null);
  for (let i = 0; i < 100 && !languageOf("src/main.rs"); i++) await new Promise((done) => setTimeout(done, 10));
  assert.equal(languageOf("src/lib.rs"), "rust");
  assert.equal(languageOf("Dockerfile"), null);
});
