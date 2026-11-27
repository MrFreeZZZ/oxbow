import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import { autoFetchScheduled, repoWorkOnFocus } from "../src/lib/tabWork.ts";

test("focus reloads and auto-fetch run only for the repository on screen", () => {
  assert.equal(repoWorkOnFocus(false), false);
  assert.equal(repoWorkOnFocus(true), true);
  assert.equal(autoFetchScheduled(false, true, true), false);
  assert.equal(autoFetchScheduled(true, false, true), false);
  assert.equal(autoFetchScheduled(true, true, false), false);
  assert.equal(autoFetchScheduled(true, true, true), true);
});

test("the window asks git on focus only through the active-repository check", () => {
  const app = fs.readFileSync(new URL("../src/App.svelte", import.meta.url), "utf8");
  const files = fs.readFileSync(new URL("../src/lib/openFile.svelte.ts", import.meta.url), "utf8");
  assert.match(app, /autoFetchScheduled\(scope\.isActive\(\)/);
  assert.match(app, /repoWorkOnFocus\(scope\.isActive\(\)\)/);
  assert.match(files, /repoWorkOnFocus\(scope\.isActive\(\)\)/);
  assert.doesNotMatch(app, /onfocus=\{\(\) => loader\.cameBack\(\)\}/);
});
