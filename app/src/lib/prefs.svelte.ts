// The user's settings, shared by every window: loaded from settings.json once, then kept in
// step through the backend's "settings-changed" event. The file holds only values that differ
// from the defaults below.

import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { BRANCH_PALETTES, branchPaletteById, branchVariables, type BranchPaletteId } from "./branchPalettes";
import { paletteOf, THEMES, themeById, themeVariables } from "./themes";

export const defaults = {
  "oxbow.appearance": "system" as "system" | "light" | "dark",
  "oxbow.history.rowStyle": "twoLines" as "twoLines" | "compact",
  "oxbow.startup.reopenRepository": true,
  /** Where Clone and New Repository put repositories; empty: ~/Developer or the like. */
  "oxbow.clone.folder": "",
  "oxbow.confirm.enabled": true,
  "oxbow.confirm.scope": "all" as "all" | "risky",
  "oxbow.confirm.showCommand": true,
  "oxbow.push.confirmForce": true,
  /** Days the Operation Log keeps its steps, so they can be undone. */
  "oxbow.undo.keepDays": 30,
  "oxbow.diff.view": "changes" as "changes" | "full",
  "oxbow.diff.contextLines": 3,
  "oxbow.diff.wordHighlight": true,
  "oxbow.diff.ignoreWhitespace": false,
  /** Which files of a commit start open in its diff. */
  "oxbow.diff.files": "smart" as "smart" | "expanded" | "collapsed",
  /** Smart folds files with more changed lines than this. */
  "oxbow.diff.foldOver": 300,
  /** Files with more changed lines than this wait for Show Diff Anyway; 0 shows every diff. */
  "oxbow.diff.maxLines": 5000,
  /** A font family; SF Mono is the system's monospaced font on macOS. */
  "oxbow.text.font": "SF Mono" as string,
  "oxbow.text.fontSize": 12,
  "oxbow.text.tabWidth": 4,
  "oxbow.git.englishOutput": true,
  "oxbow.git.path": "",
  "oxbow.git.runHooks": true,
  "oxbow.fetch.auto": true,
  /** Pull runs `git fetch <remote>` first, so the remote's other branches are current too. */
  "oxbow.pull.fetchFirst": true,
  /** Minutes between background fetches. */
  "oxbow.fetch.interval": 15,
  /** Characters before the commit summary counter warns; 0 is off. */
  "oxbow.commit.subjectGuide": 72,
  /** Empty: the first one installed. */
  "oxbow.openIn.editor": "",
  "oxbow.openIn.terminal": "",
  /** The Client ID of a GitHub OAuth app for signing in with the browser; empty: Oxbow's own. */
  "oxbow.github.clientId": "",
  "oxbow.theme": "oxbow",
  "oxbow.theme.variant": "auto" as "auto" | "light" | "dark",
  /** Colors of branches everywhere: graph, sidebar, capsules, stacks. */
  "oxbow.branchPalette": "mineral" as BranchPaletteId,
};

export type PrefKey = keyof typeof defaults;
export type PrefValue<K extends PrefKey> = (typeof defaults)[K];

/** What a setting may hold beyond the type of its default: the words a text setting knows, or
 *  the range of a number. */
type Rule = { oneOf?: readonly string[]; min?: number; max?: number; whole?: boolean };

export const rules: Partial<Record<PrefKey, Rule>> = {
  "oxbow.appearance": { oneOf: ["system", "light", "dark"] },
  "oxbow.history.rowStyle": { oneOf: ["twoLines", "compact"] },
  "oxbow.confirm.scope": { oneOf: ["all", "risky"] },
  "oxbow.undo.keepDays": { min: 1, max: 3650, whole: true },
  "oxbow.diff.view": { oneOf: ["changes", "full"] },
  "oxbow.diff.contextLines": { min: 0, max: 100, whole: true },
  "oxbow.diff.files": { oneOf: ["smart", "expanded", "collapsed"] },
  "oxbow.diff.foldOver": { min: 1, max: 100000, whole: true },
  "oxbow.diff.maxLines": { min: 0, max: 10000000, whole: true },
  "oxbow.text.fontSize": { min: 6, max: 72 },
  "oxbow.text.tabWidth": { min: 1, max: 16, whole: true },
  "oxbow.fetch.interval": { min: 1, max: 1440, whole: true },
  "oxbow.commit.subjectGuide": { min: 0, max: 500, whole: true },
  "oxbow.theme": { oneOf: THEMES.map((theme) => theme.id) },
  "oxbow.theme.variant": { oneOf: ["auto", "light", "dark"] },
  "oxbow.branchPalette": { oneOf: BRANCH_PALETTES.map((palette) => palette.id) },
};

