// Exercise the actual preference store with Svelte's real derived values and effects.
import assert from "node:assert/strict";
import fs from "node:fs";
import ts from "typescript";
import { compileModule } from "svelte/compiler";
import * as runtime from "svelte/internal/client";
import { isNewer, loadedOver, SaveQueue } from "../src/lib/settingSaves.ts";
import { REPOSITORY_TAB_TAGS, TAB_TAG_ASSIGNMENT_KEY, TAB_TAG_COLOR_KEY, TAB_TAG_STYLE_KEY, tabTagEntryProblem, tabTagSettingProblem, type TabTagStyle } from "../src/lib/repositoryTabTags.ts";

const backgroundKeys = ["oxbow.background.source", "oxbow.background.image", "oxbow.background.intensity"] as const;
const defaults = {
  "oxbow.background.source": "none", "oxbow.background.image": "", "oxbow.background.intensity": 20,
  "oxbow.diff.contextLines": 3, "oxbow.diff.ignoreWhitespace": false, "oxbow.diff.files": "smart",
  [TAB_TAG_ASSIGNMENT_KEY]: {}, [TAB_TAG_COLOR_KEY]: {}, [TAB_TAG_STYLE_KEY]: {},
};
const source = fs.readFileSync(new URL("../src/lib/prefs.svelte.ts", import.meta.url), "utf8");
const cls = source.slice(source.indexOf("const tabTagSaveKey"), source.indexOf("export const prefs"));
const compiled = compileModule(ts.transpile(cls, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext }), { filename: "prefs.svelte.js", generate: "client" }).js.code
  .replace(/^import \* as \$ from ['"]svelte\/internal\/client['"];?$/m, "");
let revision = 0;
let stored: Record<string, unknown> = {};
type SettingChange = { key: string; value: unknown; revision: number };
type GroupReset = { keys: readonly string[]; revision: number };
const listeners = new Map<string, ((event: { payload: SettingChange | GroupReset }) => void)[]>();
const emit = (name: string, payload: SettingChange | GroupReset) => {
  for (const listener of listeners.get(name) ?? []) listener({ payload });
};
const changed = ({ payload }: { payload: SettingChange }) => emit("settings-changed", payload);
let fail = false;
let saving: Promise<void> | undefined;
let resetRequests = 0;
let resetReply: Promise<void> | undefined;
const intensityWrites: unknown[] = [];
const api = {
  allSettings: async () => ({ values: { ...stored }, revision, problem: null }),
  setSetting: async (key: string, value: unknown) => {
    await saving;
    if (fail) throw new Error("Disk full");
    if (key === "oxbow.background.intensity") intensityWrites.push(value);
    if (value === null) delete stored[key]; else stored[key] = value;
    return ++revision;
  },
  resetSettingsGroup: async (keys: readonly string[]) => {
    resetRequests++;
    await saving;
    if (fail) throw new Error("Disk full");
    for (const key of keys) delete stored[key];
    const reset = { keys, revision: ++revision };
    emit("settings-group-reset", reset);
    await resetReply;
    return reset.revision;
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
  runtime, api, (name: string, callback: (event: { payload: SettingChange | GroupReset }) => void) => {
    listeners.set(name, [...listeners.get(name) ?? [], callback]);
    return Promise.resolve(() => {});
  }, defaults,
  SaveQueue, isNewer, loadedOver, (key: keyof typeof defaults, value: unknown) => typeof defaults[key] === typeof value ? null : "Wrong type",
  (key: keyof typeof defaults, value: unknown) => defaults[key] === value,
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

const contextKey = "oxbow.diff.contextLines";
for (const value of [defaults[contextKey], "invalid", null]) {
  stored[contextKey] = value;
  changed({ payload: { key: contextKey, value, revision: ++revision } });
  // A null event may mean a text edit stored null, so let its reload finish.
  await Promise.resolve();
  assert.equal(prefs.get(contextKey), defaults[contextKey]);
  assert.equal(prefs.changed(contextKey), false);
  assert.equal(prefs.hasStored(contextKey), true);
  assert.equal(await prefs.reset(contextKey), true);
  assert.equal(prefs.hasStored(contextKey), false);
  assert.equal(Object.hasOwn(stored, contextKey), false);
}

const write = prefs.set(contextKey, 8);
assert.equal(prefs.hasStored(contextKey), false);
assert.equal(await write, true);
assert.equal(prefs.hasStored(contextKey), true);
fail = true;
const reset = prefs.reset(contextKey);
assert.equal(prefs.hasStored(contextKey), true);
assert.equal(await reset, false);
assert.equal(prefs.hasStored(contextKey), true);
assert.match(prefs.failure.error, /Disk full/);
fail = false;
assert.equal(await prefs.reset(contextKey), true);
assert.equal(prefs.hasStored(contextKey), false);
assert.equal(prefs.failure, null);

// A failed batch changes neither window. Retry repeats the whole group against the latest file.
const other = new Prefs();
const drain = () => new Promise<void>((resolve) => setImmediate(resolve));
const tagKeys = [TAB_TAG_STYLE_KEY, TAB_TAG_COLOR_KEY] as const;
Object.assign(stored, { [TAB_TAG_STYLE_KEY]: null, [TAB_TAG_COLOR_KEY]: { red: "#102030" }, unknown: true });
revision++;
await Promise.all([prefs.reload(), other.reload()]);
fail = true;
assert.equal(await prefs.resetGroup(tagKeys), false);
assert.deepEqual(tagKeys.map((key) => prefs.hasStored(key)), [true, true]);
assert.deepEqual(tagKeys.map((key) => other.hasStored(key)), [true, true]);
assert.match(prefs.failure.error, /Disk full/);
fail = false;
await other.set(contextKey, 12);
assert.equal(await prefs.retry(), true);
assert.equal(resetRequests, 2);
assert.equal(prefs.failure, null);
assert.deepEqual(tagKeys.map((key) => prefs.hasStored(key)), [false, false]);
assert.deepEqual(tagKeys.map((key) => other.hasStored(key)), [false, false]);
assert.deepEqual(stored[TAB_TAG_ASSIGNMENT_KEY], { "/worktree": "blue" });
assert.equal(stored[contextKey], 12);
assert.equal(stored.unknown, true);

// A pending style edit runs before reset. A later assignment can save independently.
const styleGate = Promise.withResolvers<void>();
saving = styleGate.promise;
const style = prefs.saveRepositoryTabTagStyle("red", { label: "Pending", color: "#123abc" });
const clearStyles = prefs.resetGroup(tagKeys);
await drain();
assert.equal(resetRequests, 2);
saving = undefined;
await prefs.saveRepositoryTabTag("/repo", "green");
assert.equal(resetRequests, 2);
styleGate.resolve();
await Promise.all([style, clearStyles]);
assert.equal(resetRequests, 3);
assert.equal(Object.hasOwn(stored, TAB_TAG_STYLE_KEY), false);
assert.equal(prefs.repositoryTabTag("/repo"), "green");

// A late reset event or old write event can affect only keys it is still newer than.
const resetRevision = revision;
stored[TAB_TAG_STYLE_KEY] = { red: { label: "New", color: "#123abc" } };
changed({ payload: { key: TAB_TAG_STYLE_KEY, value: stored[TAB_TAG_STYLE_KEY], revision: ++revision } });
emit("settings-group-reset", { keys: tagKeys, revision: resetRevision });
changed({ payload: { key: TAB_TAG_COLOR_KEY, value: { red: "#abcdef" }, revision: resetRevision - 1 } });
assert.equal(other.repositoryTabTagStyle("red").label, "New");
assert.equal(other.hasStored(TAB_TAG_COLOR_KEY), false);

const reply = Promise.withResolvers<void>();
resetReply = reply.promise;
const resetWithLateReply = prefs.resetGroup(tagKeys);
await drain();
assert.equal(prefs.hasStored(TAB_TAG_STYLE_KEY), false);
const newerStyle = { red: { label: "After reset", color: "#123abc" } };
stored[TAB_TAG_STYLE_KEY] = newerStyle;
changed({ payload: { key: TAB_TAG_STYLE_KEY, value: newerStyle, revision: ++revision } });
reply.resolve();
assert.equal(await resetWithLateReply, true);
resetReply = undefined;
assert.equal(prefs.repositoryTabTagStyle("red").label, "After reset");

// A background image selection finishes both writes before the reset.
const imageGate = Promise.withResolvers<void>();
saving = imageGate.promise;
const image = prefs.saveBackgroundImage("/tmp/background.png");
const resetImage = prefs.resetGroup(backgroundKeys);
imageGate.resolve();
await Promise.all([image, resetImage]);
saving = undefined;
assert.ok(backgroundKeys.every((key) => !Object.hasOwn(stored, key)));
assert.ok(backgroundKeys.every((key) => !other.hasStored(key)));

// Exercise the real debounce and preview code without depending on wall-clock timing.
const sliderSource = fs.readFileSync(new URL("../src/lib/backgroundIntensity.svelte.ts", import.meta.url), "utf8");
const slider = compileModule(ts.transpile(sliderSource.slice(sliderSource.indexOf("export const")).replaceAll("export ", ""),
  { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext }), { filename: "intensity.svelte.js", generate: "client" }).js.code
  .replace(/^import \* as \$ from ['"]svelte\/internal\/client['"];?$/m, "");
type Preview = { origin: string; sequence: number; value: number | null };
const previews: ((event: { payload: Preview }) => Promise<void>)[] = [];
function intensity(windowPrefs: typeof prefs) {
  const timers = new Set<() => void>();
  const controls = Function("$", "prefs", "emit", "listen", "setTimeout", "clearTimeout",
    `${slider}; return { backgroundIntensity, previewIntensity, discardIntensity, saveIntensity };`)(
    runtime, windowPrefs,
    async (_name: string, payload: Preview) => { for (const preview of previews) void preview({ payload }); },
    (_name: string, preview: typeof previews[number]) => { previews.push(preview); return Promise.resolve(); },
    (fn: () => void) => (timers.add(fn), fn), (fn: () => void) => timers.delete(fn),
  );
  return { ...controls, timers };
}
const localSlider = intensity(prefs);
const remoteSlider = intensity(other);
const intensityKey = backgroundKeys[2];
intensityWrites.length = 0;
await prefs.set(intensityKey, 40);
localSlider.previewIntensity(75);
await drain();
assert.equal(remoteSlider.backgroundIntensity.value, 75);
assert.equal(await prefs.resetGroup(backgroundKeys, localSlider.discardIntensity), true);
assert.equal(localSlider.timers.size, 0);
await localSlider.saveIntensity(); // Blur / close may still request a flush.
await drain();
assert.deepEqual(intensityWrites, [40]);
assert.equal(remoteSlider.backgroundIntensity.value, null);
assert.equal(other.get(intensityKey), 20);

const intensityGate = Promise.withResolvers<void>();
saving = intensityGate.promise;
localSlider.previewIntensity(80);
const inflight = localSlider.saveIntensity();
await drain();
const resetWhileSaving = prefs.resetGroup(backgroundKeys, localSlider.discardIntensity);
localSlider.previewIntensity(55);
const later = localSlider.saveIntensity();
assert.equal(localSlider.backgroundIntensity.value, 55);
intensityGate.resolve();
await Promise.all([inflight, resetWhileSaving, later]);
saving = undefined;
await drain();
assert.deepEqual(intensityWrites, [40, 80, 55]);
assert.equal(stored[intensityKey], 55);
assert.equal(other.get(intensityKey), 55);
assert.equal(remoteSlider.backgroundIntensity.value, null);

fail = true;
assert.equal(await prefs.resetGroup(backgroundKeys, localSlider.discardIntensity), false);
localSlider.previewIntensity(90);
fail = false;
assert.equal(await prefs.retry(), true);
await localSlider.saveIntensity();
assert.equal(localSlider.timers.size, 0);
assert.equal(Object.hasOwn(stored, intensityKey), false);
assert.equal(other.get(intensityKey), 20);
