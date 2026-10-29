import { LIMIT, amend, canBack, canForward, crumbs, detailOfPlace, emptyTrail, go, nameOfPlace, placeKey, visit, type Place, type Trail } from "../src/lib/backStack.ts";
import { Keymap } from "../src/lib/shortcuts.ts";
import assert from "node:assert/strict";

const history = (commit: string | null, scroll: number | null = null): Place => ({ view: "history", commit, scroll });
const file = (path: string, commit: string | null = null, line: number | null = null): Place => ({ view: "file", path, commit, mode: "blame", at: null, line });
const pull: Place = { view: "pull", branch: "feature/login" };
const name = (p: Place) => (p.view === "file" ? (p.line ? `Line ${p.line}` : p.path) : p.view === "pull" ? "PR #42" : p.view === "history" ? "History" : p.view);
const walk = (...places: Place[]): Trail => places.reduce((t, p) => visit(t, p), emptyTrail());

// Clicks and arrow keys in History change how it is left; they add no places.
{
  const t = walk(history("a"), history("b"), history("c"));
  assert.equal(t.places.length, 1);
  assert.equal((t.places[0] as { commit: string }).commit, "c");
  assert.equal(canBack(t), false);
}

// A leap (a search result, a parent) is a place of its own; one to nowhere new is not.
{
  let t = walk(history("a"));
  t = visit(t, history("b"), true);
  assert.equal(t.places.length, 2);
  assert.equal(visit(t, history("b"), true), t);
  // The first commit selected after opening is no leap from nothing.
  assert.equal(visit(walk(history(null)), history("a"), true).places.length, 1);
}

// Another screen, file or followed line is a new place; Changes or Blame and the commit are not.
{
  let t = walk(pull, file("src/auth.ts"), file("src/auth.ts", "c1"), { ...file("src/auth.ts", "c1"), mode: "changes" } as Place);
  assert.equal(t.places.length, 2);
  t = visit(t, file("src/auth.ts", "c2", 12));
  assert.equal(t.places.length, 3);
  assert.deepEqual(t.places.map(placeKey), ["pull\x00feature/login", "file\x00src/auth.ts\x00", "file\x00src/auth.ts\x0012"]);
}

// Back and Forward move along the trail; a new place drops what was ahead.
{
  let t = walk(history("a"), pull, file("x.ts"));
  t = go(t, t.at - 1)!;
  assert.equal(t.places[t.at].view, "pull");
  assert.equal(canForward(t), true);
  t = go(t, t.at - 1)!;
  // Coming back to History with its commit as left changes nothing.
  assert.equal(visit(t, history("a")), t);
  t = visit(t, { view: "oplog" });
  assert.equal(t.places.length, 2);
  assert.equal(canForward(t), false);
  assert.equal(go(t, 5), null);
  assert.equal(go(t, t.at), null);
}

// History's scroll is set on the place being left.
{
  const t = amend(walk(history("a")), (p) => (p.view === "history" ? { ...p, scroll: 880 } : p));
  assert.deepEqual(t.places[0], history("a", 880));
  assert.deepEqual(amend(emptyTrail(), (p) => p), emptyTrail());
}

// The trail keeps its newest places.
{
  let t = emptyTrail();
  for (let i = 0; i < LIMIT + 10; i++) t = visit(t, file(`f${i}.ts`));
  assert.equal(t.places.length, LIMIT);
  assert.equal((t.places[0] as { path: string }).path, "f10.ts");
  assert.equal(t.at, LIMIT - 1);
}

// The header's path: places before this one, runs named once, the screen itself left out.
{
  let t = walk(history("a"));
  t = visit(t, history("b"), true);
  t = visit(t, pull);
  t = visit(t, file("src/auth.ts"));
  t = visit(t, file("src/auth.ts", null, 12));
  assert.deepEqual(crumbs(t, name), [
    { label: "History", at: 1 },
    { label: "PR #42", at: 2 },
    { label: "src/auth.ts", at: 3 },
  ]);
  assert.deepEqual(crumbs(t, name, 2).map((c) => c.label), ["PR #42", "src/auth.ts"]);
  assert.deepEqual(crumbs(go(t, 0)!, name), []);
}

