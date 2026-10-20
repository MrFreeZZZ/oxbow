// The Command Palette's list (⇧⌘P), as in VS Code: "Category: Title" with the shortcut beside
// it, the commands used lately first, and typing narrows it down by words.

import type { ShortcutId } from "./shortcuts";

export type PaletteItem = {
  id: string;
  /** "Git", "View"…; none for items that aren't commands, such as a recent repository. */
  category?: string;
  title: string;
  /** Grey text after the title, such as a repository's folder. */
  detail?: string;
  keys?: ShortcutId;
  run: () => void;
};

export const labelOf = (item: PaletteItem) => (item.category ? `${item.category}: ${item.title}` : item.title);

/** Where a word starts: the label's start, or after a space or sign. */
const starts = (label: string, i: number) => i === 0 || " :(-/.…“".includes(label[i - 1]);

/** How well the query fits the label, and the letters it matched; null when it doesn't. Each
 *  typed word must be in the label: best at the start of a word, else anywhere, else as the first
 *  letters of words in a row ("tsb" for Toggle Side Bar). */
export function fit(label: string, query: string): { score: number; hits: number[] } | null {
  const text = label.toLowerCase();
  const hits = new Set<number>();
  let score = 0;
  for (const word of query.toLowerCase().split(/\s+/).filter(Boolean)) {
    let at = -1;
    for (let i = text.indexOf(word); i >= 0; i = text.indexOf(word, i + 1)) {
      if (starts(text, i)) {
        at = i;
        break;
      }
      if (at < 0) at = i;
    }
    if (at >= 0) {
      score += starts(text, at) ? 3 : 2;
      for (let k = 0; k < word.length; k++) hits.add(at + k);
      continue;
    }
    const initials = [...text].flatMap((_, i) => (starts(text, i) && text[i] !== " " ? [i] : []));
    let found = -1;
    for (let s = 0; s + word.length <= initials.length && found < 0; s++) {
      if ([...word].every((ch, k) => text[initials[s + k]] === ch)) found = s;
    }
    if (found < 0) return null;
    score += 1;
    for (let k = 0; k < word.length; k++) hits.add(initials[found + k]);
  }
  return { score, hits: [...hits].sort((a, b) => a - b) };
}

/** The items to list for a query: the recent ones first when nothing is typed, else best fit first. */
export function rank(items: PaletteItem[], query: string, recent: string[]): { item: PaletteItem; hits: number[]; recent: boolean }[] {
  const order = new Map(recent.map((id, i) => [id, i]));
  const byRecent = (a: PaletteItem, b: PaletteItem) => (order.get(a.id) ?? Infinity) - (order.get(b.id) ?? Infinity);
  if (!query.trim()) {
    const used = items.filter((item) => order.has(item.id)).sort(byRecent);
    const rest = items.filter((item) => !order.has(item.id)).sort((a, b) => labelOf(a).localeCompare(labelOf(b)));
    return [...used.map((item) => ({ item, hits: [], recent: true })), ...rest.map((item) => ({ item, hits: [], recent: false }))];
  }
  return items
    .flatMap((item) => {
      const m = fit(labelOf(item), query);
      return m ? [{ item, hits: m.hits, recent: false, score: m.score }] : [];
    })
    .sort((a, b) => b.score - a.score || byRecent(a.item, b.item) || labelOf(a.item).localeCompare(labelOf(b.item)));
}

const RECENT_KEY = "oxbow.palette.recent";

export function recentCommands(): string[] {
  try {
    const list = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    return Array.isArray(list) ? list.filter((id) => typeof id === "string") : [];
  } catch {
    return [];
  }
}

export function rememberCommand(id: string) {
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify([id, ...recentCommands().filter((other) => other !== id)].slice(0, 5)));
  } catch {
    // Private mode or full storage: nothing is remembered.
  }
}
