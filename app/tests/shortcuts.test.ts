import { accelerator, assign, bindingsOf, commandIds, conflicts, defaultsOf, Keymap, pressed, refusal, resetKeys, scopeOf, strokeOf, unassign, validCombo, type Custom, type ShortcutId } from "../src/lib/shortcuts.ts";
import { fit, rank, type PaletteItem } from "../src/lib/palette.ts";
import assert from "node:assert/strict";

type Press = Partial<Pick<KeyboardEvent, "key" | "code" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey">>;
const press = (p: Press) => ({ key: "", code: "", metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...p }) as KeyboardEvent;
const id = (hit: ReturnType<Keymap["match"]>) => (hit && "id" in hit ? hit.id : null);

// The Mac's keys are VS Code's: ⇧⌘P, ⌘B, ⌥⌘B, ⇧⌘U, ⌃⇧G, ⌘G.
{
  const map = new Keymap(true);
  assert.equal(id(map.match(press({ key: "P", code: "KeyP", metaKey: true, shiftKey: true }))), "palette");
  assert.equal(id(map.match(press({ key: "F1", code: "F1" }))), "palette");
  assert.equal(id(map.match(press({ key: "p", code: "KeyP", metaKey: true }))), "quickOpen");
  assert.equal(id(map.match(press({ key: "t", code: "KeyT", metaKey: true }))), "goToBranch");
  assert.equal(id(map.match(press({ key: "b", code: "KeyB", metaKey: true }))), "sidebar");
  // ⌥ turns B into ∫ on the Mac: the key's place still says B.
  assert.equal(id(map.match(press({ key: "∫", code: "KeyB", metaKey: true, altKey: true }))), "details");
  assert.equal(id(map.match(press({ key: "U", code: "KeyU", metaKey: true, shiftKey: true }))), "console");
  assert.equal(id(map.match(press({ key: "`", code: "Backquote", ctrlKey: true }))), "console");
  assert.equal(id(map.match(press({ key: "G", code: "KeyG", ctrlKey: true, shiftKey: true }))), "changes");
  assert.equal(id(map.match(press({ key: "g", code: "KeyG", metaKey: true }))), "findNext");
  assert.equal(id(map.match(press({ key: "G", code: "KeyG", metaKey: true, shiftKey: true }))), "findPrevious");
  assert.equal(id(map.match(press({ key: "Z", code: "KeyZ", metaKey: true, shiftKey: true }))), "redo");
  assert.equal(id(map.match(press({ key: "r", code: "KeyR", ctrlKey: true }))), "openRecent");
  assert.equal(id(map.match(press({ key: "r", code: "KeyR", metaKey: true }))), "reload");
  assert.equal(id(map.match(press({ key: "+", code: "Equal", metaKey: true, shiftKey: true }))), "zoomIn");
  assert.equal(id(map.match(press({ key: "Tab", code: "Tab", ctrlKey: true }))), "nextTab");
  assert.equal(id(map.match(press({ key: "ArrowLeft", code: "ArrowLeft", metaKey: true, altKey: true }))), "previousTab");
  // The old keys still work.
  assert.equal(id(map.match(press({ key: "s", code: "KeyS", metaKey: true, ctrlKey: true }))), "sidebar");
  // A plain letter is no shortcut, and neither is a modifier on its own.
  assert.equal(map.match(press({ key: "b", code: "KeyB" })), null);
  assert.equal(map.match(press({ key: "Meta", code: "MetaLeft", metaKey: true })), null);
}

// A Russian layout types no Latin letters: ⌘P and ⌘, are still found by the key's place.
{
  const map = new Keymap(true);
  assert.equal(id(map.match(press({ key: "з", code: "KeyP", metaKey: true }))), "quickOpen");
  assert.equal(id(map.match(press({ key: "б", code: "Comma", metaKey: true }))), "settings");
  assert.equal(id(map.match(press({ key: "ё", code: "Backquote", ctrlKey: true }))), "console");
}