/** Keys Oxbow keeps in settings.json by itself, e.g. the width of a panel, with their type. */
export const internal: Record<string, "number"> = { "oxbow.history.detailsWidth": "number" };

const named = (type: string) => (type === "null" ? "null" : type === "boolean" ? "true or false" : `${/^[aeiou]/.test(type) ? "an" : "a"} ${type}`);
const quoted = (words: readonly string[]) => words.map((word) => `"${word}"`).join(", ");

/** Whether settings.json may hold the key. */
export const knownSetting = (key: string) => key in defaults || key in internal;

/** What is wrong with a value of settings.json, or null when Oxbow can use it. */
export function problemOf(key: string, value: unknown): string | null {
  const expected = key in defaults ? typeof defaults[key as PrefKey] : internal[key];
  if (!expected) return `Oxbow has no setting “${key}”`;
  const actual = value === null ? "null" : Array.isArray(value) ? "array" : typeof value;
  if (actual !== expected) return `Expected ${named(expected)}, not ${named(actual)}`;
  const rule = rules[key as PrefKey];
  if (!rule) return null;
  if (rule.oneOf && !rule.oneOf.includes(value as string)) return `Expected one of ${quoted(rule.oneOf)}`;
  if (typeof value === "number") {
    if (rule.whole && !Number.isInteger(value)) return "Expected a whole number";
    if ((rule.min !== undefined && value < rule.min) || (rule.max !== undefined && value > rule.max)) return `Expected a number from ${rule.min ?? "−∞"} to ${rule.max ?? "∞"}`;
  }
  return null;
}

/** The font stack for code: the chosen family, then fallbacks for systems that don't have it. */
function fontStack(family: string): string {
  const name = family.replace(/["\\]/g, "").trim();
  return `${name ? `"${name}", ` : ""}ui-monospace, "SF Mono", Menlo, "Cascadia Mono", Consolas, "DejaVu Sans Mono", monospace`;
}

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
    // A value settings.json can't hold, e.g. typed in by hand, counts as the default.
    return (value !== undefined && !problemOf(key, value) ? value : defaults[key]) as PrefValue<K>;
  }

  /** Whether settings.json holds anything, so Restore Defaults has something to do. */
  get customized(): boolean {
    return Object.keys(this.#values).length > 0;
  }

  /** Whether the setting differs from its default. */
  changed(key: PrefKey): boolean {
    return this.get(key) !== defaults[key];
  }

  /** Save a setting; its default value is saved as no value at all. Resolves once the backend
   *  follows it too. */
  set<K extends PrefKey>(key: K, value: PrefValue<K>): Promise<void> {
    const stored = value === defaults[key] ? null : value;
    if (stored === null) delete this.#values[key];
    else this.#values[key] = stored;
    return api.setSetting(key, stored).catch(() => {});
  }

  reset(key: PrefKey): Promise<void> {
    return this.set(key, defaults[key]);
  }
}

export const prefs = new Prefs();

/** Read inside an effect that loads diffs, so it loads them again when Diff & Text changes how
 *  the backend makes them. */
export function diffSettings(): string {
  return `${prefs.get("oxbow.diff.contextLines")}:${prefs.get("oxbow.diff.ignoreWhitespace")}:${prefs.get("oxbow.diff.maxLines")}`;
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
    // The code theme, from Settings › Themes.
    $effect(() => {
      const appearance = prefs.get("oxbow.appearance");
      const look = appearance === "system" ? (system ? "dark" : "light") : appearance;
      const theme = themeById(prefs.get("oxbow.theme"));
      const { palette, dark } = paletteOf(theme, prefs.get("oxbow.theme.variant"), look);
      for (const [name, value] of Object.entries(themeVariables(theme, palette, dark, look))) {
        if (value) root.style.setProperty(name, value);
        else root.style.removeProperty(name);
      }
    });
    // Branch colors, from Settings › Themes › Branches & graph. They follow the window's look.
    $effect(() => {
      const appearance = prefs.get("oxbow.appearance");
      const look = appearance === "system" ? (system ? "dark" : "light") : appearance;
      for (const [name, value] of Object.entries(branchVariables(branchPaletteById(prefs.get("oxbow.branchPalette")), look))) {
        if (value) root.style.setProperty(name, value);
        else root.style.removeProperty(name);
      }
    });
    $effect(() => {
      const size = prefs.get("oxbow.text.fontSize");
      root.style.setProperty("--code-font", fontStack(prefs.get("oxbow.text.font")));
      root.style.setProperty("--code-size", `${size}px`);
      root.style.setProperty("--code-line", `${Math.round(size * 1.65)}px`);
      root.style.setProperty("--tab", String(prefs.get("oxbow.text.tabWidth")));
    });
  });
}
