import { tick } from "svelte";
import { draggedTabLeft, pastDragThreshold, tabDropIndex, tabParkedLefts, type TabSlot } from "./repoTabDrag";

/**
 * One drag for this window. Pressing a tab selects it, and the strip is remounted in the
 * newly active repository, so the gesture cannot live on that component. Movement starts
 * the drag only after Chromium's slop.
 */
export const tabGesture = $state<{ current: TabGesture | null }>({ current: null });

const SLIDE_MS = 200;
const EDGE = 28;

export interface TabGesture {
  id: number;
  pointerId: number;
  originKey: string;
  clientX: number;
  clientY: number;
  startX: number;
  startY: number;
  grab: number;
  from: number;
  to: number;
  left: number;
  slots: TabSlot[];
  overlap: number;
  scroll: number;
  dragging: boolean;
  settling: boolean;
  arming: boolean;
  released: boolean;
}

interface DragHooks {
  canSwitch(): boolean;
  activate(): Promise<void>;
  isCurrent(): boolean;
  move(to: number): Promise<void>;
}

let dropMove: DragHooks["move"] | null = null;
let stillCurrent: DragHooks["isCurrent"] | null = null;
let listening = false;
let ending = false;
let frame = 0;

function tabList(): HTMLElement | null {
  return document.querySelector("[data-repo-tab-list]");
}

function measure(list: HTMLElement): TabSlot[] {
  const base = list.getBoundingClientRect().left;
  const scroll = list.scrollLeft;
  return [...list.querySelectorAll<HTMLElement>(":scope > .item")].map(item => {
    const box = item.getBoundingClientRect();
    return { left: box.left - base + scroll, width: box.width };
  });
}

function overlapOf(slots: readonly TabSlot[]): number {
  if (slots.length < 2) return 18;
  const overlap = slots[0].left + slots[0].width - slots[1].left;
  return overlap > 0 && overlap < slots[0].width ? Math.round(overlap) : 18;
}

function wait(ms: number): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms));
}

function place(gesture: TabGesture, list: HTMLElement) {
  const slot = gesture.slots[gesture.from];
  const last = gesture.slots[gesture.slots.length - 1];
  if (!slot || !last) return;
  const contentX = gesture.clientX - list.getBoundingClientRect().left + list.scrollLeft;
  gesture.left = draggedTabLeft(contentX, gesture.grab, slot.width, last.left + last.width);
  gesture.to = tabDropIndex(gesture.left + slot.width / 2, gesture.slots, gesture.from);
  gesture.scroll = list.scrollLeft;
}

function edgeScroll(list: HTMLElement, clientX: number) {
  const box = list.getBoundingClientRect();
  const intoLeft = EDGE - (clientX - box.left);
  const intoRight = EDGE - (box.right - clientX);
  if (intoLeft > 0) list.scrollLeft -= Math.ceil(intoLeft / 2);
  else if (intoRight > 0) list.scrollLeft += Math.ceil(intoRight / 2);
}

function onFrame() {
  frame = 0;
  const gesture = tabGesture.current;
  const list = gesture?.dragging && !gesture.settling ? tabList() : null;
  if (!gesture || !list) return;
  edgeScroll(list, gesture.clientX);
  place(gesture, list);
  frame = requestAnimationFrame(onFrame);
}

/** The click follows pointerup, so this stays up after the other listeners come off. */
function swallowClick(event: MouseEvent) {
  event.preventDefault();
  event.stopPropagation();
  window.removeEventListener("click", swallowClick, true);
}

function attach() {
  if (listening) return;
  listening = true;
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
  window.addEventListener("pointercancel", onCancel);
  window.addEventListener("keydown", onKey);
  window.removeEventListener("click", swallowClick, true);
  window.addEventListener("click", swallowClick, true);
}

function detach() {
  if (!listening) return;
  listening = false;
  if (frame) cancelAnimationFrame(frame);
  frame = 0;
  window.removeEventListener("pointermove", onMove);
  window.removeEventListener("pointerup", onUp);
  window.removeEventListener("pointercancel", onCancel);
  window.removeEventListener("keydown", onKey);
}

export function cancelTabDrag() {
  ending = false;
  detach();
  window.removeEventListener("click", swallowClick, true);
  dropMove = null;
  stillCurrent = null;
  tabGesture.current = null;
}

