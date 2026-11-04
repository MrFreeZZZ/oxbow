// Each webview settles its own drafts before the backend may install. The page is inert
// during the handshake so a keystroke cannot introduce an unsaved edit after its answer.
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { confirm } from "./confirm.svelte";
import { prefs } from "./prefs.svelte";
import { draftSaves } from "./conflictDrafts";
import { settleReviewSaves } from "./reviewSaves";
import type { UpdateStatus } from "./types";

const guards = new Set<() => string | null>();
export function beforeUpdateRestart(guard: () => string | null) {
  guards.add(guard);
  return () => { guards.delete(guard); };
}

export const updateRestart = $state({ active: false, installing: false });

listen<UpdateStatus>("updates-changed", ({ payload }) => {
  updateRestart.active = payload.state === "preparing" || payload.state === "installing";
  updateRestart.installing = payload.state === "installing";
  document.getElementById("app")!.inert = updateRestart.active;
}).catch(() => {});

listen<number>("update-prepare-restart", async ({ payload: generation }) => {
  updateRestart.active = true;
  document.getElementById("app")!.inert = true;
  let problem: string | null = null;
  try {
    if (confirm.request || document.querySelector('[role="dialog"], dialog[open]')) problem = "Close unfinished dialogs before restarting to install the update.";
    else if (prefs.saving || prefs.failure) problem = "Finish saving your settings before restarting to install the update.";
    else problem = [...guards].map((guard) => guard()).find(Boolean) ?? null;
    if (!problem) {
      draftSaves.retry();
      const [conflicts, reviews] = await Promise.all([draftSaves.settle(3000), settleReviewSaves(3000)]);
      if (conflicts || reviews) problem = "Oxbow could not save a conflict or review draft. Save it before restarting to install the update.";
    }
  } catch {
    problem = "A window could not save its work. Save your edits and try restarting again.";
  }
  await api.updateRestartReady(generation, problem).catch(() => {});
}).catch(() => {});

// Window-level shortcut handlers also stop while the document is inert.
addEventListener("keydown", (event) => {
  if (updateRestart.active) { event.preventDefault(); event.stopImmediatePropagation(); }
}, true);
