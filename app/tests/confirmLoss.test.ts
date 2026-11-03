import assert from "node:assert/strict";
import fs from "node:fs";
import ts from "typescript";

// Even with confirmations disabled, a discard whose files cannot be recovered must ask.
const source = fs.readFileSync(new URL("../src/lib/confirm.svelte.ts", import.meta.url), "utf8")
  .replace(/^import .*;\n/gm, "").replace(/export /g, "");
const js = ts.transpile(source, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None });
for (const kind of ["discardChangelist", "takeVersion", "restoreLocalVersion"]) {
  let performed = 0;
  const gap = { kind: "bigFiles", files: [{ path: "big.bin", size: 60_000_000 }], more: 0, limit: 50_000_000 };
  const globals = {
    $state: (x: unknown) => x,
    getCurrentWindow: () => ({ listen: async () => () => {} }),
    prefs: { get: () => false },
    api: {
      planAction: async () => ({ commands: [], notKept: [gap] }),
      performAction: async () => { performed++; return ""; },
    },
  };
  const state = Function(...Object.keys(globals), `${js}; return confirm;`)(...Object.values(globals));
  const done = state.run({ title: "Discard?", action: { kind }, danger: true });
  await new Promise<void>(resolve => setTimeout(resolve, 0));
  assert.equal(performed, 0);
  assert.equal(state.phase, "ask");
  assert.deepEqual(state.notKept, [gap]);
  state.cancel();
  assert.equal(await done, false);
}