/** Drop a press or an in-progress drag when the open tabs change under it. */
export function noteTabOrder(key: string) {
  const gesture = tabGesture.current;
  if (gesture && !gesture.settling && gesture.originKey !== key) cancelTabDrag();
}

async function startDragging(gesture: TabGesture) {
  gesture.arming = true;
  await tick();
  if (gesture.released || tabGesture.current !== gesture) return;
  const list = tabList();
  const slots = list ? measure(list) : [];
  const item = list?.querySelector<HTMLElement>(`[data-tab-id="${gesture.id}"]`)?.parentElement;
  if (!list || !item || slots.length !== gesture.originKey.split("\0").length) {
    cancelTabDrag();
    return;
  }
  gesture.slots = slots;
  gesture.overlap = overlapOf(slots);
  gesture.grab = gesture.clientX - item.getBoundingClientRect().left;
  gesture.scroll = list.scrollLeft;
  gesture.dragging = true;
  place(gesture, list);
  if (!frame) frame = requestAnimationFrame(onFrame);
}

function onMove(event: PointerEvent) {
  const gesture = tabGesture.current;
  if (!gesture || event.pointerId !== gesture.pointerId || gesture.settling) return;
  if (gesture.dragging && event.buttons === 0) {
    void finish(true);
    return;
  }
  gesture.clientX = event.clientX;
  gesture.clientY = event.clientY;
  if (!gesture.dragging) {
    if (gesture.arming || !pastDragThreshold(event.clientX - gesture.startX, event.clientY - gesture.startY)) return;
    void startDragging(gesture);
    return;
  }
  const list = tabList();
  if (list) place(gesture, list);
}

function onUp(event: PointerEvent) {
  const gesture = tabGesture.current;
  if (!gesture || event.pointerId !== gesture.pointerId) return;
  if (gesture.dragging) void finish(true);
  else {
    gesture.released = true;
    detach();
    if (stillCurrent?.()) tabGesture.current = null;
  }
}

function onCancel(event: PointerEvent) {
  const gesture = tabGesture.current;
  if (!gesture || event.pointerId !== gesture.pointerId) return;
  if (gesture.dragging) void finish(false);
  else cancelTabDrag();
}

function onKey(event: KeyboardEvent) {
  const gesture = tabGesture.current;
  if (event.key !== "Escape" || !gesture?.dragging || gesture.settling) return;
  event.preventDefault();
  void finish(false);
}

async function finish(commit: boolean) {
  const gesture = tabGesture.current;
  if (!gesture || ending) return;
  if (!gesture.dragging) {
    cancelTabDrag();
    return;
  }
  ending = true;
  detach();
  try {
    if (!commit) gesture.to = gesture.from;
    const parked = tabParkedLefts(gesture.slots, gesture.from, gesture.to, gesture.overlap)[gesture.from];
    const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (!reduced && Math.abs(parked - gesture.left) >= 1) {
      gesture.settling = true;
      await wait(32);
      if (tabGesture.current !== gesture) return;
      gesture.left = parked;
      await wait(SLIDE_MS);
    }
    if (tabGesture.current !== gesture) return;
    if (commit && gesture.to !== gesture.from) await dropMove?.(gesture.to);
  } finally {
    ending = false;
    if (tabGesture.current === gesture) tabGesture.current = null;
    dropMove = null;
    stillCurrent = null;
  }
}

/** Select the tab on press. Movement after the slop reorders it. */
export function beginTabDrag(event: PointerEvent, id: number, ids: readonly number[], hooks: DragHooks) {
  if (event.button !== 0 || event.ctrlKey || ids.length < 2 || !ids.includes(id) || !hooks.canSwitch()) return;
  if (tabGesture.current?.dragging) return;
  const from = ids.indexOf(id);
  tabGesture.current = {
    id,
    pointerId: event.pointerId,
    originKey: ids.join("\0"),
    clientX: event.clientX,
    clientY: event.clientY,
    startX: event.clientX,
    startY: event.clientY,
    grab: 0,
    from,
    to: from,
    left: 0,
    slots: [],
    overlap: 18,
    scroll: 0,
    dragging: false,
    settling: false,
    arming: false,
    released: false,
  };
  const gesture = tabGesture.current;
  dropMove = hooks.move;
  stillCurrent = hooks.isCurrent;
  attach();
  void hooks.activate().then(() => {
    if (tabGesture.current !== gesture) return;
    if (!hooks.isCurrent()) cancelTabDrag();
    else if (gesture.released && !gesture.dragging) tabGesture.current = null;
  });
}
