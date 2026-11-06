import type { FileChange } from "./types";

export const changeBadge: Record<FileChange["status"], { text: string; tone: string; title: string }> = {
  modified: { text: "M", tone: "orange", title: "Modified" },
  added: { text: "A", tone: "green", title: "Added" },
  untracked: { text: "U", tone: "green", title: "New, not tracked yet" },
  deleted: { text: "D", tone: "red", title: "Deleted" },
  renamed: { text: "R", tone: "orange", title: "Renamed" },
  copied: { text: "C", tone: "orange", title: "Copied" },
  conflicted: { text: "!", tone: "red", title: "Conflicted" },
};

/** A checkbox gives the keyboard to the adjacent row or heading. */
export const keysTo = (event: MouseEvent, selector: string) => (event.currentTarget as HTMLElement).parentElement?.querySelector<HTMLElement>(selector)?.focus({ preventScroll: true });

/** Events from a group's heading; selection and repository actions stay in ChangesPanel. */
export interface ChangesGroupEvents {
  open: (event: MouseEvent) => void;
  toggle: () => void;
  selectAll: () => void;
  keyDown: (event: KeyboardEvent) => void;
  keyUp: (event: KeyboardEvent) => void;
  picked: () => unknown;
  all: () => unknown;
  discard?: () => unknown;
  menu?: (event: MouseEvent) => void;
}

/** Events from one row, including the original DOM event for focus and Shift ranges. */
export interface ChangesFileEvents {
  tick: (event: MouseEvent) => void;
  open: (event: MouseEvent) => void;
  drag: (event: PointerEvent) => void;
  menu: (event: MouseEvent) => void;
  discard: () => unknown;
  keyDown: (event: KeyboardEvent) => void;
  keyUp: (event: KeyboardEvent) => void;
}