// Go Back and Go Forward: VS Code's keys, and ⌘[ ⌘] on the Mac.
{
  type Press = Partial<Pick<KeyboardEvent, "key" | "code" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey">>;
  const press = (p: Press) => ({ key: "", code: "", metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...p }) as KeyboardEvent;
  const id = (hit: ReturnType<Keymap["match"]>) => (hit && "id" in hit ? hit.id : null);
  const mac = new Keymap(true);
  assert.equal(id(mac.match(press({ key: "-", code: "Minus", ctrlKey: true }))), "back");
  assert.equal(id(mac.match(press({ key: "_", code: "Minus", ctrlKey: true, shiftKey: true }))), "forward");
  assert.equal(id(mac.match(press({ key: "[", code: "BracketLeft", metaKey: true }))), "back");
  assert.equal(id(mac.match(press({ key: "]", code: "BracketRight", metaKey: true }))), "forward");
  // A Russian layout types х and ъ on those keys.
  assert.equal(id(mac.match(press({ key: "х", code: "BracketLeft", metaKey: true }))), "back");
  // ⌘- is still Zoom Out.
  assert.equal(id(mac.match(press({ key: "-", code: "Minus", metaKey: true }))), "zoomOut");
  const other = new Keymap(false);
  assert.equal(id(other.match(press({ key: "ArrowLeft", code: "ArrowLeft", altKey: true }))), "back");
  assert.equal(id(other.match(press({ key: "ArrowRight", code: "ArrowRight", altKey: true }))), "forward");
  assert.equal(id(other.match(press({ key: "-", code: "Minus", ctrlKey: true, altKey: true }))), "back");
  assert.equal(id(other.match(press({ key: "_", code: "Minus", ctrlKey: true, shiftKey: true }))), "forward");
}

// Places are named for the header and the list of places: a pull request by its number once
// it has one, a commit by its summary.
{
  const pulls = (branch: string, number: number | null) => (number === 7 || branch === "feature/login" ? { number: 7 } : null);
  assert.equal(nameOfPlace(pull, pulls), "PR #7");
  assert.equal(nameOfPlace({ view: "pull", branch: "other" }, pulls), "Pull Request other");
  assert.equal(nameOfPlace(file("src/app/main.rs"), pulls), "main.rs");
  assert.equal(nameOfPlace(file("src/app/main.rs", null, 12), pulls), "Line 12");
  const rows = new Map([["a", { summary: "Fix login", worktree: null }], ["w", { summary: "", worktree: {} }]]);
  assert.equal(detailOfPlace(history("a"), (id) => rows.get(id), "main"), "Fix login");
  assert.equal(detailOfPlace(history("w"), (id) => rows.get(id), "main"), "Uncommitted changes");
  assert.equal(detailOfPlace(history("gone"), (id) => rows.get(id), "main"), undefined);
  assert.equal(detailOfPlace({ view: "stack", branch: null }, (id) => rows.get(id), "main"), "main");
  assert.equal(detailOfPlace(file("a.rs"), (id) => rows.get(id), "main"), "a.rs · Blame");
  // Browse Files: by the branch it was opened from, else the commit; under it the file shown.
  const browse = (path: string | null, label: string | null = null): Place => ({ view: "browse", rev: "a1b2c3d4e5", label, path });
  assert.equal(nameOfPlace(browse(null, "main"), pulls), "Files of main");
  assert.equal(nameOfPlace(browse(null), pulls), "Files at a1b2c3d");
  assert.equal(detailOfPlace(browse("src/lib.rs"), (id) => rows.get(id), "main"), "src/lib.rs");
  assert.equal(detailOfPlace({ ...browse(null), rev: "a" }, (id) => rows.get(id), "main"), "Fix login");
  // Another file of the same commit is how you leave it; another commit is another place.
  const t = visit(visit(emptyTrail(), browse("a.rs")), browse("b.rs"));
  assert.deepEqual(t.places, [browse("b.rs")]);
  assert.equal(visit(t, { ...browse(null), rev: "f00" }).places.length, 2);
}