// Chords: ⌘K then ⌘M, or F; anything else after ⌘K is missed; the second key must come soon.
{
  const map = new Keymap(true);
  assert.deepEqual(map.match(press({ key: "k", code: "KeyK", metaKey: true }), 0), { chord: "Mod+K" });
  // Holding ⌘ down again between the two keys changes nothing.
  assert.equal(map.match(press({ key: "Meta", code: "MetaLeft", metaKey: true }), 100), null);
  assert.equal(id(map.match(press({ key: "m", code: "KeyM", metaKey: true }), 200)), "expand");
  map.match(press({ key: "k", code: "KeyK", metaKey: true }), 1000);
  assert.equal(id(map.match(press({ key: "f", code: "KeyF" }), 1500)), "closeRepo");
  map.match(press({ key: "k", code: "KeyK", metaKey: true }), 2000);
  assert.deepEqual(map.match(press({ key: "x", code: "KeyX" }), 2500), { missed: "Mod+K X" });
  map.match(press({ key: "k", code: "KeyK", metaKey: true }), 3000);
  assert.equal(id(map.match(press({ key: "b", code: "KeyB", metaKey: true }), 9000)), "sidebar");
}

// Elsewhere Ctrl is Mod, and the Windows key is part of no combo.
{
  const map = new Keymap(false);
  assert.equal(id(map.match(press({ key: "P", code: "KeyP", ctrlKey: true, shiftKey: true }))), "palette");
  assert.equal(id(map.match(press({ key: "b", code: "KeyB", ctrlKey: true, altKey: true }))), "details");
  assert.equal(id(map.match(press({ key: "r", code: "KeyR", ctrlKey: true }))), "openRecent");
  assert.equal(id(map.match(press({ key: "F5", code: "F5" }))), "reload");
  assert.equal(id(map.match(press({ key: "y", code: "KeyY", ctrlKey: true }))), "redo");
  assert.equal(id(map.match(press({ key: "F3", code: "F3", shiftKey: true }))), "findPrevious");
  assert.equal(map.match(press({ key: "b", code: "KeyB", metaKey: true })), null);
  assert.equal(strokeOf(press({ key: "Unidentified", code: "Tab", ctrlKey: true, shiftKey: true }), false), "Shift+Mod+Tab");
}

// No two commands that can meet share a combo: the window's with any, a screen's with its own.
for (const onMac of [true, false]) {
  for (const a of commandIds()) {
    for (const c of defaultsOf(a, onMac)) {
      assert.ok(validCombo(c), `${a}: ${c}`);
      const others = conflicts(a, c, {}, onMac);
      assert.deepEqual(others, [], `${onMac ? "mac" : "other"}: ${c} is both ${a} and ${others.map((o) => o.id).join(", ")}`);
    }
  }
}

// Your own keys go over the defaults, at once: a new one works, the old one no longer does.
{
  let mine: Custom = {};
  const map = new Keymap(true, () => mine);
  assert.equal(id(map.match(press({ key: "b", code: "KeyB", metaKey: true }))), "sidebar");
  mine = { sidebar: ["Ctrl+Mod+B"], push: ["Mod+Shift+K"] };
  assert.equal(map.match(press({ key: "b", code: "KeyB", metaKey: true })), null);
  assert.equal(id(map.match(press({ key: "b", code: "KeyB", metaKey: true, ctrlKey: true }))), "sidebar");
  // A command with no key of its own, and on a Russian layout too.
  assert.equal(id(map.match(press({ key: "Л", code: "KeyK", metaKey: true, shiftKey: true }))), "push");
  // An empty list takes the keys away.
  mine = { palette: [] };
  assert.equal(map.match(press({ key: "F1", code: "F1" })), null);
  // A chord of your own.
  mine = { fetch: ["Mod+K Mod+P"] };
  assert.deepEqual(map.match(press({ key: "k", code: "KeyK", metaKey: true }), 0), { chord: "Mod+K" });
  assert.equal(id(map.match(press({ key: "p", code: "KeyP", metaKey: true }), 100)), "fetch");
  // A screen's keys are no window shortcut.
  assert.equal(map.match(press({ key: "g", code: "KeyG" })), null);
  assert.equal(map.match(press({ key: "Enter", code: "Enter", metaKey: true })), null);
}

// A screen's command checks its own key: ⌘↩ commits, G marks good, yours if you changed them.
{
  const enter = press({ key: "Enter", code: "Enter", metaKey: true });
  assert.ok(pressed("commitNow", enter, {}, true));
  assert.ok(!pressed("commitNow", press({ key: "Enter", code: "Enter" }), {}, true));
  assert.ok(pressed("commitNow", press({ key: "Enter", code: "Enter", ctrlKey: true }), {}, false));
  assert.ok(!pressed("commitNow", enter, { commitNow: ["Ctrl+Enter"] }, true));
  assert.ok(pressed("bisectGood", press({ key: "п", code: "KeyG" }), {}, true));
  assert.ok(!pressed("bisectGood", press({ key: "g", code: "KeyG", metaKey: true }), {}, true));
  assert.ok(pressed("stackUp", press({ key: "ArrowUp", code: "ArrowUp", altKey: true }), {}, true));
}

