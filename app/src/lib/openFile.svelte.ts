import type { RepoScope } from "./repoScope";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { onMount, untrack } from "svelte";
import { menuIcons, type MenuEntry } from "./Menu.svelte";
import { keepWorkingFileStatuses, loadWorkingFileStatuses, workingFileAction, type WorkingFileStatus } from "./openFile";

export function openWorkingFile(scope: RepoScope, path: string, line: number | null = null) {
  return scope.api.openWorkingFile(path, line).catch((err) => scope.confirm.fail(err));
}

/** Refresh visible actions when the diff changes or something touches the working tree. */
export function watchWorkingFiles(scope: RepoScope, paths: () => string[]) {
  let statuses = $state.raw<ReadonlyMap<string, WorkingFileStatus>>(new Map());
  let version = $state(0);
  let failure = $state<WorkingFileStatus>();
  onMount(() => {
    const refresh = () => version++;
    const stop = scope.id === null ? getCurrentWindow().listen("outside-change", refresh).catch(() => {}) : null;
    scope.filesChanged.add(refresh);
    window.addEventListener("focus", refresh);
    return () => {
      window.removeEventListener("focus", refresh);
      scope.filesChanged.delete(refresh);
      void stop?.then((unlisten) => unlisten?.());
    };
  });
  let checked: ReadonlyMap<string, WorkingFileStatus> = new Map();
  let checkedVersion = -1;
  let checkedRepo: typeof scope.nav.repo;
  $effect(() => {
    const repo = scope.nav.repo;
    const change = checkedRepo !== repo ? "repo" : checkedVersion !== version ? "disk" : "none";
    checkedRepo = repo;
    checkedVersion = version;
    const files = new Set(paths());
    const shown = untrack(() => statuses);
    const kept = keepWorkingFileStatuses({ shown, checked }, files, change);
    checked = kept.checked;
    if (kept.shown.size !== shown.size) statuses = kept.shown;
    failure = undefined;
    if (files.size === checked.size) return;
    let current = true;
    void loadWorkingFileStatuses(files, checked, scope.api.workingFileStatuses).then(
      (next) => {
        if (!current) return;
        checked = next;
        statuses = next;
      },
      (err) => { if (current) failure = { kind: "unavailable", reason: String(err) }; },
    );
    return () => { current = false; };
  });
  return (path: string) => statuses.get(path) ?? failure;
}

/** Probe afresh for a menu, without waiting to show its other items. */
export function workingFileMenuItem(scope: RepoScope, path: string, historical: boolean): MenuEntry {
  let status = $state<WorkingFileStatus>();
  void scope.api.workingFileStatuses([path]).then(
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
    run: () => openWorkingFile(scope, path),
  };
}
