// Welcome is the repository overview. Going back restores the working window's size.

import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";

const WELCOME = { width: 1360, height: 800, minWidth: 1100, minHeight: 560 };
const REPO = { width: 1360, height: 860, minWidth: 900, minHeight: 560 };
/** The size of a repository window when it last showed Welcome, to open the next repository in. */
const KEY = "oxbow.repoWindowSize";

let compact: boolean | null = null;

function saved(): { width: number; height: number } {
  try {
    const size = JSON.parse(localStorage.getItem(KEY) ?? "null");
    if (size && size.width >= REPO.minWidth && size.height >= REPO.minHeight) return size;
  } catch {
    // Nothing usable saved: the default size.
  }
  return { width: REPO.width, height: REPO.height };
}

/** Welcome when `welcome`, otherwise a repository's window. Leaves maximized and full-screen
 *  windows as they are, and does nothing outside Tauri (a browser preview). */
export async function fitWindow(welcome: boolean) {
  if (compact === welcome) return;
  const first = compact === null;
  compact = welcome;
  try {
    const win = getCurrentWindow();
    if ((await win.isMaximized()) || (await win.isFullscreen())) return;
    // A repository opened at launch keeps the size and place the window had last time, unless
    // that was Oxbow quitting from Welcome.
    if (first && !welcome && (await win.innerSize()).toLogical(await win.scaleFactor()).width >= REPO.minWidth) return;
    if (welcome) {
      const scale = await win.scaleFactor();
      const now = (await win.innerSize()).toLogical(scale);
      if (now.width >= REPO.minWidth) {
        try {
          localStorage.setItem(KEY, JSON.stringify({ width: Math.round(now.width), height: Math.round(now.height) }));
        } catch {
          // Without storage the next repository opens at the default size.
        }
      }
      await win.setMinSize(new LogicalSize(WELCOME.minWidth, WELCOME.minHeight));
      await win.setSize(new LogicalSize(WELCOME.width, WELCOME.height));
    } else {
      const size = saved();
      await win.setMinSize(new LogicalSize(REPO.minWidth, REPO.minHeight));
      await win.setSize(new LogicalSize(size.width, size.height));
    }
    await win.center();
  } catch {
    // Not in Tauri, or the window is gone.
  }
}
