import { getCurrentWindow } from "@tauri-apps/api/window";
import { onMount } from "svelte";
import { api } from "./api";
import { confirm } from "./confirm.svelte";
import { menuIcons, type MenuEntry } from "./Menu.svelte";
import { workingFileAction, type WorkingFileStatus } from "./openFile";

export function openWorkingFile(path: string, line: number | null = null) {
  return api.openWorkingFile(path, line).catch((err) => confirm.fail(err));
}

async function readStatuses(paths: string[]): Promise<WorkingFileStatus[]> {
  try {
    return await api.workingFileStatuses(paths);
  } catch (err) {
    return paths.map(() => ({ kind: "unavailable", reason: String(err) }));
  }
}

/** Refresh visible actions when the diff changes or something touches the working tree. */
export function watchWorkingFiles(paths: () => string[]) {
  let statuses = $state.raw(new Map<string, WorkingFileStatus>());
  let version = $state(0);
  onMount(() => {
    const refresh = () => version++;
    const stop = getCurrentWindow().listen("outside-change", refresh).catch(() => {});
    window.addEventListener("focus", refresh);
    return () => {
      window.removeEventListener("focus", refresh);
      void stop.then((unlisten) => unlisten?.());
    };
  });
  $effect(() => {
    version;
    const files = paths();
    statuses = new Map();
    if (!files.length) return;
    let current = true;
    void readStatuses(files).then((results) => {
      if (current) statuses = new Map(files.map((path, i) => [path, results[i]]));
    });
    return () => { current = false; };
  });
  return (path: string) => statuses.get(path);
}

/** Probe afresh for a menu, without waiting to show its other items. */
export function workingFileMenuItem(path: string, historical: boolean): MenuEntry {
  let status = $state<WorkingFileStatus>();
  void readStatuses([path]).then(([result]) => (status = result));
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
