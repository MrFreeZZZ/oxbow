<script lang="ts">
  import App from "./App.svelte";
  import { onMount, untrack, type ComponentProps } from "svelte";
  import WordPopover from "./lib/WordPopover.svelte";
  import { createRepoScope, provideRepoScope, type RepoScope } from "./lib/repoScope";
  import type { Gone } from "./lib/types";
  import type { OpenFrom, OpenTab } from "./lib/repoTabs";

  let { register, tab, active, launched, openPath, closeTab, lostTab, tabStrip, stepRepoTab, sideHidden = $bindable() }: {
    register: (scope: RepoScope, screen: App) => () => void;
    tab: OpenTab | null;
    active: boolean;
    launched: boolean;
    openPath?: (path: string, from: OpenFrom) => Promise<void>;
    closeTab?: () => Promise<void>;
    lostTab?: (gone: Gone) => Promise<void>;
    tabStrip?: ComponentProps<typeof App>["tabStrip"];
    stepRepoTab?: ComponentProps<typeof App>["stepRepoTab"];
    sideHidden: boolean;
  } = $props();

  let mounted = true;
  const scope = createRepoScope(untrack(() => tab?.id ?? null), () => mounted && active);
  provideRepoScope(scope);

  let screen: App;
  onMount(() => {
    const unregister = register(scope, screen);
    return () => {
      mounted = false;
      unregister();
    };
  });
</script>

<div class="repo-tab-view" class:active hidden={!active} inert={!active}
  id={tab ? `repo-panel-${tab.id}` : undefined} role={tab ? "tabpanel" : undefined} aria-label={tab?.repo.name}>
  <App bind:this={screen} openedTab={tab ?? undefined} {launched} {openPath} {closeTab} {lostTab} {tabStrip} {stepRepoTab} bind:sideHidden />
  <WordPopover />
</div>

<style>
  .repo-tab-view.active { display: contents; }
</style>
