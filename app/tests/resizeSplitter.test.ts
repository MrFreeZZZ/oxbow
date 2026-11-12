import assert from "node:assert/strict";
import { test } from "node:test";
import { resizeSplitter } from "../src/lib/resizeSplitter.ts";

for (const end of ["pointerup", "pointercancel", "lostpointercapture"]) {
  test(`the splitter clears its highlight and saves the width on ${end}`, () => {
    const attributes = new Set<string>();
    const target = Object.assign(new EventTarget(), {
      setPointerCapture: (id: number) => assert.equal(id, 1),
      setAttribute: (name: string) => attributes.add(name),
      removeAttribute: (name: string) => attributes.delete(name),
    });
    const deltas: number[] = [];
    let saves = 0;

    resizeSplitter({
      button: 0, isPrimary: true, pointerId: 1, clientX: 100, currentTarget: target,
    } as unknown as PointerEvent, (delta) => deltas.push(delta), () => saves++);

    const send = (type: string, pointerId: number, clientX = 0) => {
      target.dispatchEvent(Object.assign(new Event(type), { pointerId, clientX }));
    };
    send("pointerleave", 1);
    send("pointermove", 2, 140);
    send(end, 2);
    send("pointermove", 1, 125);
    assert.deepEqual(deltas, [25]);
    assert.ok(attributes.has("data-resizing"));
    assert.equal(saves, 0);

    send(end, 1);
    send("lostpointercapture", 1);
    send("pointermove", 1, 150);
    assert.deepEqual(deltas, [25]);
    assert.ok(!attributes.has("data-resizing"));
    assert.equal(saves, 1);
  });
}
