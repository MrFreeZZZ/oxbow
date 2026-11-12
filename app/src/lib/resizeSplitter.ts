export function resizeSplitter(event: PointerEvent, resize: (delta: number) => void, save: () => void) {
  if (event.button !== 0 || !event.isPrimary) return;
  const target = event.currentTarget as HTMLElement;
  const { clientX, pointerId } = event;
  target.setPointerCapture(pointerId);
  target.setAttribute("data-resizing", "");

  const move = (next: PointerEvent) => {
    if (next.pointerId === pointerId) resize(next.clientX - clientX);
  };
  const finish = (next: PointerEvent) => {
    if (next.pointerId !== pointerId) return;
    target.removeEventListener("pointermove", move);
    target.removeEventListener("pointerup", finish);
    target.removeEventListener("pointercancel", finish);
    target.removeEventListener("lostpointercapture", finish);
    target.removeAttribute("data-resizing");
    save();
  };
  target.addEventListener("pointermove", move);
  target.addEventListener("pointerup", finish);
  target.addEventListener("pointercancel", finish);
  target.addEventListener("lostpointercapture", finish);
}
