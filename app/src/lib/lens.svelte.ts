// A click on a word of a git command in a terminal block: what a subcommand or flag means, or
// which commit a branch name or SHA is, with that commit lit up in the graph behind.

import type { Meaning, Word } from "./gitWords";
import type { HistoryRow, RefInfo } from "./types";

/** What the popover shows. */
export type Look =
  | { kind: "word"; word: Word; meaning: Meaning; manual: string | null }
  | { kind: "ref"; name: string; ref: "local" | "remote" | "tag"; row: HistoryRow | null; color: number }
  | { kind: "commit"; text: string; row: HistoryRow };

/** What the open repository knows, set by the main window. */
interface Known {
  refs: RefInfo[];
  row: (id: string) => HistoryRow | undefined;
  /** Rows by the start of their SHA, for a short SHA. */
  rows: HistoryRow[];
  colorOf: (name: string) => number;
}

class Lens {
  /** The open popover and the word it hangs under. */
  open = $state.raw<{ look: Look; at: DOMRect; from: Element } | null>(null);
  #known = $state.raw<Known | null>(null);

  /** The commit to light up in the graph while the popover is open. */
  get pointed(): string | null {
    const look = this.open?.look;
    if (look?.kind === "commit") return look.row.id;
    if (look?.kind === "ref") return look.row?.id ?? null;
    return null;
  }

  /** The branch or tag name whose popover is open, lit up in the sidebar. */
  get pointedRef(): string | null {
    const look = this.open?.look;
    return look?.kind === "ref" ? look.name : null;
  }

  know(known: Known | null) {
    this.#known = known;
  }

  /** A branch, remote branch or tag name, or a SHA of a loaded commit; `main~1` and `abc123^`
   *  count as their base, since that's where the graph looks. */
  find(text: string): Look | null {
    const known = this.#known;
    if (!known) return null;
    const name = text.replace(/^['"]|['"]$/g, "").replace(/[~^]\d*$/, "");
    const ref =
      known.refs.find((r) => r.kind === "local" && r.name === name) ??
      known.refs.find((r) => r.kind === "remote" && r.name === name) ??
      known.refs.find((r) => r.kind === "tag" && (r.name === name || `refs/tags/${r.name}` === name));
    if (ref && (ref.kind === "local" || ref.kind === "remote" || ref.kind === "tag")) {
      const row = known.row(ref.target) ?? null;
      return { kind: "ref", name: ref.name, ref: ref.kind, row, color: ref.kind === "tag" ? (row?.graph.color ?? 0) : known.colorOf(ref.name) };
    }
    if (/^[0-9a-f]{7,40}$/.test(name)) {
      const row = known.row(name) ?? known.rows.find((r) => r.id.startsWith(name));
      if (row) return { kind: "commit", text: name, row };
    }
    return null;
  }

  /** Opens the popover under the word, or closes it when it is already open there. */
  toggle(look: Look, element: Element) {
    this.open = this.open?.from === element ? null : { look, at: element.getBoundingClientRect(), from: element };
  }

  close() {
    this.open = null;
  }
}

export const lens = new Lens();
