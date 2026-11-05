// Exercise the actual preference store with Svelte's real derived values and effects.
import assert from "node:assert/strict";
import fs from "node:fs";
import ts from "typescript";
import { compileModule } from "svelte/compiler";
import * as runtime from "svelte/internal/client";
import { isNewer, loadedOver, SaveQueue } from "../src/lib/settingSaves.ts";

const defaults = { "oxbow.background.intensity": 20, "oxbow.diff.contextLines": 3, "oxbow.diff.files": "smart" };
const source = fs.readFileSync(new URL("../src/lib/prefs.svelte.ts", import.meta.url), "utf8");
const cls = source.slice(source.indexOf("class Prefs {"), source.indexOf("export const prefs"));
const compiled = compileModule(ts.transpile(cls, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext }), { filename: "prefs.svelte.js", generate: "client" }).js.code
  .replace(/^import \* as \$ from ['"]svelte\/internal\/client['"];?$/m, "");
let revision = 0;
let stored: Record<string, unknown> = {};
let changed!: (event: { payload: { key: string; value: unknown; revision: number } }) => void;
let fail = false;
const api = {
  allSettings: async () => ({ values: { ...stored }, revision, problem: null }),
  setSetting: async (key: string, value: unknown) => {
    if (fail) throw new Error("Disk full");
    if (value === null) delete stored[key]; else stored[key] = value;
    return ++revision;
  },
};
const Prefs = Function("$", "api", "listen", "defaults", "SaveQueue", "isNewer", "loadedOver", "problemOf", "isDefault", `${compiled}; return Prefs;`)(
  runtime, api, (_name: string, callback: typeof changed) => { changed = callback; return Promise.resolve(() => {}); }, defaults,
  SaveQueue, isNewer, loadedOver, () => null, (key: keyof typeof defaults, value: unknown) => defaults[key] === value,
);
const prefs = new Prefs();
await prefs.reload();
let reads = 0;
let folds = 0;
let intensityChanges = 0;
const stop = runtime.effect_root(() => {
  runtime.user_effect(() => { prefs.get("oxbow.diff.contextLines"); reads++; });
  runtime.user_effect(() => { prefs.get("oxbow.diff.files"); folds++; });
  runtime.user_effect(() => { prefs.get("oxbow.background.intensity"); intensityChanges++; });
});
runtime.flush();
assert.equal(reads, 1);
assert.equal(folds, 1);

// Optimistic slider updates and their save acknowledgements leave diff loads/folding alone.
for (const intensity of [21, 30, 45, 60]) {
  const save = prefs.set("oxbow.background.intensity", intensity);
  runtime.flush();
  assert.equal(prefs.get("oxbow.background.intensity"), intensity);
  assert.equal(await save, true);
  runtime.flush();
}
assert.ok(intensityChanges > 1);
assert.equal(reads, 1);
assert.equal(folds, 1);

// The main window receives backend events rather than making the slider's own saves.
changed({ payload: { key: "oxbow.background.intensity", value: 70, revision: ++revision } });
runtime.flush();
assert.equal(prefs.get("oxbow.background.intensity"), 70);
assert.equal(reads, 1);
assert.equal(folds, 1);

// Failed saves still roll back, without clearing the unrelated diff.
fail = true;
assert.equal(await prefs.set("oxbow.background.intensity", 90), false);
runtime.flush();
assert.equal(prefs.get("oxbow.background.intensity"), 70);
assert.equal(reads, 1);
fail = false;

// Relevant settings must still reload the diff or its folding, once per changed value.
await prefs.set("oxbow.diff.contextLines", 8);
runtime.flush();
assert.equal(reads, 2);
assert.equal(folds, 1);
await prefs.set("oxbow.diff.files", "collapsed");
runtime.flush();
assert.equal(reads, 2);
assert.equal(folds, 2);
stop();
