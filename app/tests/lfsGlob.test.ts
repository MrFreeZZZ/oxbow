import { matches } from "../src/lib/lfsGlob.ts";
import assert from "node:assert/strict";
import test from "node:test";

test("patterns match like .gitattributes", () => {
  assert.ok(matches("*.psd", "art/cover.psd"));
  assert.ok(!matches("*.psd", "art/cover.png"));
  assert.ok(matches("/assets/*.png", "assets/a.png"));
  assert.ok(!matches("/assets/*.png", "assets/deep/a.png"));
  assert.ok(matches("assets/**/*.png", "assets/a/b/c.png"));
  assert.ok(matches("assets/**/*.png", "assets/c.png"));
  assert.ok(matches("media/**", "media/x/y.mov"));
  assert.ok(!matches("media/**", "media"));
  assert.ok(matches("file?.bin", "file1.bin"));
  assert.ok(matches("*.[ch]", "src/a.h"));
  assert.ok(!matches("*.[!ch]", "src/a.h"));
  assert.ok(matches("img[0-9].png", "img7.png"));
  // A broken class is plain text, not an error.
  assert.ok(matches("a[z-a.bin", "a[z-a.bin"));
  assert.ok(!matches("a[z-a].bin", "ab.bin"));
});

test("a pattern made to backtrack answers at once", () => {
  const started = Date.now();
  assert.ok(!matches("*a".repeat(30) + "b", "a".repeat(60)));
  assert.ok(!matches("**/".repeat(30) + "nothere", Array(40).fill("d").join("/") + "/file.rs"));
  assert.ok(Date.now() - started < 1000);
});
