import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import ts from "typescript";
import { compileModule } from "svelte/compiler";
import * as runtime from "svelte/internal/client";

function load(path: string, name: string) {
  const source = fs.readFileSync(new URL(path, import.meta.url), "utf8").replace(/^export /gm, "");
  const compiled = compileModule(ts.transpile(source, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext }), { filename: `${name}.svelte.js`, generate: "client" }).js.code
    .replace(/^import \* as \$ from ['"]svelte\/internal\/client['"];?$/m, "");
  return Function("$", "localStorage", `${compiled}; return ${name};`)(runtime, { getItem: () => null });
}
const FindQuery = load("../src/lib/findQuery.svelte.ts", "FindQuery");
const Nav = load("../src/lib/nav.svelte.ts", "Nav");

test("typing applies only the latest query, with its case option, after 100 ms", (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const query = new FindQuery();
  let scans = 0;
  const stop = runtime.effect_root(() => runtime.user_effect(() => { query.value; scans++; }));
  try {
    runtime.flush();
    query.update({ text: "a", matchCase: false });
    t.mock.timers.tick(60);
    query.update({ text: "ab", matchCase: true });
    t.mock.timers.tick(99);
    runtime.flush();
    assert.equal(query.value.text, "");
    assert.equal(scans, 1);
    t.mock.timers.tick(1);
    runtime.flush();
    assert.deepEqual(query.value, { text: "ab", matchCase: true });
    assert.equal(scans, 2);
    query.update({ text: "abc", matchCase: true });
    query.update({ text: "", matchCase: true });
    assert.equal(query.value.text, "", "clear is immediate");
    t.mock.timers.tick(200);
    assert.equal(query.value.text, "", "an old timer cannot restore cleared text");
    query.update({ text: "abandoned", matchCase: false });
    query.cancel();
    t.mock.timers.tick(200);
    assert.equal(query.value.text, "", "unmount cancels pending input");
  } finally { query.cancel(); stop(); }
});

test("navigation flushes pending input, then uses fresh counts and wraps", async () => {
  const nav = new Nav();
  const query = new FindQuery();
  nav.find = { text: "new", matchCase: true };
  nav.found = { count: 90, at: 70 };
  nav.applyFind = async () => {
    query.update(nav.find);
    const changed = query.flush();
    await Promise.resolve();
    if (changed) nav.found = { count: 3, at: 0 };
    return changed;
  };
  await nav.findStep(1);
  assert.equal(query.value.text, "new");
  assert.deepEqual(nav.found, { count: 3, at: 0 });
  await nav.findStep(-1);
  assert.deepEqual(nav.found, { count: 3, at: 2 });
  await nav.findStep(1);
  assert.deepEqual(nav.found, { count: 3, at: 0 });
  // A pending navigation must not step into another File History instance after unmount.
  const step = nav.findStep(1);
  nav.applyFind = null;
  await step;
  assert.equal(nav.found.at, 0);
  query.cancel();
});
