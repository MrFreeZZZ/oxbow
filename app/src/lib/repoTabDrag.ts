/** Geometry for reordering repository tabs. The gesture itself lives next to the strip. */

/** Chromium `View::ExceededDragThreshold` off Windows: 8px on either axis, so a click does not move a tab. */
export const DRAG_THRESHOLD = 8;

export interface TabSlot {
  /** Left edge in the tab list's scroll content. */
  left: number;
  width: number;
}

export function pastDragThreshold(dx: number, dy: number): boolean {
  return Math.abs(dx) > DRAG_THRESHOLD || Math.abs(dy) > DRAG_THRESHOLD;
}

/** Content x of the dragged tab's left edge, kept inside the strip. */
export function draggedTabLeft(contentX: number, grab: number, width: number, span: number): number {
  const max = Math.max(0, span - width);
  return Math.min(Math.max(0, contentX - grab), max);
}

/**
 * Where the dragged tab lands. `center` is its preview center; a resting tab counts once the
 * preview has passed that tab's center. The result is an index in the final list.
 */
export function tabDropIndex(center: number, slots: readonly TabSlot[], from: number): number {
  let to = 0;
  for (let i = 0; i < slots.length; i++) {
    if (i === from) continue;
    if (slots[i].left + slots[i].width / 2 < center) to++;
  }
  return to;
}

/** Content x of every tab after moving `from` to `to`, each tab keeping its own width. */
export function tabParkedLefts(slots: readonly TabSlot[], from: number, to: number, overlap: number): number[] {
  const order = slots.map((_, index) => index);
  if (from < 0 || from >= order.length) return slots.map(slot => slot.left);
  const [moved] = order.splice(from, 1);
  order.splice(Math.max(0, Math.min(to, order.length)), 0, moved);
  const lefts = new Array<number>(slots.length);
  let x = 0;
  order.forEach((index, place) => {
    lefts[index] = x;
    x += slots[index].width - (place === order.length - 1 ? 0 : overlap);
  });
  return lefts;
}

/** translateX for each tab. The dragged one follows `draggedLeft`; the others slide into the gap. */
export function tabDragOffsets(slots: readonly TabSlot[], from: number, to: number, draggedLeft: number, overlap: number): number[] {
  const parked = tabParkedLefts(slots, from, to, overlap);
  return parked.map((left, index) => (index === from ? draggedLeft : left) - slots[index].left);
}
