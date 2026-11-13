import assert from "node:assert/strict";
import { test } from "node:test";
import { resizeSplitter } from "../src/lib/resizeSplitter.ts";

for (const end of ["pointerup", "pointercancel", "lostpointercapture"]) {
  test(`the splitter clears its highlight and saves the width on ${end}`, () => {
    const attributes = new Set<string>();
    const document = new EventTarget();
    const target = Object.assign(new EventTarget(), {
      ownerDocument: document,
      setPointerCapture: (id: number) => assert.equal(id, 1),
      setAttribute: (name: string) => attributes.add(name),
      removeAttribute: (name: string) => attributes.delete(name),
    });
    const deltas: number[] = [];
    let saves = 0;

    resizeSplitter({
      button: 0, isPrimary: true, pointerId: 1, clientX: 100, currentTarget: target,
    } as unknown as PointerEvent, (delta) => deltas.push(delta), () => saves++);

    // A detached captured element sends lostpointercapture to the document.
    const send = (type: string, pointerId: number, clientX = 0) => {
      const lost = type === "lostpointercapture";
      const event = new Event(lost ? "lostpointercapture" : type);
      (lost ? document : target).dispatchEvent(Object.assign(event, { pointerId, clientX }));
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
