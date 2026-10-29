<script lang="ts">
  // The Pull Request screen of one branch: its stack as a strip of soft arrows, then the pull
  // request (conversation, commits or files to review) on the left and whether it can merge,
  // and how, on the right, with the one step it waits for next on top. Another account's pull
  // request, or a fork's, is picked by its number. A branch without one gets the form that
  // opens it. After the base of a stack merged, the rest of the stack can be restacked onto the
  // trunk from here, in three steps kept between runs: a push that fails goes on later. A merge
  // request on GitLab shows here the same way, in GitLab's words.
  import { untrack } from "svelte";
  import { api } from "./api";
  import { github } from "./github.svelte";
  import { wordsOf, type ForgeKind } from "./forge";
  import type { Request } from "./confirm.svelte";
  import type { BranchContext } from "./branches";
  import type { Action, GitLabCreateFacts, GitLabUser, History, MergeMethodName, Landing, PullCheck, PullEntry, PullRequest, PullSummary, Restack, RestackState, ReviewState, Rewrite, Stack, StackPreview } from "./types";
  import type { SeriesQuery } from "./series";
  import { lane, person, plate, relativeTime, shortId, splitPath, tint } from "./format";
  import PickChips, { type Pick } from "./PickChips.svelte";
  import { defaultTemplate, exactPerson, matchLabels, squashChosen, squashStarts } from "./mrForm";
  import { branchMergeRequest } from "./mergeRequest";
  import { kept } from "./kept";
  import PullFiles from "./PullFiles.svelte";
  import Menu, { menuIcons, type MenuEntry } from "./Menu.svelte";
  import ReviewThread from "./ReviewThread.svelte";
  import FinishReview from "./FinishReview.svelte";
  import { ReviewSession } from "./review.svelte";
  import { countWords, spotLabel } from "./reviewDraft";
  import { pullWorktreeRequest, worktreeOf } from "./worktrees";
  import { softArrows } from "./shapes";
  import { editStackRequest, freshDraft, planOf } from "./stack";
  import { switchRequest } from "./branches";
  import { mergeRequest as mergeIntoRequest } from "./merge";
  import { blockers, nextStep, restackFinished, restackLines, restackTouches, stillToRetarget, type RestackLine } from "./nextStep";
  import {
    autoMergeRequest,
    closeRequest,
    createRequest,
    defaultMethod,
    draftRequest,
    mergeMessage,
    mergeRequest,
    METHODS,
    methodsOf,
    targetRequest,
    readBody,
    readyRequest,
    restackPushRequest,
    reviewersRequest,
    updateBranchRequest,
    type NewPull,
    type StackLine,
  } from "./pr";

  let {
    branch,
    number = null,
    forge = "github",
    history,
    ctx,
    colorOf,
    version,
    run,
    onBranch,
    copy,
    onSeries,
  }: {
    branch: string;
    /** The pull request by its number, when it is no branch's here. */
    number?: number | null;
    /** `gitlab`: the branch's merge request on GitLab. */
    forge?: ForgeKind;
    history: History;
    ctx: BranchContext;
    colorOf: (name: string) => number;
    /** Goes up on every reload, so the pull request loads again. */
    version: number;
    run: (request: Request | Promise<Request>) => Promise<boolean>;
    /** Show another branch of the stack. */
    onBranch: (name: string) => void;
    copy: (text: string) => void;
    /** The branch's commits before and after its latest rewrite. */
    onSeries: (query: SeriesQuery) => void;
  } = $props();

  const HEIGHT = 42;

  const lab = $derived(forge === "gitlab");
  const w = $derived(wordsOf(forge));
  const gh = $derived(lab ? null : github.repo);
  /** Where it lives: the remote, and the account that owns the repository on GitHub. */
  const place = $derived(lab ? (github.mrProject ? { remote: github.mrProject.remote, owner: null as string | null } : null) : gh ? { remote: gh.remote, owner: gh.owner as string | null } : null);
  const pulls = $derived(github.listOf(forge));
  const summary = $derived(number ? github.byNumber(number, forge) : github.pullOf(branch, forge));
  const signedIn = $derived(lab ? github.mrMe : github.me);
  let pr = $state<PullRequest | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let stack = $state<Stack | null>(null);
  let tab = $state<"conversation" | "commits" | "files">("conversation");
  let method = $state<MergeMethodName | null>(null);
  let deleteBranch = $state(true);
  let me = $state<string | null>(null);
  let stripWidth = $state(0);

  api.githubAccount().then((account) => (me = account?.login ?? null), () => {});

  /** The latest rewrite of the branch: rewritten here and not pushed yet, or force pushed. */
  let rewrite = $state<Rewrite | null>(null);
  $effect(() => {
    void version;
    const name = branch;
    let latest = true;
    api.lastRewrite(name).then(
      (r) => latest && (rewrite = r),
      () => latest && (rewrite = null),
    );
    return () => {
      latest = false;
    };
  });

  const rewriteText = $derived(
    !rewrite
      ? ""
      : rewrite.source === "notPushed"
        ? "Rewritten here since the last push"
        : `${rewrite.source === "pushed" ? "Force pushed from here" : "Force pushed by someone else"}${rewrite.time ? ` ${relativeTime(rewrite.time)}` : ""}`,
  );

  function openSeries() {
    if (!rewrite) return;
    onSeries({ branch, old: rewrite.old, new: rewrite.new, rewritten: rewrite.rewritten, branches: [], from: rewriteText });
  }

  // The pull request, again after every change; GitHub works out mergeability a moment later.
  let checksAgain = 0;
  $effect(() => {
    const number = summary?.number;
    void version;
    if (!number) {
      pr = null;
      return;
    }
    // `load` reads `pr`; tracking it here would load again after every load.
    untrack(() => load(number));
  });

  /** Reviewing it: the draft, the Viewed ticks, the comment being written. */
  let review = $state<ReviewSession | null>(null);
  $effect(() => {
    const p = pr;
    untrack(() => {
      // Reviewing a merge request comes with GitLab's review calls.
      if (!p || p.gitlab) review = null;
      else if (review?.number === p.number) review.pr = p;
      else review = new ReviewSession(p, run, () => load(p.number));
    });
  });
  let finishAt = $state<HTMLButtonElement>();
  const mine = $derived(!!pr && !!signedIn && pr.author.login === signedIn);
  /** Finish Review shows on someone else's open pull request, and on one's own with comments to send. */
  const reviewable = $derived(!!review && !!github.me && pr?.state === "open" && (!mine || review.count > 0));

  async function load(number: number) {
    loading = !pr || pr.number !== number;
    try {
      const fresh = await (lab ? api.gitlabMergeRequest(number) : api.githubPull(number));
      if (summary?.number !== number) return;
      pr = fresh;
      error = null;
      if (fresh.state === "open" && fresh.mergeableState === "unknown" && checksAgain < 3) {
        checksAgain++;
        setTimeout(() => summary?.number === number && load(number), 2500);
      } else checksAgain = 0;
    } catch (err) {
      error = String(err);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void version;
    const name = branch;
    api.stack(name).then(
      (s) => name === branch && (stack = s),
      () => name === branch && (stack = null),
    );
  });

  type Left = { tab: typeof tab; method: typeof method; deleteBranch: boolean; form: typeof form };

  // Coming to another branch starts on its conversation, or as it was left there.
  $effect(() => {
    const left = kept.get<Left>(`pull\0${forge}\0${branch}`);
    tab = left?.tab ?? "conversation";
    method = left?.method ?? null;
    deleteBranch = left?.deleteBranch ?? true;
    form = left?.form ?? null;
    restackSkipped = false;
  });

  // How the screen is left, for coming back to it.
  $effect(() => {
    kept.set(`pull\0${forge}\0${branch}`, { tab, method, deleteBranch, form: $state.snapshot(form) } satisfies Left);
  });

  async function reload() {
    await github.refresh();
    if (summary) await load(summary.number);
  }

  async function act(request: Request | Promise<Request>) {
    if (await run(request)) await reload();
  }

  // --- The stack strip ---

  /** The trunk's name without its remote, e.g. "main". */
  const trunk = $derived(place && pr ? pr.settings.defaultBranch : (stack?.trunk.replace(/^[^/]+\//, "") ?? history.trunk ?? "main"));
  const stackNames = $derived(stack && stack.branches.length > 1 ? stack.branches.map((b) => b.name) : []);
  const arrows = $derived(stripWidth > 0 && stackNames.length ? softArrows(stackNames.length, stripWidth, HEIGHT).map((shape, i) => ({ ...shape, name: stackNames[i] })) : []);

  function stateWord(p: PullSummary): string {
    return p.state === "merged" ? "Merged" : p.state === "closed" ? "Closed" : p.draft ? "Draft" : "Open";
  }

  function stripStatus(name: string): string {
    const p = github.pullOf(name, forge);
    if (p) return `${w.mark}${p.number} · ${p.number === pr?.number ? status.label : stateWord(p)}`;
    const b = stack?.branches.find((x) => x.name === name);
    const toPush = b?.upstream ? b.ahead : (stack?.commits.filter((c) => c.branch === name).length ?? 0);
    return toPush ? `No ${w.noun} · ${toPush} to push` : `No ${w.noun}`;
  }

  // --- A restack under way ---

  /** The restack kept between its steps, started here or in another window of the repository. */
  let saved = $state<RestackState | null>(null);
  $effect(() => {
    void version;
    untrack(() => readSaved());
  });

  async function readSaved() {
    saved = await api.restack().catch(() => null);
  }

  const savedLines = $derived(saved ? restackLines(saved, pulls) : null);
  // Once its last step is done, there is nothing to go on with.
  $effect(() => {
    if (savedLines && pulls.length && restackFinished(savedLines)) {
      saved = null;
      api.saveRestack(null).catch(() => {});
    }
  });

  // --- The pull request ---

  const approvals = $derived(pr ? pr.reviews.filter((r) => r.state === "approved").length : 0);
  const status = $derived.by((): { label: string; tone: "ok" | "warn" | "grey" | "steel" } => {
    if (!pr) return { label: "", tone: "grey" };
    if (pr.state === "merged") return { label: "Merged", tone: "steel" };
    if (pr.state === "closed") return { label: "Closed", tone: "grey" };
    if (pr.draft) return { label: "Draft", tone: "grey" };
    if (pr.reviews.some((r) => r.state === "changesRequested")) return { label: "Changes requested", tone: "warn" };
    if (approvals && approvals >= (pr.requiredApprovals ?? 1)) return { label: "Approved", tone: "ok" };
    if (pr.reviews.length || pr.requested.length) return { label: "In review", tone: "grey" };
    return { label: "Open", tone: "grey" };
  });
  const body = $derived(pr ? readBody(pr.body) : { text: "", stack: [] });
  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

  /** The merged pull request this one is still built on, when the stack wasn't restacked yet. */
  const mergedBelow = $derived(
    pr && pr.state === "open" ? (pulls.find((p) => p.state === "merged" && p.head !== pr!.head && pr!.commits.some((c) => c.sha === p.headSha)) ?? null) : null,
  );
  const basePull = $derived(pr && pr.base !== pr.settings.defaultBranch ? github.openPullOf(pr.base, forge) : null);

  const failing = $derived(pr ? pr.checks.filter((c) => c.state === "fail") : []);
  const running = $derived(pr ? pr.checks.filter((c) => c.state === "run") : []);
  const facts = $derived(
    pr
      ? { pr, mergedBelow, basePull, restack: saved && savedLines && restackTouches(saved, pr) ? savedLines : null, trunk, checkedOut: history.head.branch === pr.head, words: w }
      : null,
  );
  /** What keeps it from merging, the first one first. */
  const blocked = $derived(facts ? blockers(facts) : []);
  const next = $derived(facts ? nextStep(facts, blocked) : null);
  const also = $derived(blocked.slice(1).flatMap((b) => (b.short ? [b.short] : [])));
  const checking = $derived(!!pr && pr.state === "open" && pr.mergeableState === "unknown");
  const canMerge = $derived(!!pr && pr.state === "open" && !blocked.length && !checking && pr.mergeable !== false);

  const methods = $derived(pr ? methodsOf(pr) : []);
  const chosen = $derived(method && methods.some((m) => m.id === method) ? method : pr ? defaultMethod(pr, methods) : "merge");
  const chosenMethod = $derived(methods.find((m) => m.id === chosen) ?? METHODS[0]);
  const preview = $derived(pr && place ? mergeMessage(place.owner ?? "", pr, chosen) : { title: null, message: null });
  /** A fast-forward on GitLab: no commit of its own, so no message. */
  const fastForward = $derived(!!pr?.gitlab && pr.gitlab.mergeMethod === "ff" && chosen === "merge");
  const autoMergeNote = $derived.by(() => {
    if (!pr) return "";
    if (!pr.settings.autoMerge) return "Turned off for this repository on GitHub";
    if (pr.base !== pr.settings.defaultBranch) return `Once it goes into ${pr.settings.defaultBranch}`;
    return lab ? "When the pipeline passes" : "When checks pass and it is approved";
  });
  const autoMergeOff = $derived(!pr || !pr.settings.autoMerge || pr.base !== pr.settings.defaultBranch || !pr.settings.canPush);

  const baseLine = $derived.by((): { tone: "ok" | "warn" | "stack" | "grey"; text: string } | null => {
    if (!pr || pr.state !== "open") return null;
    if (mergedBelow) return { tone: "warn", text: `Based on ${mergedBelow.head}, which is merged. Restack to move it onto ${trunk}.` };
    if (basePull) return { tone: "stack", text: `Based on ${pr.base} (${w.mark}${basePull.number}). It can merge once ${w.mark}${basePull.number} is in ${basePull.base}.` };
    if (pr.mergeable === false || pr.mergeableState === "dirty") return { tone: "warn", text: `Conflicts with ${pr.base}` };
    if (pr.mergeableState === "behind") return { tone: "warn", text: pr.gitlab?.behind ? `${plural(pr.gitlab.behind, "commit")} behind ${pr.base}` : `Behind ${pr.base}` };
    if (checking) return { tone: "grey", text: `${w.name} is checking whether it merges cleanly…` };
    return { tone: "ok", text: `No conflicts with ${pr.base}` };
  });

  const checksSummary = $derived.by(() => {
    if (!pr) return { text: "", tone: "" };
    if (failing.length) return { text: `${failing.length} failing`, tone: "bad" };
    if (running.length) return { text: `${running.length} running`, tone: "warn" };
    return { text: `${pr.checks.filter((c) => c.state === "ok").length} passed`, tone: "ok" };
  });
  const reviewSummary = $derived(
    !pr ? "" : pr.draft ? "Draft, reviews start when ready" : pr.requiredApprovals ? `${approvals} of ${pr.requiredApprovals} required approvals` : plural(approvals, "approval"),
  );

  const REVIEW_WORDS: Record<ReviewState, { action: string; badge: string | null; tone: string }> = {
    approved: { action: "approved these changes", badge: "Approved", tone: "ok" },
    changesRequested: { action: "asked for changes", badge: "Changes requested", tone: "warn" },
    commented: { action: "reviewed", badge: null, tone: "grey" },
    dismissed: { action: "reviewed (dismissed)", badge: "Dismissed", tone: "grey" },
  };
  const ICONS: Record<string, string> = {
    ok: "M3.5 8.5 6.5 11.5 12.5 4.5",
    fail: "M5 5l6 6M11 5l-6 6",
    run: "M8 4.5v3.5l2 1.5",
    skip: "M4.5 8h7",
    push: "M8 12V4M5 7l3-3 3 3",
    review: "M3 8h10M8 3v10",
    draft: "M4 12l1-3 6-6 2 2-6 6z",
    dot: "M8 8h.01",
    next: "M6.5 4.5 10 8l-3.5 3.5",
    warn: "M8 4.5v4M8 11.2v.1",
  };

  const reviewers = $derived.by(() => {
    if (!pr) return [];
    const given = pr.reviews.map((r) => ({ login: r.author.login, avatar: r.author.avatarUrl, state: REVIEW_WORDS[r.state].badge ?? "Commented", tone: REVIEW_WORDS[r.state].tone }));
    const asked = pr.requested.filter((login) => !given.some((g) => g.login === login)).map((login) => ({ login, avatar: "", state: "Requested", tone: "grey" }));
    return [...given, ...asked];
  });

  function duration(seconds: number | null): string {
    if (seconds === null) return "";
    if (seconds < 60) return `${seconds}s`;
    return `${Math.floor(seconds / 60)}m ${String(seconds % 60).padStart(2, "0")}s`;
  }

  /** A check's page opens from here: one on github.com, or one of the project on GitLab. */
  const opens = (check: PullCheck) => !!check.url && (lab ? !!github.mrProject && check.url.startsWith(`${github.mrProject.webUrl}/-/`) : check.url.startsWith("https://github.com/"));

  function openCheck(check: PullCheck) {
    if (!opens(check)) return;
    (lab ? api.openGitLab(check.url!) : api.openGitHub(check.url!)).catch(() => {});
  }

  // --- Checking it out ---

  /** A pull request from another account's copy of the repository. */
  const fork = $derived(!!pr?.headOwner && pr.headOwner !== place?.owner);
  /** The worktree button: none for a branch checked out here, or one of this repository's
   *  branches that has no local copy yet but is the one this screen is about. */
  const worktreeLabel = $derived.by(() => {
    if (!pr || !place) return null;
    if (fork) return "Check Out in Worktree";
    if (history.head.branch === pr.head) return null;
    return worktreeOf(history, pr.head) ? "Open Its Worktree" : "Check Out in Worktree";
  });

  function checkOut() {
    if (!pr || !place) return;
    const elsewhere = worktreeOf(history, pr.head);
    if (elsewhere && !fork) {
      api.openWindow(elsewhere.path).catch(() => {});
      return;
    }
    const here = history.worktrees.find((t) => t.current)?.path ?? "";
    act(pullWorktreeRequest(ctx, here, pr, place.remote, fork, lab));
  }

  // --- Merging ---

  function merge() {
    if (!pr || !place || !canMerge) return;
    act(mergeRequest(ctx, place, pr, chosen, deleteBranch));
  }

  function toggleAutoMerge() {
    if (!pr || autoMergeOff) return;
    act(autoMergeRequest(pr, pr.autoMerge ? null : chosen));
  }

  // --- More: draft, target, rebase, close ---

  let menu = $state<{ x: number; y: number; entries: MenuEntry[] } | null>(null);

  /** Branches it could merge into instead: the one under it in the stack, the trunk, then the
   *  others the remote has. */
  const targets = $derived.by(() => {
    if (!pr || !place) return [];
    const p = pr;
    const prefix = `${place.remote}/`;
    const there = history.refs.filter((r) => r.kind === "remote" && r.name.startsWith(prefix)).map((r) => r.name.slice(prefix.length));
    const all = [...(below ? [below] : []), p.settings.defaultBranch, ...there];
    return all.filter((b, i) => b !== "HEAD" && b !== p.base && b !== p.head && all.indexOf(b) === i).slice(0, 8);
  });

  function openMore(event: MouseEvent) {
    if (!pr) return;
    const p = pr;
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const item = (label: string, icon: string, run: () => void, danger = false): MenuEntry => ({ kind: "item", label, icon, run, danger });
    const entries: MenuEntry[] = [];
    const may = p.settings.canPush || mine;
    if (p.state === "open" && may) {
      entries.push(p.draft ? item("Ready for Review…", menuIcons.edit, () => act(readyRequest(p))) : item("Mark as Draft…", menuIcons.edit, () => act(draftRequest(p))));
      entries.push(item(lab ? "Change Target Branch…" : "Change Base Branch…", menuIcons.merge, () => act(targetRequest(ctx, p, targets))));
      if (lab ? p.gitlab?.behind !== 0 : p.mergeableState === "behind") entries.push(item(lab ? `Rebase onto ${p.base}…` : "Update Branch…", lab ? menuIcons.rebase : menuIcons.merge, () => act(updateBranchRequest(ctx, p))));
    }
    if (p.state !== "merged" && may) {
      if (entries.length) entries.push({ kind: "sep" });
      entries.push(p.state === "open" ? item(`Close ${lab ? "Merge" : "Pull"} Request…`, menuIcons.drop, () => act(closeRequest(p, false)), true) : item("Reopen…", menuIcons.undo, () => act(closeRequest(p, true))));
    }
    menu = { x: box.left, y: box.bottom + 4, entries };
  }

  // --- Restacking after the base merged ---

  interface Plan {
    /** The merged pull request whose commits leave the stack. */
    merged: PullSummary;
    stack: Stack;
    drop: string[];
    preview: StackPreview;
    /** Open pull requests of the stack that still go into the merged branch. */
    retarget: PullSummary[];
    /** Why it can't be restacked from here, e.g. the merge isn't here yet. */
    problem: string | null;
  }
  let restack = $state<Plan | null>(null);
  let restackSkipped = $state(false);

  /** Where the stack goes: a branch that has the merge, fetched first when none has it yet, so
   *  the merged commits never leave the stack for a base without them. */
  async function landing(s: Stack, merged: PullRequest): Promise<Landing | null> {
    if (!merged.mergeCommit) return null;
    const candidates = [...new Set([s.trunk, ...(place ? [`${place.remote}/${merged.base}`] : []), merged.base])];
    const found = await api.landedOn(merged.mergeCommit, candidates);
    if (found || !place) return found;
    await api.fetchInBackground(place.remote).catch(() => {});
    return api.landedOn(merged.mergeCommit, candidates);
  }

  /** The stack built on `merged`, here, with its commits left out and moved onto a branch that
   *  has the merge. */
  async function planRestack(merged: PullRequest): Promise<Plan | null> {
    const shas = new Set(merged.commits.map((c) => c.sha));
    const candidates = pulls.filter((p) => p.state === "open" && p.head !== merged.head && ctx.history.refs.some((r) => r.kind === "local" && r.name === p.head));
    for (const candidate of candidates) {
      let s = await api.stack(candidate.head).catch(() => null);
      if (!s) continue;
      const drop = s.commits.filter((c) => shas.has(c.id)).map((c) => c.id);
      if (!drop.length) continue;
      const names = new Set(s.branches.map((b) => b.name));
      const retarget = pulls.filter((p) => p.state === "open" && names.has(p.head) && p.base === merged.head);
      const stuck = (problem: string): Plan => ({ merged, stack: s!, drop, preview: { changed: false } as StackPreview, retarget, problem });
      if (!merged.commitsComplete)
        return stuck(`${w.mark}${merged.number} has more commits than ${w.name} lists (${lab ? 500 : 250}), so Oxbow can’t tell which ones to leave out. Use Edit Stack to drop them by hand.`);
      const onto = await landing(s, merged);
      if (!onto) return stuck(`${merged.base} here doesn’t have the merge yet. Fetch, then come back to restack.`);
      s = { ...s, trunk: onto.name, trunkTip: onto.tip };
      const draft = freshDraft(s, true);
      for (const id of drop) draft.acts[id] = "drop";
      draft.items = draft.items.filter((item) => !(item.kind === "branch" && item.name === merged.head));
      const preview = await api.stackPreview(planOf(s, draft));
      return { merged, stack: s, drop, preview, retarget, problem: null };
    }
    return null;
  }

  // On a merged pull request: is anything here still built on it?
  $effect(() => {
    const current = pr;
    restack = null;
    if (!current || current.state !== "merged") return;
    planRestack(current).then((r) => pr === current && (restack = r), () => {});
  });

  async function startRestack(merged: PullSummary) {
    const detail = merged.number === pr?.number ? pr : await (lab ? api.gitlabMergeRequest(merged.number) : api.githubPull(merged.number));
    const plan = restack?.merged.number === merged.number ? restack : await planRestack(detail);
    if (!plan || plan.problem) return;
    const draft = freshDraft(plan.stack, true);
    for (const id of plan.drop) draft.acts[id] = "drop";
    draft.items = draft.items.filter((item) => !(item.kind === "branch" && item.name === merged.head));
    const forced = plan.preview.branches.filter((b) => b.forcePush);
    const ups = plan.stack.branches.filter((b) => b.upstream && forced.some((f) => f.name === b.name));
    const remote = ups[0]?.upstream?.remote ?? null;
    const kept: Restack = {
      merged: merged.number,
      base: merged.base,
      branches: plan.stack.branches.filter((b) => b.name !== merged.head).map((b) => b.name),
      drop: plan.drop,
      remote,
      push: ups.filter((b) => b.upstream!.remote === remote).map((b) => ({ branch: b.name, upstream: b.upstream!.branch })),
      retarget: plan.retarget.map((p) => p.number),
      failed: null,
    };
    const rebased = await run(editStackRequest(ctx, plan.stack, draft, plan.preview));
    // Kept also when the rebase stopped on a conflict; one that never ran is forgotten when read.
    await api.saveRestack(kept).catch(() => {});
    if (rebased) await pushAndRetarget(kept, null);
    await readSaved();
  }

  /** The restack's push and new base, or the ones of them still to go. */
  async function pushAndRetarget(kept: Restack, state: RestackState | null) {
    const left = state ? kept.push.filter((p) => state.unpushed.includes(p.branch)) : kept.push;
    const push: (Action & { kind: "pushBranches" }) | null = kept.remote && left.length ? { kind: "pushBranches", remote: kept.remote, branches: left } : null;
    const retarget = (state ? stillToRetarget(state, pulls) : kept.retarget).map((number) => ({ number, base: kept.base }));
    const next = await restackPushRequest(
      ctx,
      w,
      push,
      retarget,
      (step, message) => api.saveRestack({ ...kept, failed: { step, message } }),
      () => api.saveRestack(null),
    );
    if (next) await run(next);
    else await api.saveRestack(null).catch(() => {});
    await reload();
  }

  async function continueRestack() {
    if (!saved || saved.rebasing) return;
    await pushAndRetarget(saved.restack, saved);
    await readSaved();
  }

  async function forgetRestack() {
    saved = null;
    await api.saveRestack(null).catch(() => {});
  }

  const restackSteps = $derived.by(() => {
    if (!restack || restack.problem) return [];
    const { stack: s, drop, preview: p, retarget, merged } = restack;
    const kept = s.commits.length - drop.length;
    const names = s.branches.filter((b) => b.name !== merged.head).map((b) => b.name);
    const steps = [`Rebase ${plural(kept, "commit")} of ${names.join(" and ")} onto ${s.trunk}, leaving out ${plural(drop.length, "commit")} ${merged.base} has now`];
    const forced = p.branches.filter((b) => b.forcePush).length;
    if (forced) steps.push(`Force push ${plural(forced, "branch")} with --force-with-lease`);
    for (const r of retarget) steps.push(`Change the ${lab ? "target" : "base"} of ${w.mark}${r.number} to ${merged.base}`);
    steps.push(p.conflict ? `Stops on a conflict in “${p.conflict.summary}” for you to resolve` : "No conflicts expected (dry run)");
    return steps;
  });

  // --- The next step ---

  let adding = $state("");
  let reviewerBox = $state<HTMLInputElement | null>(null);

  function askReviewer() {
    const login = adding.trim().replace(/^@/, "");
    adding = "";
    if (pr && login) act(reviewersRequest(pr, [login]));
  }

  /** The box that asks someone on GitLab, picked from the people found. */
  let askBox = $state<HTMLElement | null>(null);
  const askedIds: Record<string, number> = {};

  async function askGitLab(username: string) {
    if (!pr) return;
    let id: number | undefined = askedIds[username] ?? form?.known?.[username];
    if (!id) id = exactPerson(await api.gitlabPeople(username).catch(() => []), username)?.id;
    if (id) act(reviewersRequest(pr, [username], [id]));
  }

  /** The button of the next step, when it has one. */
  const nextAction = $derived.by((): { label: string; go: () => void } | null => {
    if (!pr || !next || !place) return null;
    const p = pr;
    const remote = place.remote;
    const local = history.refs.some((r) => r.kind === "local" && r.name === p.head);
    switch (next.kind) {
      case "continueRestack":
        return saved && !saved.rebasing ? { label: "Continue…", go: continueRestack } : null;
      case "restack":
        return mergedBelow ? { label: "Restack…", go: () => mergedBelow && startRestack(mergedBelow) } : null;
      case "waitBase":
        return basePull ? { label: `Show #${basePull.number}`, go: () => basePull && onBranch(basePull.head) } : null;
      case "ready":
        return p.settings.canPush ? { label: "Ready for Review…", go: () => act(readyRequest(p)) } : null;
      case "conflicts":
        if (history.head.branch === p.head) return { label: `Merge ${p.base}…`, go: () => act(mergeIntoRequest(ctx, `${remote}/${p.base}`)) };
        return local ? { label: `Switch to ${p.head}…`, go: () => act(switchRequest(ctx, p.head)) } : null;
      case "update":
        return p.settings.canPush ? { label: lab ? "Rebase…" : "Update Branch…", go: () => act(updateBranchRequest(ctx, p)) } : null;
      case "fixChecks": {
        const check = failing.find(opens);
        return check ? { label: `Show on ${w.name}`, go: () => openCheck(check) } : null;
      }
      case "changes":
      case "threads":
        return tab !== "conversation" ? { label: "Show Conversation", go: () => (tab = "conversation") } : null;
      case "review":
        return p.settings.canPush && !p.requested.length ? { label: "Ask for Review…", go: () => (lab ? askBox?.querySelector("input")?.focus() : reviewerBox?.focus()) } : null;
      default:
        return null;
    }
  });

  // --- A new pull request ---

  let form = $state<{
    branch: string;
    title: string;
    body: string;
    base: string;
    draft: boolean;
    reviewers: string[];
    adding: string;
    suggested: string[];
    /** GitLab: what else it opens with, and the account ids of the usernames picked. */
    assignee?: string | null;
    labels?: string[];
    removeSource?: boolean;
    squash?: boolean;
    template?: string | null;
    known?: Record<string, number>;
  } | null>(null);
  /** GitLab's labels and settings for the form, and the templates on the trunk. */
  let labFacts = $state<GitLabCreateFacts | null>(null);
  let labFactsError = $state<string | null>(null);

  /** The branch under this one in the stack, the natural base. */
  const below = $derived.by(() => {
    if (!stack) return null;
    const i = stack.branches.findIndex((b) => b.name === branch);
    return i >= 0 && i + 1 < stack.branches.length ? stack.branches[i + 1].name : null;
  });
  const bases = $derived([...(below ? [below] : []), trunk].filter((b, i, all) => all.indexOf(b) === i));

  /** Commits `branch` brings into `base`, from the stack (newest first). */
  function commitsOver(base: string): number {
    if (!stack) return 0;
    const tipOf = (name: string) => stack!.commits.findIndex((c) => c.id === stack!.branches.find((b) => b.name === name)?.at);
    const from = tipOf(branch);
    if (from < 0) return 0;
    const to = base === below ? tipOf(base) : stack.commits.length;
    return Math.max(0, (to < 0 ? stack.commits.length : to) - from);
  }

  const localRef = $derived(history.refs.find((r) => r.kind === "local" && r.name === branch) ?? null);
  const toPush = $derived.by(() => {
    if (!localRef) return 0;
    const t = localRef.tracking;
    if (!t || t.gone) return commitsOver(form?.base ?? trunk) || 1;
    return t.ahead;
  });

  // The form starts from the branch's own commits.
  $effect(() => {
    if (summary || !stack || form?.branch === branch) return;
    const base = below ?? trunk;
    const own = stack.commits.filter((c) => c.branch === branch);
    // The newest commit says best where the branch ended up.
    const title = own[0]?.summary ?? branch;
    const text = own.length === 1 ? own[0].message.split("\n").slice(1).join("\n").trim() : "";
    form = { branch, title, body: text, base, draft: true, reviewers: [], adding: "", suggested: [] };
    if (lab) {
      form = { ...form, assignee: null, labels: [], removeSource: true, squash: false, template: null, known: {} };
      const facts = untrack(() => labFacts);
      if (facts) startFrom(facts);
      else fresh = true;
    }
    suggest(base);
  });

  /** A new form waits for GitLab's facts to take its defaults from. */
  let fresh = false;
  let factsAsked = false;
  $effect(() => {
    if (!lab || !form || !stack || labFacts || labFactsError || factsAsked) return;
    const rev = stack.trunk;
    untrack(() => loadLabFacts(rev));
  });

  async function loadLabFacts(rev: string) {
    factsAsked = true;
    try {
      labFacts = await api.gitlabCreateFacts(rev);
      if (fresh && form) startFrom(labFacts);
      fresh = false;
    } catch (err) {
      labFactsError = String(err);
    } finally {
      factsAsked = false;
    }
  }

  /** The defaults GitLab gives a new merge request: the project's way with merged branches and
   *  squashing, and the Default template for the description. */
  function startFrom(facts: GitLabCreateFacts) {
    if (!form) return;
    form.removeSource = facts.removeSourceBranch;
    form.squash = squashStarts(facts.squashOption);
    const start = defaultTemplate(facts.templates);
    if (start) {
      form.template = start.name;
      form.body = start.text;
    }
  }

  function pickTemplate(name: string | null) {
    if (!form || !labFacts) return;
    form.template = name;
    form.body = labFacts.templates.find((t) => t.name === name)?.text ?? "";
  }

  async function suggest(base: string) {
    const name = branch;
    const ref = base === below ? base : (stack?.trunk ?? base);
    if (lab) {
      const owners = await api.gitlabCodeOwners(ref, name).catch(() => [] as GitLabUser[]);
      if (!form || form.branch !== name) return;
      const others = owners.filter((o) => o.username !== github.mrMe);
      form.known = { ...form.known, ...Object.fromEntries(others.map((o) => [o.username, o.id])) };
      form.suggested = others.map((o) => o.username);
      form.reviewers = [...form.suggested];
      return;
    }
    const owners = await api.codeOwners(ref, name).catch(() => [] as string[]);
    if (!form || form.branch !== name) return;
    form.suggested = owners.filter((o) => o !== me);
    form.reviewers = [...form.suggested];
  }

  /** GitLab's people for a box of the form: name, username and their id kept for sending. */
  async function findPeople(text: string): Promise<Pick[]> {
    if (!text.trim()) return [];
    const people = await api.gitlabPeople(text);
    for (const p of people) askedIds[p.username] = p.id;
    if (form) form.known = { ...form.known, ...Object.fromEntries(people.map((p) => [p.username, p.id])) };
    return people.map((p) => ({ key: p.username, label: p.username, sub: p.name }));
  }

  const labelPicks = (names: string[]): Pick[] => names.map((name) => ({ key: name, label: name, color: labFacts?.labels.find((l) => l.name === name)?.color }));

  function pickBase(base: string) {
    if (!form || form.base === base) return;
    form.base = base;
    suggest(base);
  }

  function addReviewer() {
    if (!form) return;
    const login = form.adding.trim().replace(/^@/, "");
    if (login && !form.reviewers.includes(login)) form.reviewers = [...form.reviewers, login];
    form.adding = "";
  }

  const stackLines = $derived.by((): StackLine[] | null => {
    if (!stack || stack.branches.length < 2) return null;
    const lines = [...stack.branches].reverse().map((b) => {
      const p = github.pullOf(b.name, forge);
      return { branch: b.name, number: b.name === branch ? `${w.mark}$NUMBER` : p ? `${w.mark}${p.number}` : null, merged: p?.state === "merged" };
    });
    return lines.some((l) => l.branch !== branch && l.number) ? lines : null;
  });
  /** The stack's other open pull requests, oldest first. */
  const others = $derived(stack ? [...stack.branches].reverse().map((b) => github.openPullOf(b.name, forge)).filter((p): p is PullSummary => !!p && p.head !== branch) : []);

  const newPull = $derived.by((): NewPull | null => {
    if (!form || !place) return null;
    const t = localRef?.tracking;
    const remote = t && !t.gone ? t.remote : place.remote;
    const needsPush = !t || t.gone || t.ahead > 0;
    const known = form.known ?? {};
    const project = github.mrProject;
    const ref = localRef;
    const gitlab =
      lab && project && ref
        ? {
            create: {
              reviewerIds: form.reviewers.flatMap((r) => (known[r] ? [known[r]] : [])),
              assigneeIds: form.assignee && known[form.assignee] ? [known[form.assignee]] : [],
              labels: form.labels ?? [],
              removeSourceBranch: !!form.removeSource,
              squash: !!form.squash,
            },
            assignee: form.assignee ?? null,
            fallback: () => branchMergeRequest(ctx, ref, project),
          }
        : null;
    // A merge request needs its local branch: that's what goes up, and what the push path asks for.
    if (lab && !gitlab) return null;
    return {
      branch,
      base: form.base,
      title: form.title,
      body: form.body,
      draft: form.draft,
      reviewers: form.reviewers,
      push: needsPush ? { kind: "push", remote, branch, upstream: t && !t.gone ? t.branch : branch, setUpstream: !t || t.gone, force: false, lease: null, noVerify: false } : null,
      stack: stackLines,
      others,
      commits: commitsOver(form.base),
      gitlab,
    };
  });

  function create() {
    if (newPull) act(createRequest(ctx, newPull));
  }

  // --- Small pieces ---

  const toneOf = (tone: string) =>
    tone === "ok" ? ["var(--green-soft)", "var(--green)"] : tone === "warn" ? ["var(--orange-soft)", "var(--orange)"] : tone === "bad" ? ["var(--red-soft)", "var(--red)"] : tone === "steel" ? [tint(0, "label"), plate(0)] : ["var(--field)", "var(--text2)"];

  const entryKey = (e: PullEntry, i: number) => `${e.kind}:${e.time}:${i}`;
</script>

{#snippet avatar(login: string, url: string, size: number)}
  <span class="avatar" style:width="{size}px" style:height="{size}px" style:background={person(login)} style:font-size="{Math.round(size * 0.42)}px">
    {(login[0] ?? "?").toUpperCase()}
    {#if url}<img src={url} alt="" onerror={(e) => ((e.currentTarget as HTMLImageElement).style.display = "none")} />{/if}
  </span>
{/snippet}

{#snippet finishButton(r: ReviewSession)}
  <button class="button primary small" bind:this={finishAt} onclick={() => (r.finishing = !r.finishing)} aria-expanded={r.finishing}>
    <svg class="icon small" viewBox="0 0 16 16"><path d="M3 4a1.5 1.5 0 0 1 1.5-1.5h7A1.5 1.5 0 0 1 13 4v5a1.5 1.5 0 0 1-1.5 1.5H7L4.5 13v-2.5A1.5 1.5 0 0 1 3 9z" /></svg>
    Finish Review
    {#if r.count}<span class="finish-count">{r.count}</span>{/if}
  </button>
{/snippet}

{#snippet dot(icon: string, bg: string, fg: string)}
  <span class="dot" style:background={bg} style:color={fg}>
    <svg viewBox="0 0 16 16"><path d={ICONS[icon] ?? ICONS.dot} /></svg>
  </span>
{/snippet}

{#snippet restackList(lines: RestackLine[])}
  {#each lines as line, i (line.step)}
    {@const look = line.state === "done" ? ["ok", ...toneOf("ok")] : line.state === "now" ? ["next", tint(0, "label"), plate(0)] : line.state === "next" ? ["dot", ...toneOf("grey")] : ["warn", ...toneOf("warn")]}
    <span class="step" class:waiting={line.state === "next"}>
      {@render dot(look[0], look[1], look[2])}
      <span class="step-text">
        <span>{i + 1}. {line.text}</span>
        {#if line.why}<span class="why">{line.why}</span>{/if}
      </span>
    </span>
  {/each}
{/snippet}

{#snippet capsule(name: string)}
  <span class="capsule-label" style:background={tint(colorOf(name), "label")} style:color={plate(colorOf(name))}>{name}</span>
{/snippet}

{#snippet toggle(on: boolean, label: string, sub: string, disabled: boolean, flip: () => void)}
  <button class="switch-row" role="switch" aria-checked={on} {disabled} onclick={flip}>
    <span class="switch-text"><span>{label}</span><span class="sub">{sub}</span></span>
    <span class="switch" class:on><span class="knob"></span></span>
  </button>
{/snippet}

<div class="pull">
  <section class="main" aria-label="Pull request">
    {#if stackNames.length}
      <div class="stack-head">
        <svg class="icon small" viewBox="0 0 16 16" style:color={lane(colorOf(branch))}><rect x="3" y="6.5" width="10" height="7" rx="1.5" /><path d="M4.5 4.5h7M6 2.5h4" /></svg>
        <span class="grow">Stack {stack?.top}</span>
        <span>{stackNames.indexOf(branch) + 1} of {stackNames.length}</span>
      </div>
    {/if}
    <div class="strip" class:hidden={!stackNames.length} bind:clientWidth={stripWidth}>
      {#each arrows as a (a.name)}
        {@const on = a.name === branch}
        {@const c = colorOf(a.name)}
        <svg class="shape" aria-hidden="true" width={stripWidth} height={HEIGHT} viewBox="0 0 {stripWidth} {HEIGHT}">
          <path d={a.path} style:fill={on ? tint(c) : "var(--field)"} style:stroke={on ? `color-mix(in srgb, ${lane(c)} 40%, transparent)` : "transparent"} />
        </svg>
        <button class="arrow" style:left="{a.left}px" style:width="{a.width}px" onclick={() => onBranch(a.name)} aria-current={on}>
          <span class="arrow-name" style:color={on ? plate(c) : undefined}>{a.name}</span>
          <span class="arrow-sub">{stripStatus(a.name)}</span>
        </button>
      {/each}
    </div>

    {#if !place}
      <div class="empty">{lab ? "This repository has no GitLab project on a server you are signed in to." : "This repository has no remote on github.com."}</div>
    {:else if (lab ? github.mrProblem : github.problem) && !pulls.length}
      <div class="empty">
        <span>{lab ? github.mrProblem : github.problem}</span>
        <button class="capsule" onclick={() => api.openSettings()}>Open Settings</button>
      </div>
    {:else if summary && !pr}
      <div class="empty">{error ?? (loading ? `Loading ${w.mark}${summary.number}…` : "")}</div>
    {:else if pr}
      <div class="title-block">
        <div class="title-row">
          <h1>{pr.title}</h1>
          <span class="number">{w.mark}{pr.number}</span>
          {#if status.label}
            {@const [bg, fg] = toneOf(status.tone)}
            <span class="status" style:background={bg} style:color={fg}>{status.label}</span>
          {/if}
          {#if pr.state !== "merged" && (pr.settings.canPush || mine)}
          <button class="button icon-only small more" onclick={openMore} aria-label="More" title="More">
            <svg class="icon small" viewBox="0 0 16 16"><path d="M3.5 8h.01M8 8h.01M12.5 8h.01" /></svg>
          </button>
          {/if}
        </div>
        <div class="meta-row">
          {@render capsule(pr.head)}
          <svg class="icon small" viewBox="0 0 16 16"><path d="M13 8H3.5M7 4.5 3.5 8 7 11.5" /></svg>
          {@render capsule(pr.base)}
          <span class="ellipsis">{pr.author.login} opened {relativeTime(pr.created).toLowerCase()} · {plural(pr.commitCount, "commit")} · {plural(pr.changedFiles, "file")}{#if !pr.gitlab || pr.gitlab.lineCounts}{` · +${pr.additions} −${pr.deletions}`}{/if}</span>
        </div>
      </div>
      <div class="tabs" role="tablist" aria-label="Pull request">
        <button role="tab" class="tab" class:on={tab === "conversation"} aria-selected={tab === "conversation"} onclick={() => (tab = "conversation")}
          style:--on-bg={tint(colorOf(pr.head))} style:--on-fg={plate(colorOf(pr.head))}>Conversation</button>
        <button role="tab" class="tab" class:on={tab === "commits"} aria-selected={tab === "commits"} onclick={() => (tab = "commits")}
          style:--on-bg={tint(colorOf(pr.head))} style:--on-fg={plate(colorOf(pr.head))}>Commits {pr.commits.length}</button>
        {#if !pr.gitlab}
          <button role="tab" class="tab" class:on={tab === "files"} aria-selected={tab === "files"} onclick={() => (tab = "files")}
            style:--on-bg={tint(colorOf(pr.head))} style:--on-fg={plate(colorOf(pr.head))}>Files {pr.changedFiles}</button>
        {/if}
        {#if error}<span class="load-error">{error}</span>{/if}
        {#if rewrite || worktreeLabel || (tab === "files" && review)}<span class="grow"></span>{/if}
        {#if tab === "files" && review}
          <span class="viewed-count">{review.viewed.size} of {review.files} viewed</span>
          {#if reviewable}
            <span class="finish">
              {@render finishButton(review)}
              {#if review.finishing}<FinishReview session={review} {mine} anchor={finishAt} />{/if}
            </span>
          {/if}
        {:else if worktreeLabel && pr.state === "open"}
          <button class="rewrite keep" onclick={checkOut} title={worktreeLabel === "Open Its Worktree" ? `${pr.head} is checked out in another worktree` : `A working folder of its own with ${w.mark}${pr.number} checked out`}>
            <svg class="icon small" viewBox="0 0 16 16"><path d="M2.5 4.5a1 1 0 0 1 1-1h3l1.5 1.5h4.5a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1zM8 7.5v3.5M6.25 9.25h3.5" /></svg>
            {worktreeLabel}
          </button>
        {/if}
        {#if rewrite}
          <button class="rewrite" onclick={openSeries} title="{rewriteText}: each commit before and after, side by side (git range-diff)">
            <svg class="icon small" viewBox="0 0 16 16"><path d="M2.5 5h8M8 2.5 10.5 5 8 7.5M13.5 11h-8M8 8.5 5.5 11 8 13.5" /></svg>
            <span class="rewrite-why">{rewriteText} ·</span> Compare Series
          </button>
        {/if}
      </div>

      {#if tab === "files"}
        {#if review}<PullFiles session={review} me={github.me} color={colorOf(pr.head)} {version} />{/if}
      {:else}
      <div class="scroll">
        {#if tab === "conversation"}
          <div class="timeline">
            <div class="card-row">
              {@render avatar(pr.author.login, pr.author.avatarUrl, 30)}
              <div class="card">
                <div class="card-head"><span class="who">{pr.author.login}</span><span class="ellipsis">opened this {w.noun}</span><span class="when">· {relativeTime(pr.created)}</span></div>
                {#if body.text}<p class="text selectable">{body.text}</p>{:else}<p class="text muted">No description.</p>{/if}
                {#if body.stack.length}
                  <div class="stack-box">
                    <span class="stack-title">Stack · kept up to date by Oxbow</span>
                    {#each body.stack as row}
                      {@const mine = row.includes("← this one")}
                      <span class="stack-line" class:mine>{row}</span>
                    {/each}
                  </div>
                {/if}
              </div>
            </div>
            {#each pr.timeline as e, i (entryKey(e, i))}
              {#if e.kind === "event"}
                {@const [bg, fg] = e.tone ? toneOf(e.tone === "bad" ? "bad" : "ok") : ["var(--field)", "var(--text2)"]}
                <div class="event">
                  {@render dot(e.icon, bg, fg)}
                  <span class="ellipsis"><span class="who">{e.actor}</span> {e.text}</span>
                  <span class="when">· {relativeTime(e.time)}</span>
                </div>
              {:else if e.kind === "comment" || e.kind === "review"}
                {@const words = e.kind === "review" ? REVIEW_WORDS[e.state] : { action: "commented", badge: null, tone: "grey" }}
                <div class="card-row">
                  {@render avatar(e.author.login, e.author.avatarUrl, 30)}
                  <div class="card">
                    <div class="card-head">
                      <span class="who">{e.author.login}</span><span class="ellipsis">{words.action}</span><span class="when">· {relativeTime(e.time)}</span>
                      <span class="grow"></span>
                      {#if words.badge}
                        {@const [bg, fg] = toneOf(words.tone)}
                        <span class="badge" style:background={bg} style:color={fg}>{words.badge}</span>
                      {/if}
                    </div>
                    {#if e.body.trim()}<p class="text selectable">{e.body.trim()}</p>{/if}
                  </div>
                </div>
              {:else if e.kind === "thread"}
                {@const first = e.comments[0]}
                {@const file = splitPath(e.path)}
                <div class="card-row">
                  {@render avatar(first.author.login, first.author.avatarUrl, 30)}
                  <div class="card">
                    <div class="card-head">
                      <span class="who">{first.author.login}</span><span class="ellipsis">{e.path ? `commented on ${e.path}${e.line ? ` ${spotLabel(e)}` : ""}` : "started a thread"}</span><span class="when">· {relativeTime(e.time)}</span>
                      <span class="grow"></span>
                      {#if review && e.node && pr.state === "open" && github.me}
                        <button class="button mini text" onclick={() => review?.resolve(e)} title={e.resolved ? "Open the thread again" : "Fold the thread away on GitHub"}>{e.resolved ? "Unresolve" : "Resolve"}</button>
                      {/if}
                      {#if e.resolved !== null || e.outdated}
                        {@const [bg, fg] = toneOf(e.resolved ? "grey" : e.outdated ? "grey" : "warn")}
                        <span class="badge" style:background={bg} style:color={fg}>{e.resolved ? "Resolved" : e.outdated ? "Outdated" : "Open"}</span>
                      {/if}
                    </div>
                    {#if e.hunk.length}
                      <div class="code">
                        <div class="code-head"><span class="muted">{file.dir}</span><b>{file.name}</b></div>
                        {#each e.hunk as line, j (j)}
                          <div class="code-line" class:mark={line.mark}><span class="ln">{line.number ?? ""}</span><span class="src">{line.text}</span></div>
                        {/each}
                      </div>
                    {/if}
                    <div class="thread"><ReviewThread thread={e} session={review} me={signedIn} resolve={false} /></div>
                  </div>
                </div>
              {/if}
            {/each}
          </div>
        {:else}
          <div class="commits">
            {#each pr.commits as c (c.sha)}
              <div class="commit">
                <span class="commit-dot" style:border-color={lane(colorOf(pr.head))} style:background={lane(colorOf(pr.head))}></span>
                <span class="commit-text"><span class="ellipsis">{c.summary}</span><span class="sub">{c.author} · {relativeTime(c.time)}</span></span>
                {#if c.additions !== null}<span class="mono add">+{c.additions}</span><span class="mono del">−{c.deletions}</span>{/if}
                <button class="mono sha" onclick={() => copy(c.sha)} title="Copy {c.sha}">{shortId(c.sha)}</button>
              </div>
            {/each}
          </div>
        {/if}
      </div>
      {/if}
    {:else if form && place}
      <div class="title-block">
        <h1>New {w.noun}</h1>
        <span class="muted small-text">
          {#if toPush}{branch} has {plural(toPush, "commit")} that {toPush === 1 ? "is" : "are"} not on {w.name} yet.{:else}{branch} is on {w.name}; this opens a {w.noun} for it.{/if}
        </span>
      </div>
      <div class="form">
        <label class="form-row">
          <span class="form-label">Title</span>
          <input class="field" bind:value={form.title} placeholder="What it does" />
        </label>
        <div class="form-row">
          <span class="form-label">{lab ? "Target" : "Base"}</span>
          <span class="chips">
            {#each bases as b (b)}
              <button class="chip" class:on={form.base === b} style:background={form.base === b ? tint(colorOf(b), "label") : undefined} style:color={form.base === b ? plate(colorOf(b)) : undefined} onclick={() => pickBase(b)}>{b}</button>
            {/each}
            {#if below && form.base === below}<span class="note">Picked from the stack: {branch} is built on {below}</span>{/if}
          </span>
        </div>
        {#if lab && labFacts?.templates.length}
          <div class="form-row">
            <span class="form-label">Template</span>
            <span class="chips">
              {#each labFacts.templates as t (t.name)}
                <button class="chip" class:on={form.template === t.name} onclick={() => pickTemplate(t.name)}>{t.name}</button>
              {/each}
              <button class="chip" class:on={!form.template} onclick={() => pickTemplate(null)}>None</button>
              <span class="note">From .gitlab/merge_request_templates on {trunk}</span>
            </span>
          </div>
        {/if}
        <label class="form-row">
          <span class="form-label">Description</span>
          <textarea class="field" rows={lab && form.template ? 8 : 5} bind:value={form.body} placeholder="Optional"></textarea>
        </label>
        {#if stackLines}
          <div class="form-row">
            <span class="form-label"></span>
            <span class="note">Stack: {stackLines.map((l) => (l.branch === branch ? "this one" : (l.number ?? l.branch))).join(" → ")} (added by Oxbow)</span>
          </div>
        {/if}
        <div class="form-row">
          <span class="form-label">Reviewers</span>
          {#if lab}
            <PickChips
              label="Add a reviewer"
              people
              chosen={form.reviewers.map((r) => ({ key: r, label: r }))}
              find={findPeople}
              onPick={(p) => form && !form.reviewers.includes(p.key) && (form.reviewers = [...form.reviewers, p.key])}
              onRemove={(key) => form && (form.reviewers = form.reviewers.filter((x) => x !== key))}
            />
            {#if form.suggested.length}<span class="note side">Suggested from CODEOWNERS</span>{/if}
          {:else}
          <span class="chips">
            {#each form.reviewers as r (r)}
              <span class="chip on person">
                {@render avatar(r, "", 18)}{r}
                <button class="remove" aria-label="Remove {r}" onclick={() => form && (form.reviewers = form.reviewers.filter((x) => x !== r))}>×</button>
              </span>
            {/each}
            <input
              class="add-reviewer"
              placeholder="+ Add"
              bind:value={form.adding}
              onkeydown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  addReviewer();
                }
              }}
              onblur={addReviewer}
            />
            {#if form.suggested.length}<span class="note">Suggested from CODEOWNERS</span>{/if}
          </span>
          {/if}
        </div>
        {#if lab}
          <div class="form-row">
            <span class="form-label">Assignee</span>
            <PickChips
              label="Assign it"
              people
              single
              placeholder="+ Assign"
              chosen={form.assignee ? [{ key: form.assignee, label: form.assignee }] : []}
              find={findPeople}
              onPick={(p) => form && (form.assignee = p.key)}
              onRemove={() => form && (form.assignee = null)}
            />
            {#if !form.assignee && labFacts?.me}
              {@const self = labFacts.me}
              <button class="capsule side" onclick={() => form && ((form.known = { ...form.known, [self.username]: self.id }), (form.assignee = self.username))}>Assign to Me</button>
            {/if}
          </div>
          <div class="form-row">
            <span class="form-label">Labels</span>
            <PickChips
              label="Add a label"
              placeholder={labFacts && !labFacts.labels.length ? "No labels in this project" : "+ Add"}
              chosen={labelPicks(form.labels ?? [])}
              find={(text) => matchLabels(labFacts?.labels ?? [], form?.labels ?? [], text).map((l) => ({ key: l.name, label: l.name, color: l.color }))}
              onPick={(p) => form && (form.labels = [...(form.labels ?? []), p.key])}
              onRemove={(key) => form && (form.labels = (form.labels ?? []).filter((l) => l !== key))}
            />
          </div>
          {#if labFactsError}
            <div class="form-row"><span class="form-label"></span><span class="note">{labFactsError}</span></div>
          {/if}
        {/if}
      </div>
    {:else}
      <div class="empty">{loading ? "Loading…" : ""}</div>
    {/if}
  </section>

  <aside class="box" aria-label="Merge" class:hidden={!!pr && tab === "files"}>
    {#if pr && pr.state === "open" && tab !== "files" && review && reviewable && !mine && (review.count > 0 || (!!github.me && pr.requested.includes(github.me)))}
      {@const what = countWords(review.draft?.comments.length ?? 0, review.draft?.replies.length ?? 0)}
      <div class="next yours">
        <span class="next-label">Your review</span>
        <span class="next-title">{what ? "Finish it" : "Review it"}</span>
        <span class="next-text"
          >{what ? `${what} ${review.count === 1 ? "isn’t" : "aren’t"} sent yet.` : `${pr.author.login} asked you to review it.`}
          {review.viewed.size} of {review.files} files viewed.</span
        >
        <div class="buttons">
          <span class="finish">
            {@render finishButton(review)}
            {#if review.finishing}<FinishReview session={review} {mine} anchor={finishAt} />{/if}
          </span>
          <button class="button" onclick={() => (tab = "files")}>Go to Files</button>
        </div>
      </div>
    {/if}
    {#if pr && pr.state === "open"}
      {#if next}
        {@const ready = next.kind === "merge"}
        <div class="next" class:ready>
          <span class="next-label">{ready ? "Ready to merge" : "Next step"}</span>
          <span class="next-title">{next.title}</span>
          <span class="next-text">{next.text}{pr.autoMerge && !ready ? ` ${w.name} merges it on its own once it is ready.` : ""}</span>
          {#if next.kind === "continueRestack" && savedLines}
            <div class="steps">{@render restackList(savedLines)}</div>
          {/if}
          {#if also.length}
            <div class="also">
              <span class="also-title">Also before merging</span>
              {#each also as b (b)}
                <span class="block"><span class="bullet">•</span><span>{b}</span></span>
              {/each}
            </div>
          {/if}
          {#if nextAction}
            <div class="buttons"><button class="button primary" onclick={nextAction.go}>{nextAction.label}</button></div>
          {/if}
        </div>
      {/if}
      {#if pr.checks.length}
        <div class="group">
          <div class="group-head"><span class="grow">{lab ? "Pipeline" : "Checks"}</span><span class="tone-{checksSummary.tone}">{checksSummary.text}</span></div>
          {#each pr.checks as ck, i (`${ck.name}\0${i}`)}
            {@const [bg, fg] = toneOf(ck.state === "ok" ? "ok" : ck.state === "fail" ? "bad" : ck.state === "run" || ck.state === "warn" ? "warn" : "grey")}
            <button class="check" onclick={() => openCheck(ck)} disabled={!opens(ck)} title={opens(ck) ? `Open on ${w.name}` : ck.name}>
              {@render dot(ck.state, bg, fg)}
              <span class="ellipsis grow">{ck.name}{#if ck.detail}<span class="muted">{` · ${ck.detail}`}</span>{/if}</span>
              <span class="when">{ck.state === "run" ? "running" : ck.state === "skip" ? "skipped" : duration(ck.seconds)}</span>
            </button>
          {/each}
        </div>
      {/if}
      <div class="group">
        <div class="group-head"><span class="grow">Reviews</span><span>{reviewSummary}</span></div>
        {#each reviewers as r (r.login)}
          {@const [bg, fg] = toneOf(r.tone)}
          <div class="reviewer">
            {@render avatar(r.login, r.avatar, 20)}
            <span class="ellipsis grow">{r.login}</span>
            <span class="badge" style:background={bg} style:color={fg}>{r.state}</span>
          </div>
        {:else}
          <span class="muted small-text">Nobody is asked to review it yet.</span>
        {/each}
        {#if pr.settings.canPush && lab}
          <span class="ask-box" bind:this={askBox}>
            <PickChips label="Ask someone to review it" people placeholder="+ Ask someone" chosen={[]} find={findPeople} onPick={(p) => askGitLab(p.key)} onRemove={() => {}} />
          </span>
        {:else if pr.settings.canPush}
          <input
            class="add-reviewer ask"
            placeholder="+ Ask someone"
            aria-label="Ask someone to review it"
            bind:this={reviewerBox}
            bind:value={adding}
            onkeydown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                askReviewer();
              }
            }}
          />
        {/if}
      </div>
      {#if baseLine}
        {@const [bg, fg] = baseLine.tone === "stack" ? [tint(colorOf(pr.head)), plate(colorOf(pr.head))] : toneOf(baseLine.tone)}
        <div class="base-line" style:background={bg}>
          <svg class="icon small" viewBox="0 0 16 16" style:color={fg}
            ><path d={baseLine.tone === "ok" ? ICONS.ok : baseLine.tone === "stack" ? "M3 6.5h10v7H3zM4.5 4.5h7M6 2.5h4" : "M8 3.5v5.5M8 12.2v.1"} /></svg
          >
          <span class="grow">{baseLine.text}</span>
        </div>
      {/if}
      <div class="sep"></div>
      <div class="method-row">
        <span class="grow muted">Merge method</span>
        <span class="segmented" role="radiogroup" aria-label="Merge method">
          {#each methods as m (m.id)}
            <button role="radio" aria-checked={chosen === m.id} class:on={chosen === m.id} title={m.tip} onclick={() => (method = m.id)}>{m.label}</button>
          {/each}
        </span>
      </div>
      <div class="message">
        {#if preview.title}
          <span class="message-title ellipsis">{preview.title}</span>
          {#if preview.message}<span class="muted message-body">{preview.message}</span>{/if}
        {:else if lab && !fastForward}
          <span class="muted">GitLab writes the commit message from the project’s template.</span>
        {:else}
          <span class="muted">Each commit goes onto {pr.base} as it is.</span>
        {/if}
      </div>
      {#if canMerge}
        {@render toggle(deleteBranch, `Delete ${pr.head} after merging`, lab ? "On GitLab and here" : pr.settings.deleteBranchOnMerge ? "GitHub deletes it there itself; here too" : "On GitHub and here", false, () => (deleteBranch = !deleteBranch))}
      {:else}
        {@render toggle(!!pr.autoMerge, "Merge automatically when ready", autoMergeNote, autoMergeOff, toggleAutoMerge)}
      {/if}
      <button class="button primary merge" disabled={!canMerge} onclick={merge}>
        {pr.autoMerge && !canMerge ? "Will Merge When Ready" : chosenMethod.button}
      </button>
      <span class="note center">
        {#if canMerge}Merges on {w.name}, then updates your local {pr.base}.{:else if pr.autoMerge}{w.name} merges it once it is ready.{:else if checking}{w.name} is still checking it.{:else}Merging waits for the steps above.{/if}
      </span>
    {:else if pr && pr.state === "merged"}
      {@const kept = history.refs.some((r) => r.kind === "local" && r.name === pr!.head)}
      <div class="merged">
        <span class="merged-icon"><svg viewBox="0 0 16 16"><path d={ICONS.ok} /></svg></span>
        <span class="merged-text">
          <span class="merged-title">Merged into {pr.base}</span>
          <span class="muted">{pr.mergeCommit ? shortId(pr.mergeCommit) : ""}{pr.mergedBy ? ` · by ${pr.mergedBy}` : ""} · {kept ? `${pr.head} kept here` : `${pr.head} deleted here`}</span>
        </span>
      </div>
      {#if saved && saved.restack.merged === pr.number && savedLines}
        {@const top = saved.restack.branches[0] ?? pr.head}
        <div class="restack" style:border-color="color-mix(in srgb, {lane(colorOf(top))} 40%, transparent)" style:background={tint(colorOf(top))}>
          <span class="restack-title">Finish the restack</span>
          <span class="muted small-text">
            {saved.rebasing ? "The rebase stopped. Once it is done, the restack goes on from here." : "It goes on from the step that didn’t go through."}
          </span>
          <div class="steps">{@render restackList(savedLines)}</div>
          <div class="buttons">
            <button class="capsule" onclick={forgetRestack} title="Stop showing it; the branches stay as they are">Forget</button>
            <button class="button primary" disabled={saved.rebasing} onclick={continueRestack}>Continue…</button>
          </div>
        </div>
      {:else if restack && !restackSkipped}
        {@const built = restack.stack.branches.filter((b) => b.name !== pr!.head).map((b) => b.name)}
        <div class="restack" style:border-color="color-mix(in srgb, {lane(colorOf(restack.stack.top))} 40%, transparent)" style:background={tint(colorOf(restack.stack.top))}>
          <span class="restack-title">Restack the rest of the stack?</span>
          <span class="muted small-text">
            {built.join(" and ")} {built.length > 1 ? "were" : "was"} built on {pr.head}, which is in {pr.base} now{restack.drop.length && pr.commits.length > 1 && pr.mergeCommit ? " as new commits" : ""}.
          </span>
          {#if restack.problem}
            {@const [bg, fg] = toneOf("warn")}
            <span class="step">{@render dot("dot", bg, fg)}<span>{restack.problem}</span></span>
          {/if}
          {#each restackSteps as step, i (i)}
            {@const warn = i === restackSteps.length - 1 && !!restack.preview.conflict}
            {@const [bg, fg] = toneOf(warn ? "warn" : "ok")}
            <span class="step">{@render dot(warn ? "dot" : "ok", bg, fg)}<span>{step}</span></span>
          {/each}
          <div class="buttons">
            <button class="capsule" onclick={() => (restackSkipped = true)}>Later</button>
            <button class="button primary" disabled={!!restack.problem} onclick={() => restack && startRestack(restack.merged)}>Restack…</button>
          </div>
        </div>
      {:else if restack}
        <div class="skipped">
          <span class="grow">Restack skipped. The pull requests built on it still say “Restack” until it is done.</span>
          <button class="button small" onclick={() => (restackSkipped = false)}>Undo</button>
        </div>
      {/if}
    {:else if pr && pr.state === "closed"}
      <div class="skipped"><span class="grow">Closed without merging.</span></div>
    {:else if form && newPull}
      <span class="group-title">What happens</span>
      {#if newPull.push}
        <span class="step"><span class="num">1</span><span>Push {branch} to {newPull.push.remote} ({plural(toPush, "commit")})</span></span>
      {/if}
      <span class="step"
        ><span class="num">{newPull.push ? 2 : 1}</span><span
          >Open a {w.noun} into {form.base}{form.base === below ? `, so reviewers see only these ${plural(newPull.commits, "commit")}` : ""}</span
        ></span
      >
      {#if newPull.stack && others.length}
        <span class="step"><span class="num">{newPull.push ? 3 : 2}</span><span>Add the stack list to {others.map((o) => `${w.mark}${o.number}`).join(", ")} and the new {w.noun}</span></span>
      {/if}
      <div class="sep"></div>
      {@render toggle(form.draft, "Create as draft", lab ? "GitLab puts Draft: before its title; it can’t merge until marked ready" : "Reviewers hear about it when you mark it ready", false, () => form && (form.draft = !form.draft))}
      {#if lab}
        {@render toggle(!!form.removeSource, "Delete the source branch when merged", `GitLab deletes ${branch} there once it is in ${form.base}`, false, () => form && (form.removeSource = !form.removeSource))}
        {#if labFacts && labFacts.squashOption !== "never"}
          {@render toggle(!!form.squash, "Squash commits when merged", squashChosen(labFacts.squashOption) ? "One commit on the target for the whole merge request" : "The project always squashes", !squashChosen(labFacts.squashOption), () => form && (form.squash = !form.squash))}
        {/if}
      {/if}
      <button class="button primary merge" disabled={!form.title.trim()} onclick={create}>{newPull.push ? "Push and Create" : lab ? "Create Merge Request" : "Create Pull Request"}</button>
    {/if}
  </aside>
</div>

{#if menu}
  <Menu x={menu.x} y={menu.y} label={`${w.name} ${w.noun}`} entries={menu.entries} onClose={() => (menu = null)} />
{/if}

<style>
  .pull {
    flex-grow: 1;
    display: flex;
    min-height: 0;
    border-top: 1px solid var(--sep);
  }
  .main {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 14px 22px 16px;
    overflow: hidden;
  }
  .box {
    width: 392px;
    flex-shrink: 0;
    border-left: 1px solid var(--sep);
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 16px 18px;
    overflow-y: auto;
  }
  /* The Files tab takes the whole width. */
  .box.hidden {
    display: none;
  }
  .grow {
    flex-grow: 1;
    min-width: 0;
  }
  .ellipsis {
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .muted {
    color: var(--text2);
  }
  .small-text {
    font-size: var(--text-small);
  }
  .selectable {
    user-select: text;
    cursor: text;
  }
  .mono {
    font-family: var(--mono);
  }
  .icon {
    width: 16px;
    height: 16px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
    flex-shrink: 0;
  }
  .icon.small {
    width: 14px;
    height: 14px;
  }
  .empty {
    flex-grow: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: var(--text2);
    text-align: center;
  }

  /* The stack strip */
  .stack-head {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
    flex-shrink: 0;
    margin-bottom: -6px;
  }
  .strip {
    position: relative;
    height: 42px;
    flex-shrink: 0;
  }
  .strip.hidden {
    height: 0;
    margin-top: -14px;
  }
  .shape {
    position: absolute;
    left: 0;
    top: 0;
    overflow: visible;
    pointer-events: none;
  }
  .shape path {
    stroke-width: 1.5;
  }
  .arrow {
    position: absolute;
    top: 0;
    height: 42px;
    padding: 0 0 0 12px;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 1px;
    text-align: left;
    overflow: hidden;
  }
  .arrow-name {
    font-size: var(--text-small);
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .arrow-sub {
    font-size: var(--text-caption);
    color: var(--text2);
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Title */
  .title-block {
    display: flex;
    flex-direction: column;
    gap: 6px;
    flex-shrink: 0;
  }
  .title-row {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  h1 {
    margin: 0;
    font-size: var(--text-title);
    font-weight: 600;
    line-height: 1.3;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .number {
    font-size: var(--text-title);
    color: var(--text2);
    flex-shrink: 0;
  }
  .more {
    flex-shrink: 0;
  }
  .status {
    line-height: 22px;
    height: 22px;
    padding: 0 10px;
    border-radius: var(--radius-pill);
    font-size: var(--text-small);
    font-weight: 600;
    white-space: nowrap;
    flex-shrink: 0;
  }
  .meta-row {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-small);
    color: var(--text2);
    white-space: nowrap;
    min-width: 0;
  }
  .capsule-label {
    line-height: 20px;
    height: 20px;
    padding: 0 9px;
    border-radius: var(--radius-pill);
    font-size: var(--text-caption);
    font-weight: 500;
    flex-shrink: 0;
  }
  .tabs {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }
  .tab {
    height: 26px;
    padding: 0 12px;
    border-radius: var(--radius-pill);
    border: 1px solid var(--sep);
    font-size: var(--text-small);
    font-weight: 500;
  }
  .tab.on {
    background: var(--on-bg);
    border-color: color-mix(in srgb, var(--on-fg) 30%, transparent);
    color: var(--on-fg);
    font-weight: 600;
  }
  .load-error {
    font-size: var(--text-small);
    color: var(--red);
    margin-left: 8px;
  }
  .rewrite {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    height: 26px;
    padding: 0 12px;
    border-radius: var(--radius-pill);
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    font-size: var(--text-small);
    font-weight: 500;
    white-space: nowrap;
  }
  .rewrite.keep {
    flex-shrink: 0;
  }
  .rewrite-why {
    color: var(--text2);
    font-weight: 400;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .scroll {
    flex-grow: 1;
    min-height: 0;
    overflow-y: auto;
    margin: 0 -8px;
    padding: 0 8px 8px;
  }

  /* Conversation */
  .timeline {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .card-row {
    display: flex;
    gap: 10px;
    align-items: flex-start;
  }
  .avatar {
    position: relative;
    border-radius: 50%;
    color: #fff;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-weight: 700;
    flex-shrink: 0;
    overflow: hidden;
  }
  .avatar img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .card {
    flex-grow: 1;
    min-width: 0;
    border-radius: var(--radius-l);
    background: var(--panel);
    border: 0.5px solid var(--panel-border);
    padding: 9px 12px 10px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .card-head {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-small);
    color: var(--text2);
    white-space: nowrap;
    min-width: 0;
  }
  .who {
    color: var(--text);
    font-weight: 600;
    flex-shrink: 0;
  }
  .when {
    flex-shrink: 0;
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .card-head .when {
    font-size: var(--text-small);
  }
  .badge {
    line-height: 20px;
    height: 20px;
    padding: 0 9px;
    border-radius: var(--radius-pill);
    font-size: var(--text-caption);
    font-weight: 600;
    flex-shrink: 0;
  }
  .text {
    margin: 0;
    line-height: 1.45;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .stack-box {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 7px 10px;
    border-radius: var(--radius-m);
    background: var(--field);
    font-size: var(--text-small);
  }
  .stack-title {
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
  }
  .stack-line {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .stack-line.mine {
    font-weight: 600;
  }
  .event {
    display: flex;
    align-items: center;
    gap: 10px;
    padding-left: 7px;
    font-size: var(--text-small);
    color: var(--text2);
    min-width: 0;
  }
  .event .who {
    font-weight: 500;
  }
  .dot {
    width: 16px;
    height: 16px;
    border-radius: var(--radius-pill);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }
  .dot svg {
    width: 10px;
    height: 10px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2.2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .code {
    border-radius: var(--radius-m);
    overflow: hidden;
    border: 0.5px solid var(--panel-border);
    background: var(--win);
  }
  .code-head {
    height: 26px;
    display: flex;
    align-items: center;
    padding: 0 10px;
    font-size: var(--text-caption);
    background: var(--field);
  }
  .code-line {
    display: flex;
    align-items: center;
    min-height: 19px;
    font-family: var(--mono);
    font-size: var(--text-small);
  }
  .code-line.mark {
    background: var(--found-bg);
  }
  .ln {
    width: 34px;
    text-align: right;
    padding-right: 12px;
    color: var(--text2);
    opacity: 0.7;
    flex-shrink: 0;
  }
  .src {
    white-space: pre;
    color: var(--code);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* The thread's comments and the reply, edge to edge under the code. */
  .thread {
    margin: 0 -12px -10px;
    border-top: 0.5px solid var(--sep);
  }

  /* Commits */
  .commits {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .commit {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 44px;
    padding: 0 12px;
    border-radius: var(--radius-m);
    background: var(--panel);
  }
  .commit-dot {
    width: 10px;
    height: 10px;
    border-radius: var(--radius-pill);
    border: 2px solid;
    box-sizing: border-box;
    flex-shrink: 0;
  }
  .commit-text {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: 1.3;
  }
  .sub {
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .add {
    font-size: var(--text-caption);
    color: var(--green);
  }
  .del {
    font-size: var(--text-caption);
    color: var(--red);
  }
  .sha {
    font-size: var(--text-caption);
    color: var(--text2);
    width: 56px;
    text-align: right;
  }

  /* New pull request */
  .form {
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-l);
    background: var(--panel);
    border: 0.5px solid var(--panel-border);
    overflow-y: auto;
  }
  .form-row {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 10px 14px;
    border-top: 0.5px solid var(--sep);
  }
  .form-row:first-child {
    border-top: 0;
  }
  .form-label {
    width: 84px;
    flex-shrink: 0;
    padding-top: 4px;
    color: var(--text2);
  }
  .field {
    flex-grow: 1;
    min-width: 0;
    padding: 4px 9px;
    border-radius: var(--radius-s);
    background: var(--win);
    border: 0.5px solid var(--glass-border);
    line-height: 1.45;
    font: inherit;
    color: var(--text);
    resize: vertical;
  }
  .chips {
    flex-grow: 1;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding-top: 1px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 10px;
    border-radius: var(--radius-pill);
    background: var(--field);
    font-size: var(--text-small);
    font-weight: 500;
    white-space: nowrap;
  }
  .chip.person {
    padding-left: 3px;
  }
  .remove {
    color: var(--text2);
    font-size: var(--text-heading);
    line-height: 1;
  }
  .add-reviewer {
    width: 90px;
    height: 24px;
    border: 0;
    background: transparent;
    color: var(--accent-text);
    font: inherit;
    font-size: var(--text-small);
    outline: none;
  }
  .note {
    font-size: var(--text-caption);
    color: var(--text2);
    line-height: 1.4;
  }
  .note.center {
    text-align: center;
  }
  .side {
    flex-shrink: 0;
    align-self: center;
  }
  .ask-box {
    display: flex;
  }

  /* The box on the right */
  .next {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 12px;
    border-radius: var(--radius-l);
    background: var(--accent-quiet);
    border: 1.5px solid color-mix(in srgb, var(--accent) 30%, transparent);
  }
  .next.ready {
    background: var(--green-soft);
    border-color: color-mix(in srgb, var(--green) 30%, transparent);
  }
  .next-label {
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--accent-ink);
  }
  .next.ready .next-label {
    color: var(--green);
  }
  .next-title {
    font-weight: 600;
    font-size: var(--text-body);
  }
  .next-text {
    font-size: var(--text-small);
    color: var(--text2);
    line-height: 1.4;
  }
  .next .steps {
    margin-top: 6px;
  }
  .next .buttons {
    margin-top: 8px;
  }
  .next.yours .buttons {
    justify-content: flex-start;
  }
  .finish {
    display: inline-flex;
  }
  .finish-count {
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    border-radius: var(--radius-pill);
    background: rgba(255, 255, 255, 0.25);
    font-size: var(--text-caption);
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .viewed-count {
    font-size: var(--text-small);
    color: var(--text2);
    white-space: nowrap;
  }
  .also {
    display: flex;
    flex-direction: column;
    gap: 3px;
    margin-top: 8px;
    padding-top: 8px;
    border-top: 1px solid var(--sep);
    font-size: var(--text-small);
  }
  .also-title {
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
  }
  .block {
    display: flex;
    gap: 7px;
    line-height: 1.4;
  }
  .bullet {
    color: var(--text2);
  }
  .steps {
    display: flex;
    flex-direction: column;
    gap: 7px;
  }
  .step-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .step.waiting {
    color: var(--text2);
  }
  .why {
    font-size: var(--text-caption);
    color: var(--orange);
    overflow-wrap: anywhere;
  }
  .add-reviewer.ask {
    width: auto;
    align-self: flex-start;
    padding: 0;
  }

  .group {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .group-head,
  .group-title {
    display: flex;
    align-items: center;
    font-size: var(--text-caption);
    font-weight: 600;
    color: var(--text2);
    padding: 0 2px 2px;
  }
  .tone-ok {
    color: var(--green);
  }
  .tone-warn {
    color: var(--orange);
  }
  .tone-bad {
    color: var(--red);
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 24px;
    padding: 0 2px;
    font-size: var(--text-small);
    border-radius: var(--radius-s);
    width: 100%;
  }
  .check:disabled {
    opacity: 1;
  }
  .reviewer {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 26px;
    padding: 0 2px;
    font-size: var(--text-small);
  }
  .base-line {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-radius: var(--radius-l);
    font-size: var(--text-small);
    line-height: 1.35;
  }
  .sep {
    height: 1px;
    background: var(--sep);
    flex-shrink: 0;
  }
  .method-row {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: var(--text-small);
  }
  .segmented {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: var(--radius-m);
    background: var(--field);
  }
  .segmented button {
    min-width: 70px;
    height: 24px;
    padding: 0 10px;
    border-radius: var(--radius-s);
    text-align: center;
    font-size: var(--text-small);
  }
  .segmented button.on {
    background: var(--seg-on);
    box-shadow: var(--seg-shadow);
    font-weight: 600;
  }
  .message {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 8px 10px;
    border-radius: var(--radius-m);
    background: var(--win);
    border: 0.5px solid var(--glass-border);
    font-size: var(--text-small);
  }
  .message-title {
    font-weight: 600;
  }
  .message-body {
    line-height: 1.4;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
    white-space: pre-wrap;
  }
  .switch-row {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: var(--text-small);
    width: 100%;
    white-space: normal;
  }
  .switch-row:disabled {
    opacity: 0.55;
  }
  .switch-text {
    flex-grow: 1;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .switch {
    position: relative;
    width: 32px;
    height: 18px;
    border-radius: var(--radius-pill);
    background: var(--switch-off);
    flex-shrink: 0;
    transition: background-color 0.15s;
  }
  .switch.on {
    background: var(--accent);
  }
  .knob {
    position: absolute;
    left: 2px;
    top: 2px;
    width: 14px;
    height: 14px;
    border-radius: var(--radius-pill);
    background: #fff;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.25);
    transition: transform 0.15s;
  }
  .switch.on .knob {
    transform: translateX(14px);
  }

  .merge {
    height: 34px;
  }

  .merged {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px;
    border-radius: var(--radius-l);
    background: var(--green-soft);
  }
  .merged-icon {
    width: 28px;
    height: 28px;
    border-radius: var(--radius-pill);
    background: var(--green);
    color: #fff;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }
  .merged-icon svg {
    width: 14px;
    height: 14px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2.2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .merged-text {
    flex-grow: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: var(--text-small);
  }
  .merged-title {
    font-weight: 600;
    font-size: var(--text-body);
  }
  .restack {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    border-radius: var(--radius-l);
    border: 1.5px solid;
  }
  .restack-title {
    font-weight: 600;
  }
  .step {
    display: flex;
    gap: 8px;
    font-size: var(--text-small);
    line-height: 1.4;
  }
  .step .dot {
    margin-top: 1px;
  }
  .num {
    width: 18px;
    height: 18px;
    border-radius: var(--radius-pill);
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--field);
    color: var(--text2);
    font-size: var(--text-caption);
    font-weight: 700;
    flex-shrink: 0;
  }
  .buttons {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }
  .capsule {
    min-width: 88px;
    height: 30px;
    padding: 0 14px;
    border-radius: var(--radius-pill);
    text-align: center;
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    font-weight: 500;
  }
  .skipped {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border-radius: var(--radius-l);
    background: var(--field);
    font-size: var(--text-small);
    line-height: 1.4;
  }
</style>
