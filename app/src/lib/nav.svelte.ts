// Screens opened from deep inside other components, such as File History from a diff's header.

export type FileMode = "changes" | "blame";

class Nav {
  /** File History's file and the commit to start on; null when it isn't open. */
  file = $state<{ path: string; commit: string | null } | null>(null);
  fileMode = $state<FileMode>("blame");

  openFile(path: string, commit: string | null = null) {
    this.file = { path, commit };
  }
}

export const nav = new Nav();
