import { listen } from "@tauri-apps/api/event";
import { api, settings } from "./api";
import { applyDetailsChrome, detailsChromeEqual, type DetailsChrome } from "./detailsChrome";

/** One copy for this window. Every repository tab reads and writes it. */
export const detailsChrome = $state<DetailsChrome>({
  panelWidth: null,
  detailsHidden: false,
  changesExpanded: false,
});

let epoch = 0;
let loading: Promise<void> | null = null;
let listening = false;

function readChrome(): DetailsChrome {
  return {
    panelWidth: detailsChrome.panelWidth,
    detailsHidden: detailsChrome.detailsHidden,
    changesExpanded: detailsChrome.changesExpanded,
  };
}

function writeChrome(next: DetailsChrome) {
  detailsChrome.panelWidth = next.panelWidth;
  detailsChrome.detailsHidden = next.detailsHidden;
  detailsChrome.changesExpanded = next.changesExpanded;
}

/** A drag or a toggle happened here, so a settings read already in flight must not put the old value back. */
export function noteDetailsChromeEdit() {
  epoch++;
}

function followSettings() {
  if (listening) return;
  listening = true;
  listen<{ key: string; value: unknown }>("settings-changed", ({ payload }) => {
    const next = applyDetailsChrome(readChrome(), payload.key, payload.value);
    if (detailsChromeEqual(readChrome(), next)) return;
    epoch++;
    writeChrome(next);
  }).catch(() => {
    listening = false;
  });
}

/** Read the saved panel once. A later edit, here or in another window, wins over this read. */
export function loadDetailsChrome() {
  if (loading) return loading;
  followSettings();
  const mine = epoch;
  loading = Promise.all([
    api.getSetting<number>(settings.detailsWidth),
    api.getSetting<number>(settings.changesWidth),
    api.getSetting<boolean>(settings.detailsHidden),
    api.getSetting<boolean>(settings.changesExpanded),
  ]).then(([details, changes, hidden, expanded]) => {
    if (mine !== epoch) return;
    let next = readChrome();
    const width = typeof details === "number" ? details : changes;
    if (typeof width === "number") next = applyDetailsChrome(next, settings.detailsWidth, width);
    if (typeof hidden === "boolean") next = applyDetailsChrome(next, settings.detailsHidden, hidden);
    if (typeof expanded === "boolean") next = applyDetailsChrome(next, settings.changesExpanded, expanded);
    if (mine !== epoch) return;
    writeChrome(next);
  }).catch(() => {
    loading = null;
  });
  return loading;
}
