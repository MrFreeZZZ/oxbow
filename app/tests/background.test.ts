import assert from "node:assert/strict";
import { backgroundOpacity, backgroundType as validate } from "../src/lib/background.ts";
import { MAX_PIXELS, pictureSize } from "../src/lib/pictures.ts";

const backgroundType = (data: Uint8Array) => validate(data, pictureSize(data), MAX_PIXELS);

// A file extension isn't permission to render active or unknown content as an image.
assert.throws(() => backgroundType(new TextEncoder().encode('<svg xmlns="http://www.w3.org/2000/svg"/>')), /JPEG, PNG or WebP/);
assert.throws(() => backgroundType(new Uint8Array([255, 216, 255])), /unreadable size/);

const png = (width: number, height: number) => {
  const bytes = new Uint8Array(24);
  bytes.set([137, 80, 78, 71, 13, 10, 26, 10]);
  bytes.set([73, 72, 68, 82], 12);
  const view = new DataView(bytes.buffer);
  view.setUint32(16, width);
  view.setUint32(20, height);
  return bytes;
};
assert.equal(backgroundType(png(6000, 4000)), "image/png");
assert.throws(() => backgroundType(png(8000, 8000)), /50 million pixels/);
assert.throws(() => backgroundType(png(0, 100)), /unreadable size/);

// Turning intensity off restores the original surfaces; limits protect both themes.
for (const dark of [true, false]) {
  assert.equal(backgroundOpacity(0, dark), 0);
  assert.equal(backgroundOpacity(-5, dark), 0);
  assert.equal(backgroundOpacity(200, dark), backgroundOpacity(100, dark));
  assert.ok(backgroundOpacity(35, dark) < backgroundOpacity(100, dark));
  assert.ok(backgroundOpacity(100, dark) <= 0.5);
}
