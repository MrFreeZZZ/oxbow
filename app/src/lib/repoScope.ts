import { getContext, setContext } from "svelte";
import { api, apiForSession, type RepoSessionId } from "./api";
import { Activity, activity } from "./activity.svelte";
import { ConfirmState, confirm } from "./confirm.svelte";
import { DraftSaves } from "./draftSaves";
import type { OutputLine } from "./types";
import { draftSaves } from "./conflictDrafts";
import { GitHubState, github } from "./github.svelte";
import { KeptScreens } from "./kept";
import { Lens, lens } from "./lens.svelte";
import { LfsState, lfs } from "./lfs.svelte";
import { Nav, nav } from "./nav.svelte";
import { OplogState, oplog } from "./oplog.svelte";
import { Search, search } from "./search.svelte";
import { Start, start } from "./start.svelte";
import { Tips, tips } from "./tips.svelte";

export function createRepoScope(id: RepoSessionId | null, isActive: () => boolean) {
  const filesChanged = new Set<() => void>();
  const kept = new KeptScreens();
  const clone = { line: null as ((line: OutputLine) => void) | null };
  if (id === null) {
    return { api, activity, confirm, draftSaves, github, lens, lfs, nav, oplog, search, start, tips, isActive, clone, id, filesChanged, kept };
  }
  const bound = apiForSession(id);
  const log = new OplogState(bound);
  const confirmation = new ConfirmState(bound, log, false);
  return {
    api: bound,
    activity: new Activity(bound, false),
    confirm: confirmation,
    draftSaves: new DraftSaves(bound),
    github: new GitHubState(bound),
    lens: new Lens(),
    lfs: new LfsState(bound),
    nav: new Nav(),
    oplog: log,
    search: new Search(bound),
    start: new Start(),
    tips: new Tips(confirmation),
    isActive,
    clone,
    filesChanged,
    kept,
    id,
  };
}

export type RepoScope = ReturnType<typeof createRepoScope>;

const key = Symbol("repository");
const legacy = createRepoScope(null, () => true);

export function provideRepoScope(scope: RepoScope) {
  setContext(key, scope);
}

export function useRepoScope(): RepoScope {
  return getContext<RepoScope>(key) ?? legacy;
}
