import { opts, text, withCurrent } from "../src/lib/settingsRows.ts";
import assert from "node:assert/strict";

const values = (options: { value: unknown }[]) => options.map((o) => o.value);

// A value typed into settings.json that no option has still shows, in its place by number.
assert.deepEqual(values(withCurrent(opts(10, 12, 14), 13)), [10, 12, 13, 14]);
assert.deepEqual(values(withCurrent(opts(10, 12, 14), 20)), [10, 12, 14, 20]);
assert.deepEqual(values(withCurrent(opts("SF Mono", "Menlo"), "Fira Code")), ["SF Mono", "Menlo", "Fira Code"]);

// A value the options have, or none at all, adds nothing.
assert.deepEqual(values(withCurrent(opts(10, 12), 12)), [10, 12]);
assert.deepEqual(values(withCurrent(opts(10, 12), "12")), [10, 12]);
assert.deepEqual(values(withCurrent(opts("a"), "")), ["a"]);

// A label is text or what a function says now.
assert.equal(text("Name"), "Name");
assert.equal(text(() => "Later"), "Later");
assert.equal(text(undefined), "");
