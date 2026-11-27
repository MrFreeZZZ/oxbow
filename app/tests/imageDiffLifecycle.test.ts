import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import ts from "typescript";
import { compileModule } from "svelte/compiler";
import * as runtime from "svelte/internal/client";
import { Urls, inTurn, loadPair } from "../src/lib/pictures.ts";

const source = fs.readFileSync(new URL("../src/lib/ImageDiff.svelte", import.meta.url), "utf8");
const logic = source.slice(source.indexOf("  type Side ="), source.indexOf("  const both ="));
const compiled = compileModule(ts.transpile(`function view(initial) {
  let diff = $state(initial);
  ${logic}
  return {
    get before() { return before; }, get after() { return after; },
    get error() { return error; }, get huge() { return huge; },
    refresh(change = {}) { diff = { ...diff, ...change }; }, near(value) { near = value; }
  };
}`, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext }), {
  filename: "imageDiff.svelte.js", generate: "client",
}).js.code.replace(/^import \* as \$ from ['"]svelte\/internal\/client['"];?$/m, "");

function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

async function settle() {
  runtime.flush();
  await new Promise<void>((resolve) => setImmediate(resolve));
  runtime.flush();
}

function fixture() {
  const live = new Set<string>();
  const reads: ReturnType<typeof deferred<Uint8Array>>[] = [];
  const decodes: ReturnType<typeof deferred<void>>[] = [];
  const cleanups: (() => void)[] = [];
  class OwnedUrls extends Urls {
    own: string[] = [];
    stopped = false;
    override make(blob: Blob) {
      const url = super.make(blob);
      if (!this.stopped) {
        this.own.push(url);
        live.add(url);
      }
      return url;
    }
    override close() {
      super.close();
      this.stopped = true;
      this.own.forEach((url) => live.delete(url));
      this.own = [];
    }
  }
  class Image {
    src = "";
    naturalWidth = 10;
    naturalHeight = 10;
    decode() { const work = deferred<void>(); decodes.push(work); return work.promise; }
  }
  const api = { sourceBytes: () => { const work = deferred<Uint8Array>(); reads.push(work); return work.promise; } };
  const create = Function("$", "api", "Urls", "inTurn", "loadPair", "imageType", "Image", "onDestroy", "tick",
    `${compiled}; return view;`)(runtime, api, OwnedUrls, inTurn, loadPair, () => "image/png", Image,
    (fn: () => void) => cleanups.push(fn), async () => { runtime.flush(); });
  let view: any;
  const stop = runtime.effect_root(() => {
    view = create({ file: { path: "picture.png" }, old: { kind: "blob", id: "old" }, new: { kind: "worktree", path: "picture.png" } });
  });
  view.near(true);
  runtime.flush();
  return {
    view, reads, decodes, live,
    async finish(start: number, data = new Uint8Array([1, 2, 3])) {
      const decoding = decodes.length;
      reads.slice(start, start + 2).forEach((r) => r.resolve(data));
      await settle();
      decodes.slice(decoding).forEach((d) => d.resolve());
      await settle();
    },
    stop() { stop(); cleanups.forEach((fn) => fn()); assert.equal(live.size, 0); },
  };
}

test("refresh keeps both pictures and their URLs until decoded replacements arrive", async () => {
  const f = fixture();
  try {
    await f.finish(0);
    const before = f.view.before.url, after = f.view.after.url;
    f.view.refresh();
    await settle();
    assert.equal(f.view.before.url, before);
    assert.equal(f.view.after.url, after);
    assert.ok(f.live.has(before) && f.live.has(after));
    f.reads.slice(2).forEach((r) => r.resolve(new Uint8Array([4, 5, 6])));
    await settle();
    assert.equal(f.view.after.url, after, "reading alone does not replace the shown image");
    f.decodes.slice(2).forEach((d) => d.resolve());
    await settle();
    assert.notEqual(f.view.after.url, after, "real edits replace the image after decoding");
    assert.deepEqual([...f.live], [f.view.before.url, f.view.after.url]);
  } finally { f.stop(); }
});

test("superseded reads cannot replace the latest pictures or leak URLs", async () => {
  const f = fixture();
  try {
    await f.finish(0);
    f.view.refresh();
    await settle();
    f.view.refresh();
    await settle();
    await f.finish(4);
    const after = f.view.after.url;
    await f.finish(2);
    assert.equal(f.view.after.url, after);
    assert.equal(f.live.size, 2);
  } finally { f.stop(); }
});

test("scrolling far away releases the displayed pictures and ignores pending reads", async () => {
  const f = fixture();
  try {
    await f.finish(0);
    f.view.refresh();
    await settle();
    f.view.near(false);
    await settle();
    assert.equal(f.view.after, null);
    assert.equal(f.live.size, 0);
    await f.finish(2);
    assert.equal(f.view.after, null);
    assert.equal(f.live.size, 0);
  } finally { f.stop(); }
});

test("a failed refresh shows its error and releases the previous pictures", async () => {
  const f = fixture();
  try {
    await f.finish(0);
    f.view.refresh();
    await settle();
    f.reads[2].reject(new Error("File removed"));
    f.reads[3].resolve(new Uint8Array([1]));
    await settle();
    f.decodes[2]?.resolve();
    await settle();
    assert.match(f.view.error, /File removed/);
    assert.equal(f.view.after, null);
    assert.equal(f.live.size, 0);
  } finally { f.stop(); }
});

test("an oversized replacement clears the old image and still asks before decoding", async () => {
  const f = fixture();
  try {
    await f.finish(0);
    f.view.refresh();
    await settle();
    const png = new Uint8Array(24);
    png.set([137, 80, 78, 71], 0);
    png.set([73, 72, 68, 82], 12);
    new DataView(png.buffer).setUint32(16, 12000);
    new DataView(png.buffer).setUint32(20, 12000);
    await f.finish(2, png);
    assert.equal(f.view.after, null);
    assert.equal(f.view.huge.width, 12000);
    assert.equal(f.decodes.length, 2);
    assert.equal(f.live.size, 0);
  } finally { f.stop(); }
});

test("closing during a refresh releases current and late picture URLs", async () => {
  const f = fixture();
  try {
    await f.finish(0);
    f.view.refresh();
    await settle();
    f.stop();
    await f.finish(2);
    assert.equal(f.live.size, 0);
    assert.equal(f.view.after, null);
  } finally { f.stop(); }
});

for (const [name, change] of Object.entries({
  "a different before blob after partial stage": { old: { kind: "blob", id: "staged" } },
  "switching from Unstaged to Staged": { new: { kind: "blob", id: "staged" } },
  "a different after blob": { new: { kind: "blob", id: "other" } },
  "a different working file": { new: { kind: "worktree", path: "other.png" } },
  "a different file path": { file: { path: "other.png" } },
  "a missing side": { old: null },
})) {
  test(`${name} clears the previous pictures before reading replacements`, async () => {
    const f = fixture();
    try {
      await f.finish(0);
      f.view.refresh(change);
      await settle();
      assert.equal(f.view.before, null);
      assert.equal(f.view.after, null);
      assert.equal(f.live.size, 0);
      await f.finish(2);
      assert.ok(f.view.after);
    } finally { f.stop(); }
  });
}

test("equivalent source descriptors keep the shown pictures", async () => {
  const f = fixture();
  try {
    await f.finish(0);
    const before = f.view.before.url, after = f.view.after.url;
    f.view.refresh({ old: { id: "old", kind: "blob" }, new: { path: "picture.png", kind: "worktree" } });
    await settle();
    assert.equal(f.view.before.url, before);
    assert.equal(f.view.after.url, after);
    await f.finish(2);
  } finally { f.stop(); }
});
