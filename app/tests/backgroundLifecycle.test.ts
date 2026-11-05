// Run the actual background effects with real Svelte reactivity and controlled image I/O.
import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import ts from "typescript";
import { compileModule } from "svelte/compiler";
import * as runtime from "svelte/internal/client";
import { backgroundType } from "../src/lib/background.ts";
import { MAX_PIXELS, pictureSize, Urls, inTurn } from "../src/lib/pictures.ts";

const source = fs.readFileSync(new URL("../src/lib/background.svelte.ts", import.meta.url), "utf8")
  .replace(/^import .*;\n/gm, "").replace(/^export /gm, "");
const compiled = compileModule(ts.transpile(source, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext }), { filename: "background.svelte.js", generate: "client" }).js.code
  .replace(/^import \* as \$ from ['"]svelte\/internal\/client['"];?$/m, "");

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

const png = new Uint8Array(24);
png.set([137, 80, 78, 71, 13, 10, 26, 10]);
png.set([73, 72, 68, 82], 12);
new DataView(png.buffer).setUint32(16, 10);
new DataView(png.buffer).setUint32(20, 10);

async function settle() {
  runtime.flush();
  await new Promise<void>((resolve) => setImmediate(resolve));
  runtime.flush();
}

function fixture(render = true) {
  let version = "v1";
  const handlers = new Map<string, (event: { payload: any }) => void>();
  const events: { name: string; payload: any }[] = [];
  const emit = async (name: string, payload: any) => { events.push({ name, payload }); };
  const listen = async (name: string, callback: (event: { payload: any }) => void) => { handlers.set(name, callback); return () => { handlers.delete(name); }; };
  const selection = runtime.state({ source: "none", image: "", pending: false });
  const reads: ReturnType<typeof deferred<Uint8Array>>[] = [];
  const decodes: { src: string; work: ReturnType<typeof deferred<void>> }[] = [];
  const live = new Set<string>();
  const roots: (() => void)[] = [];
  const window = new EventTarget();
  class OwnedUrls extends Urls {
    own = new Set<string>();
    override make(blob: Blob) {
      const url = super.make(blob);
      this.own.add(url);
      live.add(url);
      return url;
    }
    override close() {
      super.close();
      for (const url of this.own) live.delete(url);
      this.own.clear();
    }
  }
  class Image {
    src = "";
    decode() {
      const work = deferred<void>();
      decodes.push({ src: this.src, work });
      return work.promise;
    }
  }
  const prefs = {
    get: (key: string) => runtime.get(selection)[key.endsWith("source") ? "source" : "image"],
    pending: () => runtime.get(selection).pending,
  };
  const api = { backgroundImageVersion: async () => version, backgroundImage: () => { const read = deferred<Uint8Array>(); reads.push(read); return read.promise; } };
  const module = Function("emit", "listen", "$", "api", "prefs", "backgroundType", "MAX_PIXELS", "pictureSize", "Urls", "inTurn", "Image", "window", "spring", "summer", "autumn", "winter",
    `${compiled}; return { background, followBackground, followBackgroundStatus, retryBackground };`)(
    emit, listen, { ...runtime, effect_root: (fn: () => void) => { const stop = runtime.effect_root(fn); roots.push(stop); return stop; } },
    api, prefs, backgroundType, MAX_PIXELS, pictureSize, OwnedUrls, inTurn, Image, window, "spring.webp", "summer.webp", "autumn.webp", "winter.webp",
  );
  if (render) module.followBackground();
  else roots.push(runtime.effect_root(() => module.followBackgroundStatus()));
  runtime.flush();
  return {
    ...module, reads, decodes, live, window, events, handlers,
    version(value: string) { version = value; },
    select(source: string, image = "", pending = false) { runtime.set(selection, { source, image, pending }); runtime.flush(); },
    stop() { roots.forEach((stop) => stop()); assert.equal(live.size, 0); },
  };
}

test("a custom read finishing after a preset switch cannot replace that preset", async () => {
  const f = fixture();
  try {
    f.select("custom", "/old.png");
    await settle();
    f.select("summer");
    assert.equal(f.decodes[0].src, "summer.webp");
    f.decodes[0].work.resolve();
    await settle();
    f.reads[0].resolve(png);
    await settle();
    assert.equal(f.background.url, "summer.webp");
    assert.equal(f.background.problem, "");
    assert.equal(f.decodes.length, 1);
    assert.equal(f.live.size, 0);
  } finally { f.stop(); }
});

test("a stale custom decode cannot replace the newer file and releases its URL", async () => {
  const f = fixture();
  try {
    f.select("custom", "/old.png");
    await settle();
    f.reads[0].resolve(png);
    await settle();
    const old = f.decodes[0];
    f.select("custom", "/new.png");
    await settle();
    assert.equal(f.live.has(old.src), false);
    f.reads[1].resolve(png);
    await settle();
    const newest = f.decodes[1];
    newest.work.resolve();
    await settle();
    old.work.resolve();
    await settle();
    assert.equal(f.background.url, newest.src);
    assert.deepEqual([...f.live], [newest.src]);
  } finally { f.stop(); }
});

test("None stays off when an earlier decode fails", async () => {
  const f = fixture();
  try {
    f.select("spring");
    f.select("none");
    f.decodes[0].work.reject(new Error("Old decode failed"));
    await settle();
    assert.equal(f.background.url, "");
    assert.equal(f.background.loading, false);
    assert.equal(f.background.problem, "");
  } finally { f.stop(); }
});

test("custom reads wait for saved preferences and Retry recovers from a read error", async () => {
  const f = fixture();
  try {
    f.select("custom", "/new.png", true);
    await settle();
    assert.equal(f.reads.length, 0);
    f.select("custom", "/new.png");
    await settle();
    f.reads[0].reject(new Error("File missing"));
    await settle();
    assert.match(f.background.problem, /File missing/);
    assert.equal(f.background.loading, false);
    f.retryBackground();
    await settle();
    f.reads[1].resolve(png);
    await settle();
    f.decodes[0].work.resolve();
    await settle();
    assert.equal(f.background.url, f.decodes[0].src);
    assert.equal(f.background.problem, "");
  } finally { f.stop(); }
});

test("focus refresh retains the displayed image until its replacement decodes", async () => {
  const f = fixture();
  try {
    f.select("custom", "/photo.png");
    await settle();
    f.reads[0].resolve(png);
    await settle();
    f.decodes[0].work.resolve();
    await settle();
    const shown = f.background.url;
    f.version("v2");
    f.window.dispatchEvent(new Event("focus"));
    await settle();
    assert.equal(f.background.url, shown);
    assert.ok(f.live.has(shown), "the displayed URL stays valid while refreshing");
    f.reads[1].resolve(png);
    await settle();
    assert.equal(f.background.url, shown);
    f.decodes[1].work.resolve();
    await settle();
    assert.equal(f.background.url, f.decodes[1].src);
    assert.equal(f.live.size, 1);
  } finally { f.stop(); }
});


test("unchanged custom files do not reread or decode on focus; Retry still forces a read", async () => {
  const f = fixture();
  try {
    f.select("custom", "/photo.png");
    await settle();
    f.reads[0].resolve(png);
    await settle();
    f.decodes[0].work.resolve();
    await settle();
    f.window.dispatchEvent(new Event("focus"));
    await settle();
    assert.equal(f.reads.length, 1);
    assert.equal(f.decodes.length, 1);
    f.retryBackground();
    await settle();
    assert.equal(f.reads.length, 2);
    f.reads[1].resolve(png);
    await settle();
    f.decodes[1].work.resolve();
    await settle();
  } finally { f.stop(); }
});

test("Settings observes decode errors, ignores stale selections, and forwards Retry without decoding", async () => {
  const f = fixture(false);
  try {
    f.select("custom", "/photo.png");
    await settle();
    const report = f.handlers.get("background-status")!;
    report({ payload: { selection: JSON.stringify(["custom", "/old.png"]), loading: false, problem: "stale" } });
    assert.equal(f.background.problem, "");
    report({ payload: { selection: JSON.stringify(["custom", "/photo.png"]), loading: false, problem: "Decode failed" } });
    assert.equal(f.background.problem, "Decode failed");
    f.retryBackground();
    await settle();
    assert.ok(f.events.some((event: { name: string }) => event.name === "background-retry"));
    assert.equal(f.reads.length, 0);
    assert.equal(f.decodes.length, 0);
  } finally { f.stop(); }
});

test("a failed decode is reported by the rendering window and releases its URL", async () => {
  const f = fixture();
  try {
    f.select("custom", "/broken.png");
    await settle();
    f.reads[0].resolve(png);
    await settle();
    f.decodes[0].work.reject(new Error("Decode failed"));
    await settle();
    assert.equal(f.live.size, 0);
    assert.equal(f.background.url, "");
    assert.ok(f.events.some((event: { name: string; payload: { problem?: string } }) => event.name === "background-status" && event.payload.problem?.includes("Decode failed")));
  } finally { f.stop(); }
});
