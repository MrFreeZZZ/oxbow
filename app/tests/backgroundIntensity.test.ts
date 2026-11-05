// Exercise the actual preview/save coordinator, including the cross-window event order.
import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import ts from "typescript";
import { compileModule } from "svelte/compiler";
import * as runtime from "svelte/internal/client";

const source = fs.readFileSync(new URL("../src/lib/backgroundIntensity.svelte.ts", import.meta.url), "utf8")
  .replace(/^import .*;\n/gm, "").replace(/^export /gm, "");
const compiled = compileModule(ts.transpile(source, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext }), { filename: "backgroundIntensity.svelte.js", generate: "client" }).js.code
  .replace(/^import \* as \$ from ['"]svelte\/internal\/client['"];?$/m, "");

function fixture(reload: () => Promise<void> = async () => {}, delivery: () => Promise<void> = async () => {}) {
  const writes: { value: number; resolve: (ok: boolean) => void }[] = [];
  const events: { sequence: number; value: number | null }[] = [];
  const timers = new Map<number, () => void>();
  let nextTimer = 0;
  let receive!: (event: { payload: { origin: string; sequence: number; value: number | null } }) => void;
  const prefs = { reload, set: (_key: string, value: number) => new Promise<boolean>((resolve) => writes.push({ value, resolve })) };
  const module = Function("$", "prefs", "emit", "listen", "crypto", "setTimeout", "clearTimeout", `${compiled}; return { backgroundIntensity, previewIntensity, saveIntensity };`)(
    runtime, prefs,
    async (_name: string, payload: typeof events[number]) => { events.push(payload); await delivery(); },
    async (_name: string, callback: typeof receive) => { receive = callback; },
    { randomUUID: () => "settings" },
    (callback: () => void, delay: number) => { assert.equal(delay, 200); timers.set(++nextTimer, callback); return nextTimer; },
    (id: number) => timers.delete(id),
  );
  return { ...module, writes, events, timers, receive,
    pause() { const work = [...timers.values()]; timers.clear(); work.forEach((run) => run()); },
  };
}
const settle = () => new Promise<void>((resolve) => setImmediate(resolve));

test("a drag previews every value in order but writes only its latest value after a pause", async () => {
  const f = fixture();
  for (let value = 21; value <= 90; value++) f.previewIntensity(value);
  await settle();
  assert.equal(f.events.length, 70);
  assert.equal(f.backgroundIntensity.value, 90);
  assert.equal(f.writes.length, 0);
  assert.equal(f.timers.size, 1);
  f.pause();
  assert.deepEqual(f.writes.map((write) => write.value), [90]);
  f.writes[0].resolve(true);
  await f.saveIntensity();
  await settle();
  assert.equal(f.backgroundIntensity.value, null);
  assert.equal(f.backgroundIntensity.saving, false);
  assert.equal(f.events.at(-1)?.value, null);
});

test("change or window close flushes the final value immediately and waits for persistence", async () => {
  const f = fixture();
  f.previewIntensity(80);
  const flush = f.saveIntensity();
  assert.equal(f.writes[0].value, 80);
  assert.equal(f.timers.size, 0);
  assert.equal(f.backgroundIntensity.saving, true);
  f.writes[0].resolve(true);
  await flush;
  assert.equal(f.backgroundIntensity.saving, false);
});

test("an older save cannot remove the preview of a newer drag; failures release the preview for rollback", async () => {
  const f = fixture();
  f.previewIntensity(40);
  const first = f.saveIntensity();
  f.previewIntensity(90);
  f.writes[0].resolve(true);
  await first;
  assert.equal(f.backgroundIntensity.value, 90);
  assert.equal(f.backgroundIntensity.saving, true);
  const last = f.saveIntensity();
  f.writes[1].resolve(false);
  await last;
  assert.equal(f.backgroundIntensity.value, null);
  assert.equal(f.backgroundIntensity.saving, false);
});

test("other windows ignore stale preview and clear events and reject invalid values", async () => {
  const f = fixture();
  const send = (sequence: number, value: number | null) => f.receive({ payload: { origin: "other-settings", sequence, value } });
  send(3, 80);
  send(2, null);
  assert.equal(f.backgroundIntensity.value, 80);
  send(4, 120);
  assert.equal(f.backgroundIntensity.value, 80);
  send(5, null);
  await settle();
  assert.equal(f.backgroundIntensity.value, null);
  send(4, 30);
  assert.equal(f.backgroundIntensity.value, null);
});


test("a late final-value reload cannot clear a newer cross-window preview", async () => {
  let complete!: () => void;
  const f = fixture(() => new Promise<void>((resolve) => { complete = resolve; }));
  f.receive({ payload: { origin: "other-settings", sequence: 1, value: 40 } });
  f.receive({ payload: { origin: "other-settings", sequence: 2, value: null } });
  assert.equal(f.backgroundIntensity.value, 40);
  f.receive({ payload: { origin: "other-settings", sequence: 3, value: 80 } });
  complete();
  await settle();
  assert.equal(f.backgroundIntensity.value, 80);
});


test("window close waits for the final preview clear to reach the other windows", async () => {
  let deliver!: () => void;
  const f = fixture(async () => {}, () => new Promise<void>((resolve) => { deliver = resolve; }));
  f.previewIntensity(70);
  await settle();
  deliver();
  await settle();
  const close = f.saveIntensity();
  f.writes[0].resolve(true);
  await settle();
  assert.equal(f.events.at(-1)?.value, null);
  assert.equal(f.backgroundIntensity.saving, true);
  deliver();
  await close;
  assert.equal(f.backgroundIntensity.saving, false);
});
