// Settings previews reach the other webviews immediately; only persistence waits for a pause.
import { emit, listen } from "@tauri-apps/api/event";
import { prefs } from "./prefs.svelte";

export const backgroundIntensity = $state({ value: null as number | null, saving: false });
const origin = crypto.randomUUID();
let sequence = 0;
let timer: ReturnType<typeof setTimeout> | undefined;
let pending: number | null = null;
let sends = Promise.resolve();
let saving = Promise.resolve();
const seen = new Map<string, number>();

type Preview = { origin: string; sequence: number; value: number | null };
listen<Preview>("background-intensity-preview", async ({ payload }) => {
  if (payload.origin === origin || payload.sequence <= (seen.get(payload.origin) ?? -1)) return;
  if (payload.value !== null && (!Number.isInteger(payload.value) || payload.value < 0 || payload.value > 100)) return;
  seen.set(payload.origin, payload.sequence);
  if (payload.value === null) {
    // The final settings event and this frontend event use different IPC paths. Read the
    // committed value before dropping the overlay, even if its event is still in transit.
    await prefs.reload();
    if (seen.get(payload.origin) !== payload.sequence) return;
  }
  backgroundIntensity.value = payload.value;
}).catch(() => {});

function preview(value: number | null) {
  backgroundIntensity.value = value;
  const payload = { origin, sequence: ++sequence, value };
  sends = sends.then(() => emit("background-intensity-preview", payload)).catch(() => {});
}

export function previewIntensity(value: number) {
  pending = value;
  backgroundIntensity.saving = true;
  preview(value);
  clearTimeout(timer);
  timer = setTimeout(() => { void saveIntensity(); }, 200);
}

/** Flush on change, blur and window close, including a write already in flight. */
export function saveIntensity(): Promise<void> {
  clearTimeout(timer);
  if (pending === null) return saving;
  const value = pending;
  pending = null;
  const turn = sequence;
  const save = prefs.set("oxbow.background.intensity", value);
  saving = save.then(async () => {
    if (turn !== sequence) return;
    preview(null);
    const cleared = sequence;
    // Closing Settings must also deliver the final clear event before its webview goes away.
    await sends;
    if (cleared === sequence) backgroundIntensity.saving = false;
  });
  return saving;
}
