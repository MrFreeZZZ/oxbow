// Screens opened from deep inside other components, such as File History from a diff's header.

export type FileMode = "changes" | "blame";

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

  openFile(path: string, commit: string | null = null) {
    this.file = { path, commit };
  }
}

export const nav = new Nav();
