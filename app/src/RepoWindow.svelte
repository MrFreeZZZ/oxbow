<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow, type CloseRequestedEvent } from "@tauri-apps/api/window";
  import { message } from "@tauri-apps/plugin-dialog";
  import { onDestroy, onMount, tick, type Snippet } from "svelte";
  import type App from "./App.svelte";
  import RepoTabView from "./RepoTabView.svelte";
  import RepoTabStrip from "./lib/RepoTabStrip.svelte";
  import { api, apiForSession, settings, type RepoSessionId } from "./lib/api";
  import { keepCommitMessage, keepMessagesForRestart } from "./lib/commitDrafts.svelte";
  import { FIRST_HISTORY_PAGE } from "./lib/historyLoad.svelte";
  import type { RepoScope } from "./lib/repoScope";
  import { loadDetailsChrome } from "./lib/detailsChrome.svelte";
  import { nextTabNumberPhase, TAB_NUMBER_DELAY, type TabNumberPhase } from "./lib/repoTabNumbers";
  import { prefs } from "./lib/prefs.svelte";
  import { DIALOG_IN_THE_WAY, isPending, readTabLayout, removeSavedWorktree, RepoTabs, saveTabLayout, type OpenFrom, type ShownTab } from "./lib/repoTabs";
  import { settleReviewSaves } from "./lib/reviewSaves";
  import { REPOSITORY_TAB_TAGS_GROUP } from "./lib/settingsNavigation";
  import { mac, type ShortcutId } from "./lib/shortcuts";
  import type { Gone } from "./lib/types";
  import { beforeUpdateRestart } from "./lib/updateRestart.svelte";
  import { updates } from "./lib/updates.svelte";

  let tabsEnabled = $state(false);
  let preparing = $state(0);
  let finishing = 0;
  let localClose = $state(false);
  const blocked = $derived(preparing !== 0 || localClose);
  let tabs = $state.raw<readonly ShownTab[]>([]);
  let activeId = $state<RepoSessionId | null>(null);
  let highlightedId = $state<RepoSessionId | null>(null);
  let home = $state<"closed" | "open" | "selected">("closed");
  const showingHome = $derived(home === "selected");
  let launched = $state(false);
  const main = getCurrentWindow().label === "main";
  let sideHidden = $state(false);
  const screens = new Map<RepoSessionId | null, { screen: App; scope: RepoScope }>();
  function register(scope: RepoScope, screen: App) {
    screens.set(scope.id, { screen, scope });
    return () => { screens.delete(scope.id); };
  }

  const activeScreen = () => screens.get(showingHome ? null : activeId)?.screen;
  const draftSaves = () => [...screens.values()].map(({ scope }) => scope.draftSaves);
  const noDialog = () => (activeScreen()?.canSwitch() ?? true) &&
    !document.querySelector('[aria-modal="true"]:not([hidden] *)');
  const canSwitch = () => !blocked && noDialog();
  const report = (error: unknown) => activeScreen()?.fileOpenError(String(error));

  let numberPhase = $state<TabNumberPhase>("idle");
  let numberTimer: ReturnType<typeof setTimeout> | undefined;
  function setNumberPhase(next: TabNumberPhase) {
    if (next === numberPhase) return;
    clearTimeout(numberTimer);
    numberTimer = undefined;
    numberPhase = next;
    if (next !== "waiting") return;
    numberTimer = setTimeout(() => {
      numberTimer = undefined;
      if (numberPhase === "waiting") numberPhase = "showing";
    }, TAB_NUMBER_DELAY);
  }
  function onTabModifier(event: KeyboardEvent) {
    if (!tabsEnabled || tabs.length < 2 || !canSwitch()) return setNumberPhase("idle");
    const type = event.type === "keyup" ? "keyup" : "keydown";
    setNumberPhase(nextTabNumberPhase(numberPhase, event, type, mac));
  }
  onDestroy(() => clearTimeout(numberTimer));
  $effect(() => {
    if (!tabsEnabled || tabs.length < 2) setNumberPhase("idle");
  });

  async function tabDraftProblem(id: RepoSessionId) {
    const scope = screens.get(id)?.scope;
    const tab = tabs.find(tab => tab.id === id);
    if (!tab) return null;
    const { path, name } = tab.repo;
    const busy = () => scope?.activity.hasQueuedChange
      ? `Oxbow can’t close ${name} while a Pull or Push is waiting.` : null;
    const queued = busy();
    if (queued) return queued;
    const commit = keepCommitMessage(path);
    scope?.draftSaves.retry();
    const [conflict, review] = await Promise.all([scope?.draftSaves.settle(3000), settleReviewSaves(3000, path)]);
    return commit ?? conflict ?? review ?? busy();
  }

  let eventsReady: Promise<void>;
  const manager = new RepoTabs({
    open: async (path, answer) => {
      await eventsReady;
      return api.openRepoTab(path, answer);
    },
    history: (id) => apiForSession(id).history(FIRST_HISTORY_PAGE),
    activate: (id) => apiForSession(id).activateRepoTab(),
    publish: (id) => apiForSession(id).activateRepoTab(true),
    close: (id) => apiForSession(id).closeRepo(true),
    closeBatch: (ids, active, previous) => api.closeRepoTabs(ids, active, previous),
  }, {
    show(next, active, highlighted) {
      if (main && launched && tabsEnabled && (active !== activeId || next.length !== tabs.length
        || next.some((tab, i) => tab.id !== tabs[i].id))) saveTabLayout(next, manager.layoutActive);
      tabs = next;
      activeId = active;
      highlightedId = highlighted;
    },
    repoSelected() {
      if (showingHome) home = "open";
    },
    restored: {
      live: () => true,
      trust: askOpenTrust,
      reopened: (id, repo) => screens.get(id)?.screen.reopenTab(id, repo),
      fail: async (_path, error) => { await tick(); report(error); },
    },
    canSwitch,
    switchProblem: () => noDialog() ? null : DIALOG_IN_THE_WAY,
    beforeClose: async (id) => {
      const problem = await tabDraftProblem(id);
      if (problem) throw new Error(problem);
      if (!noDialog()) throw new Error(DIALOG_IN_THE_WAY);
      return !preparing;
    },
    async whileClosing(run) {
      localClose = true;
      try { await run(); }
      finally { localClose = false; }
    },
    notice: (notice) => {
      void tick().then(() => {
        if (notice.event === "menu-command") {
          if (blocked) return;
          if (showingHome) return activeScreen()?.fromMenu(notice.payload as ShortcutId);
        }
        screens.get(notice.session)?.screen.notice(notice);
      });
    },
  });

  function askOpenTrust(...args: Parameters<OpenFrom["trust"]>) {
    return activeScreen()?.askOpenTrust(...args) ?? Promise.resolve(null);
  }
  function openPath(path: string, from: OpenFrom) {
    const source = activeScreen();
    return manager.open(path, {
      ...from,
      trust: askOpenTrust,
      fail: async (folder, error) => {
        await tick();
        if (source && source === activeScreen()) await from.fail(folder, error);
        else report(error);
      },
    }).catch(report);
  }

  export function openTabs() { return tabs; }
  export function activeTab() { return activeId; }
  export const openRepository = (path: string) => activeScreen()?.startup(path);
  export const activateTab = (id: RepoSessionId) => manager.activate(id).catch(report);
  export const closeTab = (id: RepoSessionId) => manager.close(id).catch(report);
  const moveTab = (id: RepoSessionId, to: number) => manager.move(id, to).catch(report);
  async function openSaved(path: string, restoring: boolean) {
    await tick();
    await (activeScreen() ?? screens.get(null)?.screen)?.startup(path, restoring);
  }
  const activeNow = () => showingHome ? null : activeId;
  function showHome() {
    if (!canSwitch()) return;
    home = "selected";
    manager.selectHome();
    void tick().then(() => document.querySelector<HTMLButtonElement>('[data-tab-id="home"][aria-selected="true"]')?.focus());
  }
  async function closeHome() {
    if (canSwitch()) home = "closed";
  }
  const closeOtherTabs = (id: RepoSessionId) => manager.closeOthers(id).catch(report);
  const stepTab = (by: 1 | -1) => manager.step(by, showingHome).catch(report);
  const jumpTab = (n: number) => {
    const tab = tabs[n - 1];
    if (tab) activateTab(tab.id);
  };

  async function lostTab(id: RepoSessionId, gone: Gone) {
    const shown = activeId === id;
    await tick();
    await manager.lose(id).catch(report);
    if (shown && activeId === null) screens.get(null)?.screen.showGone(gone);
  }

  let askingToClose = false;
  async function closeRequested(event: CloseRequestedEvent) {
    if (askingToClose || blocked) {
      api.cancelQuit().catch(() => {});
      return event.preventDefault();
    }
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
    let restoringLayout = false;
    const window = getCurrentWindow();
    void api.getSetting<boolean>(settings.sidebarHidden).then((hidden) => {
      if (alive && typeof hidden === "boolean") sideHidden = hidden;
    }).catch(() => {});
    void loadDetailsChrome();
    const guards = beforeUpdateRestart(async () => {
      const problem = keepMessagesForRestart();
      if (problem) return problem;
      const drafts = [...screens.values()].filter(({ scope }) => scope.id !== null).map(({ scope }) => scope.draftSaves);
      for (const save of drafts) save.retry();
      const failed = (await Promise.all(drafts.map(save => save.settle(3000)))).find(Boolean);
      return failed ? "Oxbow could not save a conflict draft. Save it before restarting to install the update." : null;
    });
    const stops = [
      window.onCloseRequested(closeRequested),
      window.listen<ShortcutId>("menu-command", ({ payload }) => {
        if (!blocked && activeId === null) activeScreen()?.fromMenu(payload);
      }),
      window.listen<string>("menu-open", ({ payload }) => { if (!blocked) activeScreen()?.recentFromMenu(payload); }),
      window.onFocusChanged(({ payload }) => { if (payload) activeScreen()?.windowFocused(); }),
      listen("account-changed", () => { for (const { screen } of screens.values()) screen.accountChanged(); }),
      listen<string>("trust-changed", ({ payload }) => { for (const { screen } of screens.values()) screen.trustChanged(payload); }),
      window.listen<string>("open-file-error", ({ payload }) => { if (activeId === null) report(payload); }),
      listen<string>("worktree-removed", ({ payload }) => {
        if (main) removeSavedWorktree(payload);
        void manager.removeRestored(payload).catch(report);
      }),
      window.listen<{ generation: number; closing: RepoSessionId[] }>("repo-tabs-prepare", async ({ payload }) => {
        const { generation, closing } = payload;
        if (generation <= finishing || preparing >= generation) return;
        preparing = generation;
        let problem: string | null = null;
        try {
          await manager.settled;
          await tick();
          if (!noDialog()) problem = "Close unfinished dialogs before hiding repository tabs.";
          else problem = (await Promise.all(closing.map(tabDraftProblem))).find(Boolean) ?? null;
        } catch (error) {
          problem = String(error);
        }
        if (alive && preparing === generation) await api.repoTabsReady(generation, problem).catch(report);
      }),
      listen<{ generation: number; enabled: boolean; closed: RepoSessionId[] }>("repo-tabs-finished", ({ payload }) => {
        if (payload.generation <= finishing) {
          if (payload.closed.length) void manager.setMode(tabsEnabled, payload.closed).catch(report);
          return;
        }
        finishing = payload.generation;
        if (preparing && preparing <= finishing) preparing = 0;
        const enabling = payload.enabled && !tabsEnabled;
        tabsEnabled = payload.enabled;
        if (!tabsEnabled) home = "closed";
        void manager.setMode(tabsEnabled, payload.closed).catch(report);
        if (main && launched && enabling) saveTabLayout(tabs, manager.layoutActive);
      }),
    ];
    eventsReady = api.repoEvents((notice) => { void manager.receive(notice)?.catch(report); });
    void Promise.all([eventsReady, ...stops]).then(async () => {
      const previous = finishing;
      const enabled = await api.repoTabsEnabled();
      if (!alive) return;
      if (previous === finishing) {
        tabsEnabled = enabled;
        await manager.setMode(enabled, [], main && !enabled ? readTabLayout() : null);
      }
      const { path, restoreTabs, explicit } = await api.initialRepo();
      if (!alive) return;
      const layout = restoreTabs ? readTabLayout() : null;
      if (layout && tabsEnabled) {
        restoringLayout = true;
        await manager.restore(layout, (folder) => openSaved(folder, true), explicit ? path : null);
      } else {
        const folder = path ?? layout?.active ?? layout?.paths.at(-1);
        if (folder) await openRepository(folder);
      }
    }).catch((error) => { if (alive) report(error); }).finally(() => {
      if (!alive) return;
      if (main && tabsEnabled && (tabs.length || restoringLayout)) saveTabLayout(tabs, manager.layoutActive);
      launched = true;
    });
    return () => {
      alive = false;
      manager.dispose();
      guards();
      for (const stop of stops) void stop.then((unlisten) => unlisten(), () => {});
    };
  });
