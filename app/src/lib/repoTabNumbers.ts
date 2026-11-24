// The first nine repository tabs, numbered from the left. ⌘1–⌘9 on the Mac and Ctrl+1–Ctrl+9
// elsewhere: fixed keys, kept out of Settings › Keyboard Shortcuts.

export const TAB_NUMBER_DELAY = 300;

const here = typeof navigator !== "undefined" && navigator.platform.startsWith("Mac");

type Key = Pick<KeyboardEvent, "key" | "code" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey">;

const digitOf = (code: string) => /^(?:Digit|Numpad)([1-9])$/.exec(code);

/** ⌘ on the Mac, Ctrl elsewhere, with no other modifier held. */
function switchHeld(event: Key, onMac: boolean) {
  return (onMac ? event.metaKey : event.ctrlKey) && !event.altKey && !event.shiftKey && !(onMac ? event.ctrlKey : event.metaKey);
}

/** 1–9 when the fixed tab key is pressed, or null. */
export function repoTabDigit(event: Key, onMac = here): number | null {
  const digit = digitOf(event.code);
  return digit && switchHeld(event, onMac) ? Number(digit[1]) : null;
}

const modifierAlone = (event: Key, onMac: boolean) =>
  (onMac ? event.key === "Meta" : event.key === "Control") && switchHeld(event, onMac);

export type TabNumberPhase = "idle" | "waiting" | "showing";

/** How a key changes the number badges. They appear only after the modifier is held on its own. */
export function nextTabNumberPhase(phase: TabNumberPhase, event: Key, type: "keydown" | "keyup", onMac: boolean): TabNumberPhase {
  if (type === "keyup") return switchHeld(event, onMac) ? (phase === "idle" ? "waiting" : phase) : "idle";
  if (modifierAlone(event, onMac)) return phase === "idle" ? "waiting" : phase;
  return phase === "showing" ? "showing" : "idle";
}
