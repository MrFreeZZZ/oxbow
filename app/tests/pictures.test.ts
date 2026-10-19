import assert from "node:assert/strict";
import { test } from "node:test";
import { Urls, loadPair, pictureSize } from "../src/lib/pictures.ts";

// URLs made and let go, by the pictures' own URL functions.
const made: string[] = [];
const revoked = new Set<string>();
URL.createObjectURL = () => {
  const url = `blob:${made.length}`;
  made.push(url);
  return url;
};
URL.revokeObjectURL = (url: string) => void revoked.add(url);
const live = () => made.filter((u) => !revoked.has(u));
const reset = () => {
  made.length = 0;
  revoked.clear();
};

/** A PNG header of `width` × `height`. */
function png(width: number, height: number): Uint8Array {
  const data = new Uint8Array(32);
  data.set([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48, 0x44, 0x52]);
  const view = new DataView(data.buffer);
  view.setUint32(16, width);
  view.setUint32(20, height);
  return data;
}
const small = png(10, 10);
const tooBig = new Error("it is 40 MB, too big to preview here");
const later = (ms: number) => new Promise((go) => setTimeout(go, ms));
const decode = async () => ({ width: 10, height: 10 });

test("both sides drawn keep their URLs until closed", async () => {
  reset();
  const urls = new Urls();
  const sides = await loadPair(["old", "new"], "image/png", false, urls, async () => small, decode);
  assert.equal(sides.length, 2);
  assert.equal(live().length, 2);
  urls.close();
  assert.deepEqual(live(), []);
});

for (const failing of ["old", "new"]) {
  test(`the ${failing} side too big lets go of the other's URL`, async () => {
    reset();
    const urls = new Urls();
    const read = async (s: string) => {
      if (s === failing) throw tooBig;
      return small;
    };
    await assert.rejects(loadPair(["old", "new"], "image/png", false, urls, read, decode));
    // The side that drew may finish after the other failed.
    await later(20);
    assert.equal(made.length, 1);
    assert.deepEqual(live(), []);
  });
}

test("a side that finishes after the other failed lets go of its URL", async () => {
  reset();
  const urls = new Urls();
  const read = async (s: string) => {
    if (s === "old") throw tooBig;
    await later(10);
    return small;
  };
  await assert.rejects(loadPair(["old", "new"], "image/png", false, urls, read, decode));
  await later(30);
  assert.equal(made.length, 1);
  assert.deepEqual(live(), []);
});

test("both sides failing make no URL", async () => {
  reset();
  const urls = new Urls();
  await assert.rejects(loadPair(["old", "new"], "image/png", false, urls, async () => Promise.reject(tooBig), decode));
  assert.equal(made.length, 0);
});

test("closed while decoding, the URLs go", async () => {
  reset();
  const urls = new Urls();
  const slow = async () => {
    await later(20);
    return { width: 10, height: 10 };
  };
  const pair = loadPair(["old", "new"], "image/png", false, urls, async () => small, slow);
  await later(5);
  urls.close();
  await pair;
  assert.equal(made.length, 2);
  assert.deepEqual(live(), []);
});

test("too many pixels give the size, not a URL", async () => {
  reset();
  const urls = new Urls();
  const sides = await loadPair([null, "new"], "image/png", false, urls, async () => png(12000, 12000), decode);
  assert.deepEqual(sides, [null, { width: 12000, height: 12000 }]);
  assert.equal(made.length, 0);
});

test("AVIF and ICO sizes are read from their headers", () => {
  // ftyp, then an ispe box of a 64 × 64 thumbnail and one of the 9000 × 9000 picture.
  const avif = new Uint8Array(64);
  const view = new DataView(avif.buffer);
  avif.set([...("ftypavif")].map((c) => c.charCodeAt(0)), 4);
  const ispe = (at: number, w: number, h: number) => {
    avif.set([..."ispe"].map((c) => c.charCodeAt(0)), at);
    view.setUint32(at + 8, w);
    view.setUint32(at + 12, h);
  };
  ispe(16, 64, 64);
  ispe(36, 9000, 9000);
  assert.deepEqual(pictureSize(avif), { width: 9000, height: 9000 });
  const ico = new Uint8Array(38);
  ico.set([0, 0, 1, 0, 2, 0]);
  ico.set([16, 16], 6);
  ico.set([0, 0], 22);
  assert.deepEqual(pictureSize(ico), { width: 256, height: 256 });
});
