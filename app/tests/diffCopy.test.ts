import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import ts from "typescript";
import { compileModule } from "svelte/compiler";
import * as runtime from "svelte/internal/client";

const view = fs.readFileSync(new URL("../src/lib/DiffView.svelte", import.meta.url), "utf8");
const state = view.slice(view.indexOf("  let copySelection"), view.indexOf("  function selectedCode"));
const lifecycle = view.slice(view.indexOf("  const codeRoots"), view.indexOf("  function beginMouseSelection"));
const source = `
  function start(code, action) {
    let active = $state(true);
    let diff = $state.raw({ file: { path: "same.txt" } });
    const scope = { isActive: () => active };
    const copyParts = () => {};
    ${state}
    ${lifecycle}
    action(code, copyRoot, () => [diff, null]);
    return {
      replace: (next) => diff = next,
      setActive: (value) => active = value,
      select: () => copySelection = { code, text: "selected", x: 0, y: 0 },
      text: () => copySelection?.text,
    };
  }
`;
const compiled = compileModule(ts.transpile(source, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext }), {
  filename: "diffCopy.svelte.js", generate: "client",
}).js.code.replace(/^import \* as \$ from ['"]svelte\/internal\/client['"];?$/m, "");

test("copy selection expires when the same path shows a new diff or its tab is left", () => {
  class Code {
    closest() { return this; }
  }
  const code = new Code();
  let anchor: Code | null = null;
  const document = {
    getSelection: () => ({ anchorNode: anchor, removeAllRanges: () => { anchor = null; } }),
  };
  const start = Function("$", "untrack", "Element", "document", `${compiled}; return start;`)(
    runtime, runtime.untrack, Code, document,
  );
  let copy!: ReturnType<typeof start>;
  const stop = runtime.effect_root(() => { copy = start(code, runtime.action); });
  const select = () => {
    anchor = code;
    copy.select();
    runtime.flush();
    assert.equal(copy.text(), "selected");
  };
  const cleared = () => {
    runtime.flush();
    assert.equal(copy.text(), undefined);
    assert.equal(anchor, null);
  };
  try {
    select();
    copy.replace({ file: { path: "same.txt" } });
    cleared();
    select();
    copy.setActive(false);
    cleared();
    const otherTab = new Code();
    anchor = otherTab;
    copy.setActive(true);
    runtime.flush();
    assert.equal(copy.text(), undefined);
    assert.equal(anchor, otherTab);
    select();
  } finally {
    stop();
  }
  assert.equal(copy.text(), undefined, "destroy clears the snapshot");
});