</script>

<svelte:window onkeydown={onTabModifier} onkeyup={onTabModifier} onblur={() => setNumberPhase("idle")} />

<span class="sr-only" role="status">{updates.actionLabel ? updates.notice.title : ""}</span>

{#snippet tabStrip(openMenu: ((event: MouseEvent) => void) | undefined, openRecent: () => void, copyPath: (path: string) => void, update: Snippet | undefined)}
  {#if tabs.length > 0 || home !== "closed"}
    <RepoTabStrip {tabs} activeId={highlightedId} showNumbers={numberPhase === "showing"} {openMenu} {openRecent} {copyPath} {update}
      home={home !== "closed"} homeSelected={showingHome && highlightedId === activeId} {showHome} {closeHome}
      activate={activateTab} close={closeTab} closeOthers={closeOtherTabs}
      canSwitch={canSwitch} activeNow={activeNow} move={moveTab}
      saveTag={(path, id) => void prefs.saveRepositoryTabTag(path, id).catch(report)}
      customizeTags={() => void api.openSettings(REPOSITORY_TAB_TAGS_GROUP).catch(report)} />
  {/if}
{/snippet}

<RepoTabView {register} tab={null} active={showingHome || activeId === null} homePanel={home !== "closed"} {blocked} bind:sideHidden
  {launched} tabStrip={tabsEnabled && launched && (showingHome || (activeId === null && (tabs.length > 0 || home !== "closed"))) ? tabStrip : undefined}
  stepRepoTab={tabsEnabled && (showingHome ? tabs.length > 0 : tabs.length > 1) ? stepTab : undefined}
  jumpRepoTab={showingHome && tabs.length > 1 ? jumpTab : undefined}
  {openPath} />
{#each tabs as tab (tab.id)}
  {#if !isPending(tab)}
    <RepoTabView {register} {tab} active={!showingHome && activeId === tab.id} {blocked} {launched} bind:sideHidden
      tabStrip={tabsEnabled && !showingHome && activeId === tab.id ? tabStrip : undefined}
      showHome={tabsEnabled ? showHome : undefined}
      stepRepoTab={tabsEnabled && tabs.length > 1 ? stepTab : undefined}
      jumpRepoTab={tabsEnabled && tabs.length > 1 ? jumpTab : undefined}
      openPath={(path, from) => openPath(path, {
        ...from, live: () => from.live() && tabs.some(open => open.id === tab.id),
      })} closeTab={() => closeTab(tab.id)} lostTab={(gone) => lostTab(tab.id, gone)} />
  {/if}
{/each}

<style>
  .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }
</style>
