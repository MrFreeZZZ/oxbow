import { opts, stepPopup, text, withCurrent } from "../src/lib/settingsRows.ts";
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

const choices = opts("Mineral", "Paper", "Signal");
function arrow(key: string, index: number, altKey = false) {
  const event = Object.assign(new Event("keydown", { cancelable: true }), { key, altKey }) as KeyboardEvent;
  const option = stepPopup(choices, index, event);
  return [option?.value, event.defaultPrevented];
}

assert.deepEqual(arrow("ArrowDown", 1), ["Signal", true]);
assert.deepEqual(arrow("ArrowUp", 1), ["Mineral", true]);
assert.deepEqual(arrow("ArrowUp", 0), [undefined, true]);
assert.deepEqual(arrow("ArrowDown", 2), [undefined, true]);
assert.deepEqual(arrow("ArrowDown", 1, true), [undefined, false]);
assert.deepEqual(arrow("Enter", 1), [undefined, false]);