// Giving a key takes it from the command that had it; one equal to the defaults is not kept.
{
  let mine = assign("push", "Mod+B", {}, true);
  assert.deepEqual(mine, { push: ["Mod+B"], sidebar: ["Ctrl+Mod+S"] });
  assert.deepEqual(conflicts("fetch", "Mod+B", mine, true), [{ id: "push", combo: "Mod+B" }]);
  mine = assign("sidebar", "Mod+B", mine, true, undefined);
  // Push had no key before, so nothing is kept for it.
  assert.deepEqual(mine, { sidebar: ["Ctrl+Mod+S", "Mod+B"] });
  // In place of a key, and back to the defaults' order: nothing kept for it.
  mine = assign("sidebar", "Mod+B", { sidebar: ["Ctrl+Mod+S"] }, true, "Ctrl+Mod+S");
  assert.deepEqual(mine, { sidebar: ["Mod+B"] });
  assert.deepEqual(unassign("sidebar", "Mod+B", mine, true), { sidebar: [] });
  assert.deepEqual(resetKeys("sidebar", mine), {});
  assert.deepEqual(bindingsOf("sidebar", {}, true), ["Mod+B", "Ctrl+Mod+S"]);
  // ⌘K alone would cut off the chords that start with it.
  assert.deepEqual(conflicts("fetch", "Mod+K", {}, true).map((c) => c.id).sort(), ["closeRepo", "expand"]);
  // Two screens never meet: Edit Stack's S and bisect's S are both fine.
  assert.deepEqual(conflicts("stackSquash", "S", {}, true), []);
  assert.deepEqual(conflicts("bisectGood", "S", {}, true).map((c) => c.id), ["bisectSkip"]);
  assert.equal(scopeOf("fetch"), "window");
}

// Keys a command can't have.
{
  assert.match(refusal("push", "K", true)!, /Add ⌘, ⌃ or ⌥/);
  assert.match(refusal("push", "Shift+K", false)!, /Add Ctrl or Alt/);
  assert.equal(refusal("push", "F6", true), null);
  assert.equal(refusal("push", "Mod+K F", true), null);
  assert.match(refusal("push", "Mod+Q", true)!, /quits Oxbow/);
  assert.match(refusal("push", "Ctrl+C", false)!, /copies/);
  assert.equal(refusal("bisectGood", "N", true), null);
  assert.match(refusal("commitNow", "Enter", true)!, /types in text fields/);
  assert.match(refusal("stackPick", "Mod+K P", true)!, /one press/);
  assert.ok(!validCombo("Mod+Hyper+K"));
  assert.ok(!validCombo("Mod+K Mod+M Mod+X"));
  assert.ok(!validCombo(""));
  assert.ok(validCombo("Alt+Shift+F5"));
}

// The app menu's Settings… shows the same key; a chord it can't show.
{
  assert.equal(accelerator("Mod+,"), "CmdOrCtrl+,");
  assert.equal(accelerator("Ctrl+Alt+S"), "Ctrl+Alt+S");
  assert.equal(accelerator("Mod+K Mod+S"), null);
  assert.equal(accelerator(null), null);
}

// The palette finds commands by words, word starts and initials.
{
  assert.deepEqual(fit("Git: Push", "push")?.hits, [5, 6, 7, 8]);
  assert.ok(fit("View: Toggle Sidebar", "tog side"));
  assert.deepEqual(fit("View: Show Current Branch Only", "scb")?.hits, [6, 11, 19]);
  assert.equal(fit("Git: Fetch", "pull"), null);
  // At a word's start beats inside a word.
  assert.ok(fit("Git: Stash", "st")!.score > fit("View: Show History", "st")!.score);

  const item = (id: string, category: string, title: string): PaletteItem => ({ id, category, title, run: () => {} });
  const items = [item("push", "Git", "Push"), item("pull", "Git", "Pull"), item("fetch", "Git", "Fetch"), item("settings", "Preferences", "Open Settings")];
  // Nothing typed: the ones used lately, newest first, then the rest by name.
  assert.deepEqual(
    rank(items, "", ["settings", "push"]).map((r) => [r.item.id, r.recent]),
    [["settings", true], ["push", true], ["fetch", false], ["pull", false]],
  );
  assert.deepEqual(rank(items, "pu", ["pull"]).map((r) => r.item.id), ["pull", "push"]);
}
