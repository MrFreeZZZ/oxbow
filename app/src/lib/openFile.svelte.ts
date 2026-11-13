import { getCurrentWindow } from "@tauri-apps/api/window";
import { onMount, untrack } from "svelte";
import { api } from "./api";
import { confirm } from "./confirm.svelte";
import { menuIcons, type MenuEntry } from "./Menu.svelte";
import { nav } from "./nav.svelte";
import { loadWorkingFileStatuses, workingFileAction, type WorkingFileStatus } from "./openFile";

export function openWorkingFile(path: string, line: number | null = null) {
  return api.openWorkingFile(path, line).catch((err) => confirm.fail(err));
}

/** Refresh visible actions when the diff changes or something touches the working tree. */
export function watchWorkingFiles(paths: () => string[]) {
  let statuses = $state.raw(new Map<string, WorkingFileStatus>());
  let version = $state(0);
  let failure = $state<WorkingFileStatus>();
  onMount(() => {
    const refresh = () => version++;
    const stop = getCurrentWindow().listen("outside-change", refresh).catch(() => {});
    window.addEventListener("focus", refresh);
    return () => {
      window.removeEventListener("focus", refresh);
      void stop.then((unlisten) => unlisten?.());
    };
  });
  let checkedVersion = -1;
  let checkedRepo: typeof nav.repo;
  $effect(() => {
    const repo = nav.repo;
    const refresh = checkedVersion !== version || checkedRepo !== repo;
    checkedRepo = repo;
    checkedVersion = version;
    const known = refresh ? new Map<string, WorkingFileStatus>() : untrack(() => statuses);
    const files = new Set(paths());
    const kept = new Map(known);
    for (const path of known.keys()) {
      if (!files.has(path)) kept.delete(path);
    }
    if (refresh || kept.size !== known.size) statuses = kept;
    failure = undefined;
    if (files.size === kept.size) return;
    let current = true;
    void loadWorkingFileStatuses(files, kept, api.workingFileStatuses).then(
      (next) => { if (current) statuses = next; },
      (err) => { if (current) failure = { kind: "unavailable", reason: String(err) }; },
    );
    return () => { current = false; };
  });
  return (path: string) => statuses.get(path) ?? failure;
}

/** Probe afresh for a menu, without waiting to show its other items. */
export function workingFileMenuItem(path: string, historical: boolean): MenuEntry {
  let status = $state<WorkingFileStatus>();
  void api.workingFileStatuses([path]).then(
    ([result]) => { status = result; },
    (err) => { status = { kind: "unavailable", reason: String(err) }; },
  );
  const action = $derived(workingFileAction(status, historical));
  return {
    kind: "item",
    icon: menuIcons.open,
    get label() { return action.label; },
    get detail() { return action.reason; },
    get off() { return action.reason !== undefined; },
    run: () => openWorkingFile(path),
  };
}
