import assert from "node:assert/strict";
import { nextTabNumberPhase, repoTabDigit, TAB_NUMBER_DELAY } from "../src/lib/repoTabNumbers.ts";

assert.equal(TAB_NUMBER_DELAY, 300);

type Key = { key: string; code: string; metaKey?: boolean; ctrlKey?: boolean; altKey?: boolean; shiftKey?: boolean };
const key = (p: Key) => ({ metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...p });

assert.equal(repoTabDigit(key({ key: "1", code: "Digit1", metaKey: true }), true), 1);
assert.equal(repoTabDigit(key({ key: "9", code: "Digit9", metaKey: true }), true), 9);
assert.equal(repoTabDigit(key({ key: "1", code: "Numpad1", metaKey: true }), true), 1);
assert.equal(repoTabDigit(key({ key: "1", code: "Digit1", ctrlKey: true }), true), null);
assert.equal(repoTabDigit(key({ key: "1", code: "Digit1", metaKey: true, shiftKey: true }), true), null);
assert.equal(repoTabDigit(key({ key: "1", code: "Digit1", metaKey: true, altKey: true }), true), null);
assert.equal(repoTabDigit(key({ key: "1", code: "Digit1", metaKey: true, ctrlKey: true }), true), null);
assert.equal(repoTabDigit(key({ key: "0", code: "Digit0", metaKey: true }), true), null);
assert.equal(repoTabDigit(key({ key: "Meta", code: "MetaLeft", metaKey: true }), true), null);
assert.equal(repoTabDigit(key({ key: "1", code: "Digit1", ctrlKey: true }), false), 1);
assert.equal(repoTabDigit(key({ key: "1", code: "Digit1", metaKey: true }), false), null);
assert.equal(repoTabDigit(key({ key: "1", code: "Digit1", ctrlKey: true, altKey: true }), false), null);

const meta = key({ key: "Meta", code: "MetaLeft", metaKey: true });
const one = key({ key: "1", code: "Digit1", metaKey: true });
const released = key({ key: "Meta", code: "MetaLeft" });
assert.equal(nextTabNumberPhase("idle", meta, "keydown", true), "waiting");
assert.equal(nextTabNumberPhase("waiting", meta, "keydown", true), "waiting");
assert.equal(nextTabNumberPhase("waiting", one, "keydown", true), "idle");
assert.equal(nextTabNumberPhase("showing", one, "keydown", true), "showing");
assert.equal(nextTabNumberPhase("showing", one, "keyup", true), "showing");
assert.equal(nextTabNumberPhase("showing", released, "keyup", true), "idle");
assert.equal(nextTabNumberPhase("idle", one, "keyup", true), "waiting");

const ctrl = key({ key: "Control", code: "ControlLeft", ctrlKey: true });
const tab = key({ key: "Tab", code: "Tab", ctrlKey: true });
assert.equal(nextTabNumberPhase("idle", ctrl, "keydown", false), "waiting");
assert.equal(nextTabNumberPhase("waiting", tab, "keydown", false), "idle");
assert.equal(nextTabNumberPhase("idle", meta, "keydown", false), "idle");
