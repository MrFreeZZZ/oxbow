// Exercise the actual preference store with Svelte's real derived values and effects.
import assert from "node:assert/strict";
import fs from "node:fs";
import ts from "typescript";
import { compileModule } from "svelte/compiler";
import * as runtime from "svelte/internal/client";
import { isNewer, loadedOver, SaveQueue } from "../src/lib/settingSaves.ts";
import { REPOSITORY_TAB_TAGS, TAB_TAG_ASSIGNMENT_KEY, TAB_TAG_COLOR_KEY, TAB_TAG_STYLE_KEY, tabTagEntryProblem, tabTagSettingProblem, type TabTagStyle } from "../src/lib/repositoryTabTags.ts";

const defaults = { "oxbow.background.intensity": 20, "oxbow.diff.contextLines": 3, "oxbow.diff.ignoreWhitespace": false, "oxbow.diff.files": "smart", [TAB_TAG_ASSIGNMENT_KEY]: {}, [TAB_TAG_COLOR_KEY]: {}, [TAB_TAG_STYLE_KEY]: {} };
const source = fs.readFileSync(new URL("../src/lib/prefs.svelte.ts", import.meta.url), "utf8");
const cls = source.slice(source.indexOf("class Prefs {"), source.indexOf("export const prefs"));
const compiled = compileModule(ts.transpile(cls, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext }), { filename: "prefs.svelte.js", generate: "client" }).js.code
  .replace(/^import \* as \$ from ['"]svelte\/internal\/client['"];?$/m, "");
let revision = 0;
let stored: Record<string, unknown> = {};
let changed!: (event: { payload: { key: string; value: unknown; revision: number } }) => void;
let fail = false;
let saving: Promise<void> | undefined;
const api = {
  allSettings: async () => ({ values: { ...stored }, revision, problem: null }),
  setSetting: async (key: string, value: unknown) => {
    await saving;
    if (fail) throw new Error("Disk full");
    if (value === null) delete stored[key]; else stored[key] = value;
    return ++revision;
  },
  editTabTag: async (setting: "styles" | "assignments", entry: string, value: string | TabTagStyle | null) => {
    await saving;
    if (fail) throw new Error("Disk full");
    const key = setting === "styles" ? TAB_TAG_STYLE_KEY : TAB_TAG_ASSIGNMENT_KEY;
    const entries = { ...stored[key] as Record<string, unknown> | undefined };
    if (value === null) delete entries[entry]; else entries[entry] = value;
    stored[key] = entries;
    return { key, value: entries, revision: ++revision };
  },
};
const Prefs = Function("$", "api", "listen", "defaults", "SaveQueue", "isNewer", "loadedOver", "problemOf", "isDefault", "REPOSITORY_TAB_TAGS", "TAB_TAG_ASSIGNMENT_KEY", "TAB_TAG_COLOR_KEY", "TAB_TAG_STYLE_KEY", "tabTagEntryProblem", `${compiled}; return Prefs;`)(
  runtime, api, (_name: string, callback: typeof changed) => { changed = callback; return Promise.resolve(() => {}); }, defaults,
  SaveQueue, isNewer, loadedOver, () => null, (key: keyof typeof defaults, value: unknown) => defaults[key] === value,
  REPOSITORY_TAB_TAGS, TAB_TAG_ASSIGNMENT_KEY, TAB_TAG_COLOR_KEY, TAB_TAG_STYLE_KEY, tabTagEntryProblem,
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

// Selecting a file during the save should still use the currently applied mode.
const release = Promise.withResolvers<void>();
saving = release.promise;
const save = prefs.set("oxbow.diff.ignoreWhitespace", true);
runtime.flush();
assert.equal(prefs.get("oxbow.diff.ignoreWhitespace"), false);
release.resolve();
assert.equal(await save, true);
runtime.flush();
assert.equal(prefs.get("oxbow.diff.ignoreWhitespace"), true);
stop();

assert.equal(prefs.repositoryTabTag("/repo"), null);
assert.deepEqual(prefs.repositoryTabTagStyle("red"), { label: "Red", color: "#ff3038" });
const both = Promise.all([prefs.saveRepositoryTabTag("/repo", "red"), prefs.saveRepositoryTabTag("/worktree", "blue")]);
assert.equal(prefs.repositoryTabTag("/repo"), "red");
assert.equal(prefs.repositoryTabTag("/worktree"), "blue");
await both;
assert.deepEqual(stored[TAB_TAG_ASSIGNMENT_KEY], { "/repo": "red", "/worktree": "blue" });
changed({ payload: { key: TAB_TAG_COLOR_KEY, value: { red: "#102030" }, revision: ++revision } });
assert.deepEqual(prefs.repositoryTabTagStyle("red"), { label: "Red", color: "#102030" });
await prefs.saveRepositoryTabTagStyle("red", { label: "Urgent", color: "#102030" });
assert.deepEqual(stored[TAB_TAG_STYLE_KEY], { red: { label: "Urgent", color: "#102030" } });
assert.deepEqual(prefs.repositoryTabTagStyle("red"), { label: "Urgent", color: "#102030" });
assert.equal(prefs.repositoryTabTag("/repo"), "red");
await prefs.saveRepositoryTabTagStyle("red", { label: "Urgent", color: "#0042ff" });
assert.deepEqual(prefs.repositoryTabTagStyle("red"), { label: "Urgent", color: "#0042ff" });
fail = true;
await assert.rejects(prefs.saveRepositoryTabTagStyle("red", { label: "Review", color: "#123abc" }), /Disk full/);
assert.deepEqual(prefs.repositoryTabTagStyle("red"), { label: "Urgent", color: "#0042ff" });
fail = false;
await prefs.saveRepositoryTabTagStyle("red", { label: "Red", color: "#102030" });
assert.deepEqual(prefs.repositoryTabTagStyle("red"), { label: "Red", color: "#102030" });

changed({ payload: { key: TAB_TAG_ASSIGNMENT_KEY, value: { "/repo": "green", "/worktree": "blue" }, revision: ++revision } });
assert.equal(prefs.repositoryTabTag("/repo"), "green");
fail = true;
const removal = prefs.saveRepositoryTabTag("/repo", null);
assert.equal(prefs.repositoryTabTag("/repo"), null);
await assert.rejects(removal, /Disk full/);
assert.equal(prefs.repositoryTabTag("/repo"), "green");
fail = false;
await prefs.saveRepositoryTabTag("/repo", null);
assert.deepEqual(stored[TAB_TAG_ASSIGNMENT_KEY], { "/worktree": "blue" });

assert.equal(tabTagSettingProblem(TAB_TAG_COLOR_KEY, { gray: "#123aBc" }), null);
assert.ok(tabTagSettingProblem(TAB_TAG_COLOR_KEY, { blue: "#123abc\n" }));
assert.ok(tabTagSettingProblem(TAB_TAG_COLOR_KEY, { cyan: "#123abc" }));
assert.equal(tabTagSettingProblem(TAB_TAG_STYLE_KEY, { red: { label: "Urgent", color: "#123abc" } }), null);
assert.ok(tabTagSettingProblem(TAB_TAG_STYLE_KEY, { red: { label: " ", color: "#123abc" } }));
assert.ok(tabTagSettingProblem(TAB_TAG_STYLE_KEY, { red: { label: "Urgent\nNow", color: "#123abc" } }));
assert.equal(tabTagSettingProblem(TAB_TAG_ASSIGNMENT_KEY, { "C:\\Worktree": "purple" }), null);
assert.ok(tabTagSettingProblem(TAB_TAG_ASSIGNMENT_KEY, { worktree: "purple" }));
assert.ok(tabTagSettingProblem(TAB_TAG_ASSIGNMENT_KEY, { "/worktree": "cyan" }));
