<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow, type CloseRequestedEvent } from "@tauri-apps/api/window";
  import { message } from "@tauri-apps/plugin-dialog";
  import { onMount, tick } from "svelte";
  import type App from "./App.svelte";
  import RepoTabView from "./RepoTabView.svelte";
  import { api, apiForSession, type RepoSessionId } from "./lib/api";
  import { keepMessagesForRestart } from "./lib/commitDrafts.svelte";
  import { FIRST_HISTORY_PAGE } from "./lib/historyLoad.svelte";
  import type { RepoScope } from "./lib/repoScope";
  import { RepoTabs, type OpenTab } from "./lib/repoTabs";
  import { settleReviewSaves } from "./lib/reviewSaves";
  import type { ShortcutId } from "./lib/shortcuts";
  import type { Gone, OutsideChange } from "./lib/types";
  import { beforeUpdateRestart } from "./lib/updateRestart.svelte";

  // The tab UI will supply this at the window entry point.
  let { tabsEnabled = false }: { tabsEnabled?: boolean } = $props();
  let tabs = $state.raw<readonly OpenTab[]>([]);
  let activeId = $state<RepoSessionId | null>(null);
  let launched = $state(false);
  const screens = new Map<RepoSessionId | null, { screen: App; scope: RepoScope }>();
  function register(scope: RepoScope, screen: App) {
    screens.set(scope.id, { screen, scope });
    return () => { screens.delete(scope.id); };
  }
  const activeScreen = () => screens.get(activeId)?.screen;
  const draftSaves = () => [...screens.values()].map(({ scope }) => scope.draftSaves);
  const canSwitch = () => (activeScreen()?.canSwitch() ?? true) &&
    !document.querySelector('[aria-modal="true"]:not([inert] *)');
  const report = (error: unknown) => activeScreen()?.fileOpenError(String(error));

  async function tabDraftProblem(id: RepoSessionId) {
    const drafts = screens.get(id)?.scope.draftSaves;
    const path = tabs.find(tab => tab.id === id)?.repo.path;
    drafts?.retry();
    const [conflict, review] = await Promise.all([drafts?.settle(3000), path && settleReviewSaves(3000, path)]);
    return conflict ?? review;
  }

  let eventsReady: Promise<void>;
  const manager = new RepoTabs({
    open: async (path, answer) => {
      await eventsReady;
      return api.openRepoTab(path, answer);
    },
    history: (id) => apiForSession(id).history(FIRST_HISTORY_PAGE),
    activate: (id) => apiForSession(id).activateRepoTab(),
    close: (id) => apiForSession(id).closeRepo(),
  }, {
    show(next, active) {
      tabs = next;
      activeId = active;
    },
    canSwitch,
    beforeClose: async (id) => {
      const problem = await tabDraftProblem(id);
      if (!problem) return canSwitch();
      screens.get(id)?.screen.fileOpenError(problem);
      return false;
    },
    notice: (notice) => {
      void tick().then(() => screens.get(notice.session)?.screen.notice(notice));
    },
  });

  export function openTabs() { return tabs; }
  export function activeTab() { return activeId; }
  export const openRepository = (path: string) => activeScreen()?.startup(path);
  export const activateTab = (id: RepoSessionId) => manager.activate(id).catch(report);
  export const closeTab = (id: RepoSessionId) => manager.close(id).catch(report);

  async function lostTab(id: RepoSessionId, gone: Gone) {
    const shown = activeId === id;
    await tick();
    const problem = await tabDraftProblem(id);
    if (problem) {
      screens.get(id)?.screen.fileOpenError(problem);
      return;
    }
    await manager.lose(id).catch(report);
    if (shown && activeId === null) screens.get(null)?.screen.showGone(gone);
  }

  let askingToClose = false;
  async function closeRequested(event: CloseRequestedEvent) {
    if (askingToClose) return event.preventDefault();
    const drafts = draftSaves();
    for (const save of drafts) save.retry();
    const [conflicts, reviewWhy] = await Promise.all([
      Promise.all(drafts.map((save) => save.settle(3000))),
      settleReviewSaves(3000),
    ]);
    const conflictWhy = conflicts.find(Boolean);
    const why = conflictWhy ?? reviewWhy;
    if (why === null) return;
    event.preventDefault();
    askingToClose = true;
    const kind = conflictWhy ? "conflict" : "review";
    const answer = await message(`Oxbow couldn’t save your ${kind} draft: ${why.replace(/\.$/, "")}. Do you really want to close without it?`, {
      title: `Your ${kind} draft isn’t saved`,
      kind: "warning",
      buttons: { yes: "Try Again", no: "Close Without Saving", cancel: "Cancel" },
    }).catch(() => "Cancel");
    askingToClose = false;
    if (answer === "Close Without Saving") return void getCurrentWindow().destroy();
    if (answer === "Try Again") return void getCurrentWindow().close();
    api.cancelQuit().catch(() => {});
  }

  onMount(() => {
    let alive = true;
    const window = getCurrentWindow();
    const guards = beforeUpdateRestart(async () => {
      const problem = keepMessagesForRestart();
      if (problem) return problem;
      if (!tabsEnabled) return null;
      const drafts = [...screens.values()].filter(({ scope }) => scope.id !== null).map(({ scope }) => scope.draftSaves);
      for (const save of drafts) save.retry();
      const failed = (await Promise.all(drafts.map(save => save.settle(3000)))).find(Boolean);
      return failed ? "Oxbow could not save a conflict draft. Save it before restarting to install the update." : null;
    });
    const stops = [
      window.onCloseRequested(closeRequested),
      window.listen<ShortcutId>("menu-command", ({ payload }) => {
        if (!tabsEnabled || activeId === null) activeScreen()?.fromMenu(payload);
      }),
      window.listen<string>("menu-open", ({ payload }) => activeScreen()?.recentFromMenu(payload)),
      window.onFocusChanged(({ payload }) => { if (payload) activeScreen()?.windowFocused(); }),
      listen("account-changed", () => { for (const { screen } of screens.values()) screen.accountChanged(); }),
      listen<string>("trust-changed", ({ payload }) => { for (const { screen } of screens.values()) screen.trustChanged(payload); }),
    ];
    if (!tabsEnabled) stops.push(
      window.listen("repo-touched", () => activeScreen()?.repoTouched()),
      window.listen<OutsideChange>("outside-change", ({ payload }) => activeScreen()?.changedOutside(payload)),
      window.listen<string>("open-file-error", ({ payload }) => activeScreen()?.fileOpenError(payload)),
    );
    eventsReady = tabsEnabled
      ? api.repoEvents((notice) => { void manager.receive(notice)?.catch(report); })
      : Promise.resolve();
    void Promise.all([eventsReady, ...stops]).then(() => api.initialRepo()).then((path) => {
      if (alive && path) void openRepository(path);
    }).catch((error) => { if (alive) report(error); }).finally(() => { if (alive) launched = true; });
    return () => {
      alive = false;
      manager.dispose();
      guards();
      for (const stop of stops) void stop.then((unlisten) => unlisten(), () => {});
    };
  });
</script>

<RepoTabView {register} tab={null} active={activeId === null}
  {launched} openPath={tabsEnabled ? (path, from) => manager.open(path, from).catch(report) : undefined} />
{#each tabs as tab (tab.id)}
  <RepoTabView {register} {tab} active={activeId === tab.id} {launched}
    openPath={(path, from) => manager.open(path, {
      ...from, live: () => from.live() && tabs.some(open => open.id === tab.id),
    }).catch(report)} closeTab={() => closeTab(tab.id)} lostTab={(gone) => lostTab(tab.id, gone)} />
{/each}
