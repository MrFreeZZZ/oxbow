// Screens opened from deep inside other components, such as File History from a diff's header.

export type FileMode = "changes" | "blame";

const RECENT_KEY = "oxbow.recentFiles";

function loadRecent(): Record<string, string[]> {
  try {
    return JSON.parse(localStorage.getItem(RECENT_KEY) ?? "{}") ?? {};
  } catch {
    return {};
  }
}

class Nav {
  /** File History's file and the commit to start on; null when it isn't open. */
  file = $state<{ path: string; commit: string | null } | null>(null);
  fileMode = $state<FileMode>("blame");
  /** Find in file: the text, whether case matters, how many matches File History shows and which one is current. */
  find = $state({ text: "", matchCase: false });
  found = $state({ count: 0, at: 0 });

  /** The next match, or the previous one, wrapping around. */
  findStep(by: 1 | -1) {
    const { count, at } = this.found;
    if (count) this.found = { count, at: (at + by + count) % count };
  }

  /** Quick Open (⌘P): pick a file to open File History on. */
  quickOpen = $state(false);
  /** Put the cursor in Find in file once File History is up, after Quick Open. */
  focusFind = $state(false);
  /** Files opened in File History lately, newest first, per repository. */
  recentFiles = $state<Record<string, string[]>>(loadRecent());
  /** The repository the recent files are kept for. */
  repo = "";

  openFile(path: string, commit: string | null = null) {
    this.file = { path, commit };
    if (!this.repo) return;
    const list = [path, ...(this.recentFiles[this.repo] ?? []).filter((p) => p !== path)].slice(0, 8);
    this.recentFiles = { ...this.recentFiles, [this.repo]: list };
    try {
      localStorage.setItem(RECENT_KEY, JSON.stringify(this.recentFiles));
    } catch {
      // Private mode or full storage: recent files last until the app quits.
    }
  }
}

export const nav = new Nav();
