// The user's settings, shared by every window: loaded from settings.json once, then kept in
// step through the backend's "settings-changed" event. The file holds only values that differ
// from the defaults below.

import { listen } from "@tauri-apps/api/event";
import { api } from "./api";

export const defaults = {
  "oxbow.appearance": "system" as "system" | "light" | "dark",
  "oxbow.history.rowStyle": "twoLines" as "twoLines" | "compact",
  "oxbow.startup.reopenRepository": true,
  "oxbow.confirm.enabled": true,
  "oxbow.confirm.scope": "all" as "all" | "risky",
  "oxbow.confirm.showCommand": true,
  "oxbow.push.confirmForce": true,
  "oxbow.diff.view": "changes" as "changes" | "full",
  "oxbow.diff.contextLines": 3,
  "oxbow.diff.wordHighlight": true,
  "oxbow.diff.ignoreWhitespace": false,
  "oxbow.text.font": "SF Mono" as "SF Mono" | "Menlo" | "JetBrains Mono",
  "oxbow.text.fontSize": 12,
  "oxbow.text.tabWidth": 4,
};

export type PrefKey = keyof typeof defaults;
export type PrefValue<K extends PrefKey> = (typeof defaults)[K];

/** Font stacks for code, with fallbacks for systems that don't have the font. */
const FONTS: Record<PrefValue<"oxbow.text.font">, string> = {
  "SF Mono": `"SF Mono", ui-monospace, Menlo, "Cascadia Mono", Consolas, "DejaVu Sans Mono", monospace`,
  Menlo: `Menlo, ui-monospace, "Cascadia Mono", Consolas, "DejaVu Sans Mono", monospace`,
  "JetBrains Mono": `"JetBrains Mono", ui-monospace, Menlo, "Cascadia Mono", Consolas, "DejaVu Sans Mono", monospace`,
};

class Prefs {
  #values = $state<Record<string, unknown>>({});
  /** True once settings.json has been read. */
  loaded = $state(false);

  constructor() {
    api.allSettings().then(
      (all) => {
        this.#values = { ...all, ...this.#values };
        this.loaded = true;
      },
      () => (this.loaded = true),
    );
    listen<{ key: string; value: unknown }>("settings-changed", (event) => {
      const { key, value } = event.payload;
      if (value === null) delete this.#values[key];
      else this.#values[key] = value;
    }).catch(() => {});
  }

  get<K extends PrefKey>(key: K): PrefValue<K> {
    const value = this.#values[key];
    const fallback = defaults[key];
    return (value !== undefined && typeof value === typeof fallback ? value : fallback) as PrefValue<K>;
  }

  /** Whether the setting differs from its default. */
  changed(key: PrefKey): boolean {
    return this.get(key) !== defaults[key];
  }

  /** Save a setting; its default value is saved as no value at all. */
  set<K extends PrefKey>(key: K, value: PrefValue<K>) {
    const stored = value === defaults[key] ? null : value;
    if (stored === null) delete this.#values[key];
    else this.#values[key] = stored;
    api.setSetting(key, stored).catch(() => {});
  }

  reset(key: PrefKey) {
    this.set(key, defaults[key]);
  }
}

export const prefs = new Prefs();

/** Read inside an effect that loads diffs, so it loads them again when Diff & Text changes how
 *  the backend makes them. */
export function diffSettings(): string {
  return `${prefs.get("oxbow.diff.contextLines")}:${prefs.get("oxbow.diff.ignoreWhitespace")}`;
}

/** Whether code views start on the whole file, from Diff & Text. */
export function wholeByDefault(): boolean {
  return prefs.get("oxbow.diff.view") === "full";
}

/** Light or dark, after following the system when the setting says so. */
export function effectiveLook(): "light" | "dark" {
  const appearance = prefs.get("oxbow.appearance");
  if (appearance !== "system") return appearance;
  return matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

/** Keep the page's light or dark look and its code font in step with the settings. */
export function followLook() {
  const root = document.documentElement;
  const media = matchMedia("(prefers-color-scheme: dark)");
  let system = $state(media.matches);
  media.addEventListener("change", () => (system = media.matches));
  $effect.root(() => {
    $effect(() => {
      const appearance = prefs.get("oxbow.appearance");
      root.dataset.theme = appearance === "system" ? (system ? "dark" : "light") : appearance;
    });
    $effect(() => {
      const size = prefs.get("oxbow.text.fontSize");
      root.style.setProperty("--code-font", FONTS[prefs.get("oxbow.text.font")] ?? FONTS["SF Mono"]);
      root.style.setProperty("--code-size", `${size}px`);
      root.style.setProperty("--code-line", `${Math.round(size * 1.65)}px`);
      root.style.setProperty("--tab", String(prefs.get("oxbow.text.tabWidth")));
    });
  });
}
