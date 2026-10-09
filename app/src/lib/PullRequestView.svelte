<script lang="ts">
  // The Pull Request screen of one branch: its stack as a strip of soft arrows, then the pull
  // request (conversation or commits) on the left and whether it can merge, and how, on the
  // right. A branch without one gets the form that opens it. After the base of a stack merged,
  // the rest of the stack can be restacked onto the trunk from here.
  import { untrack } from "svelte";
  import { api } from "./api";
  import { github } from "./github.svelte";
  import type { Request } from "./confirm.svelte";
  import type { BranchContext } from "./branches";
  import type { Action, History, MergeMethodName, Landing, PullCheck, PullEntry, PullRequest, PullSummary, ReviewState, Stack, StackPreview } from "./types";
  import { lane, person, plate, relativeTime, shortId, splitPath, tint } from "./format";
  import { softArrows } from "./shapes";
  import { editStackRequest, freshDraft, planOf } from "./stack";
  import {
    autoMergeRequest,
    createRequest,
    mergeMessage,
    mergeRequest,
    METHODS,
    NEW_NUMBER,
    readBody,
    readyRequest,
    restackPushRequest,
    type NewPull,
    type StackLine,
  } from "./pr";

  let {
    branch,
    history,
    ctx,
    colorOf,
    version,
    run,
    onBranch,
    copy,
  }: {
    branch: string;
    history: History;
    ctx: BranchContext;
    colorOf: (name: string) => number;
    /** Goes up on every reload, so the pull request loads again. */
    version: number;
    run: (request: Request | Promise<Request>) => Promise<boolean>;
    /** Show another branch of the stack. */
    onBranch: (name: string) => void;
    copy: (text: string) => void;
  } = $props();

  const HEIGHT = 42;

  const gh = $derived(github.repo);
  const summary = $derived(github.pullOf(branch));
  let pr = $state<PullRequest | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let stack = $state<Stack | null>(null);
  let tab = $state<"conversation" | "commits">("conversation");
  let method = $state<MergeMethodName | null>(null);
  let deleteBranch = $state(true);
  let me = $state<string | null>(null);
  let stripWidth = $state(0);

  api.githubAccount().then((account) => (me = account?.login ?? null), () => {});

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

  async function load(number: number) {
    loading = !pr || pr.number !== number;
    try {
      const fresh = await api.githubPull(number);
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

  // Coming to another branch starts on its conversation.
  $effect(() => {
    void branch;
    tab = "conversation";
    method = null;
    deleteBranch = true;
    restackSkipped = false;
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
  const trunk = $derived(gh && pr ? pr.settings.defaultBranch : (stack?.trunk.replace(/^[^/]+\//, "") ?? history.trunk ?? "main"));
  const stackNames = $derived(stack && stack.branches.length > 1 ? stack.branches.map((b) => b.name) : []);
  const arrows = $derived(stripWidth > 0 && stackNames.length ? softArrows(stackNames.length, stripWidth, HEIGHT).map((shape, i) => ({ ...shape, name: stackNames[i] })) : []);

  function stateWord(p: PullSummary): string {
    return p.state === "merged" ? "Merged" : p.state === "closed" ? "Closed" : p.draft ? "Draft" : "Open";
  }

  function stripStatus(name: string): string {
    const p = github.pullOf(name);
    if (p) return `#${p.number} · ${p.number === pr?.number ? status.label : stateWord(p)}`;
    const b = stack?.branches.find((x) => x.name === name);
    const toPush = b?.upstream ? b.ahead : (stack?.commits.filter((c) => c.branch === name).length ?? 0);
    return toPush ? `No pull request · ${toPush} to push` : "No pull request";
  }

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
    pr && pr.state === "open" ? (github.pulls.find((p) => p.state === "merged" && p.head !== pr!.head && pr!.commits.some((c) => c.sha === p.headSha)) ?? null) : null,
  );
  const basePull = $derived(pr && pr.base !== pr.settings.defaultBranch ? github.openPullOf(pr.base) : null);

  const failing = $derived(pr ? pr.checks.filter((c) => c.state === "fail") : []);
  const running = $derived(pr ? pr.checks.filter((c) => c.state === "run") : []);
  const blocks = $derived.by(() => {
    if (!pr || pr.state !== "open") return [];
    const list: string[] = [];
    if (pr.draft) list.push("Draft, mark it ready for review first");
    if (basePull) list.push(`Merge #${basePull.number} first, it is the base of this pull request`);
    if (mergedBelow) list.push(`Restack onto ${trunk}, #${mergedBelow.number} is merged`);
    const strict = pr.mergeableState === "blocked";
    if (strict && failing.length) list.push(`${plural(failing.length, "check")} failing: ${failing.map((c) => c.name).join(", ")}`);
    if (strict && running.length) list.push("Checks are running");
    const asked = pr.reviews.filter((r) => r.state === "changesRequested").map((r) => r.author.login);
    if (asked.length) list.push(`Changes requested by ${asked.join(", ")}`);
    if (pr.requiredApprovals && approvals < pr.requiredApprovals) list.push(`Needs ${plural(pr.requiredApprovals - approvals, "approval")}`);
    if (pr.mergeable === false || pr.mergeableState === "dirty") list.push(`Conflicts with ${pr.base}: resolve them first`);
    if (pr.mergeableState === "behind") list.push(`${pr.base} has newer commits: update the branch first`);
    if (!pr.settings.canPush) list.push("Your account can’t push to this repository, so it can’t merge");
    if (strict && !list.length) list.push(`GitHub’s rules for ${pr.base} block merging for now`);
    return list;
  });
  const checking = $derived(!!pr && pr.state === "open" && pr.mergeableState === "unknown");
  const canMerge = $derived(!!pr && pr.state === "open" && !blocks.length && !checking && pr.mergeable !== false);

  const methods = $derived(pr ? METHODS.filter((m) => (m.id === "merge" ? pr!.settings.mergeCommit : m.id === "squash" ? pr!.settings.squash : pr!.settings.rebase)) : []);
  const chosen = $derived(method && methods.some((m) => m.id === method) ? method : (methods.find((m) => m.id === "squash") ?? methods[0])?.id ?? "merge");
  const preview = $derived(pr && gh ? mergeMessage(gh, pr, chosen) : { title: null, message: null });
  const autoMergeNote = $derived.by(() => {
    if (!pr) return "";
    if (!pr.settings.autoMerge) return "Turned off for this repository on GitHub";
    if (pr.base !== pr.settings.defaultBranch) return `Once it goes into ${pr.settings.defaultBranch}`;
    return "When checks pass and it is approved";
  });
  const autoMergeOff = $derived(!pr || !pr.settings.autoMerge || pr.base !== pr.settings.defaultBranch || !pr.settings.canPush);

  const baseLine = $derived.by((): { tone: "ok" | "warn" | "stack" | "grey"; text: string } | null => {
    if (!pr || pr.state !== "open") return null;
    if (mergedBelow) return { tone: "warn", text: `Based on ${mergedBelow.head}, which is merged. Restack to move it onto ${trunk}.` };
    if (basePull) return { tone: "stack", text: `Based on ${pr.base} (#${basePull.number}). It can merge once #${basePull.number} is in ${basePull.base}.` };
    if (pr.mergeable === false || pr.mergeableState === "dirty") return { tone: "warn", text: `Conflicts with ${pr.base}` };
    if (pr.mergeableState === "behind") return { tone: "warn", text: `Behind ${pr.base}` };
    if (checking) return { tone: "grey", text: "GitHub is checking whether it merges cleanly…" };
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

  function openCheck(check: PullCheck) {
    if (check.url?.startsWith("https://github.com/")) api.openGitHub(check.url).catch(() => {});
  }

  // --- Merging ---

  function merge() {
    if (!pr || !gh || !canMerge) return;
    act(mergeRequest(ctx, gh, pr, chosen, deleteBranch));
  }

  function toggleAutoMerge() {
    if (!pr || autoMergeOff) return;
    act(autoMergeRequest(pr, pr.autoMerge ? null : chosen));
  }

  // --- Restacking after the base merged ---

  interface Restack {
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
  let restack = $state<Restack | null>(null);
  let restackSkipped = $state(false);

  /** Where the stack goes: a branch that has the merge, fetched first when none has it yet, so
   *  the merged commits never leave the stack for a base without them. */
  async function landing(s: Stack, merged: PullRequest): Promise<Landing | null> {
    if (!merged.mergeCommit) return null;
    const candidates = [...new Set([s.trunk, ...(gh ? [`${gh.remote}/${merged.base}`] : []), merged.base])];
    const found = await api.landedOn(merged.mergeCommit, candidates);
    if (found || !gh) return found;
    await api.fetchInBackground(gh.remote).catch(() => {});
    return api.landedOn(merged.mergeCommit, candidates);
  }

  /** The stack built on `merged`, here, with its commits left out and moved onto a branch that
   *  has the merge. */
  async function planRestack(merged: PullRequest): Promise<Restack | null> {
    const shas = new Set(merged.commits.map((c) => c.sha));
    const candidates = github.pulls.filter((p) => p.state === "open" && p.head !== merged.head && ctx.history.refs.some((r) => r.kind === "local" && r.name === p.head));
    for (const candidate of candidates) {
      let s = await api.stack(candidate.head).catch(() => null);
      if (!s) continue;
      const drop = s.commits.filter((c) => shas.has(c.id)).map((c) => c.id);
      if (!drop.length) continue;
      const names = new Set(s.branches.map((b) => b.name));
      const retarget = github.pulls.filter((p) => p.state === "open" && names.has(p.head) && p.base === merged.head);
      const stuck = (problem: string): Restack => ({ merged, stack: s!, drop, preview: { changed: false } as StackPreview, retarget, problem });
      if (!merged.commitsComplete)
        return stuck(`#${merged.number} has more commits than GitHub lists (250), so Oxbow can’t tell which ones to leave out. Use Edit Stack to drop them by hand.`);
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
    const detail = merged.number === pr?.number ? pr : await api.githubPull(merged.number);
    const plan = restack?.merged.number === merged.number ? restack : await planRestack(detail);
    if (!plan || plan.problem) return;
    const draft = freshDraft(plan.stack, true);
    for (const id of plan.drop) draft.acts[id] = "drop";
    draft.items = draft.items.filter((item) => !(item.kind === "branch" && item.name === merged.head));
    if (!(await run(editStackRequest(ctx, plan.stack, draft, plan.preview)))) return;
    const forced = plan.preview.branches.filter((b) => b.forcePush);
    const ups = plan.stack.branches.filter((b) => b.upstream && forced.some((f) => f.name === b.name));
    const push: (Action & { kind: "pushBranches" }) | null = ups.length
      ? { kind: "pushBranches", remote: ups[0].upstream!.remote, branches: ups.filter((b) => b.upstream!.remote === ups[0].upstream!.remote).map((b) => ({ branch: b.name, upstream: b.upstream!.branch })) }
      : null;
    const next = await restackPushRequest(ctx, push, plan.retarget.map((p) => ({ number: p.number, base: merged.base })));
    if (next) await run(next);
    await reload();
  }

  const restackSteps = $derived.by(() => {
    if (!restack || restack.problem) return [];
    const { stack: s, drop, preview: p, retarget, merged } = restack;
    const kept = s.commits.length - drop.length;
    const names = s.branches.filter((b) => b.name !== merged.head).map((b) => b.name);
    const steps = [`Rebase ${plural(kept, "commit")} of ${names.join(" and ")} onto ${s.trunk}, leaving out ${plural(drop.length, "commit")} ${merged.base} has now`];
    for (const r of retarget) steps.push(`Change the base of #${r.number} to ${merged.base}`);
    const forced = p.branches.filter((b) => b.forcePush).length;
    if (forced) steps.push(`Force push ${plural(forced, "branch")} with --force-with-lease`);
    steps.push(p.conflict ? `Stops on a conflict in “${p.conflict.summary}” for you to resolve` : "No conflicts expected (dry run)");
    return steps;
  });

  // --- A new pull request ---

  let form = $state<{ branch: string; title: string; body: string; base: string; draft: boolean; reviewers: string[]; adding: string; suggested: string[] } | null>(null);

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
    const tipOf = (name: string) => stack!.commits.findIndex((c) => c.id === stack!.branches.find((b) => b.name === name)?.tip);
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
    suggest(base);
  });

  async function suggest(base: string) {
    const name = branch;
    const ref = base === below ? base : (stack?.trunk ?? base);
    const owners = await api.codeOwners(ref, name).catch(() => [] as string[]);
    if (!form || form.branch !== name) return;
    form.suggested = owners.filter((o) => o !== me);
    form.reviewers = [...form.suggested];
  }

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
      const p = github.pullOf(b.name);
      return { branch: b.name, number: b.name === branch ? NEW_NUMBER : p ? `#${p.number}` : null, merged: p?.state === "merged" };
    });
    return lines.some((l) => l.branch !== branch && l.number) ? lines : null;
  });
  /** The stack's other open pull requests, oldest first. */
  const others = $derived(stack ? [...stack.branches].reverse().map((b) => github.openPullOf(b.name)).filter((p): p is PullSummary => !!p && p.head !== branch) : []);

  const newPull = $derived.by((): NewPull | null => {
    if (!form || !gh) return null;
    const t = localRef?.tracking;
    const remote = t && !t.gone ? t.remote : gh.remote;
    const needsPush = !t || t.gone || t.ahead > 0;
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

{#snippet dot(icon: string, bg: string, fg: string)}
  <span class="dot" style:background={bg} style:color={fg}>
    <svg viewBox="0 0 16 16"><path d={ICONS[icon] ?? ICONS.dot} /></svg>
  </span>
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

    {#if !gh}
      <div class="empty">This repository has no remote on github.com.</div>
    {:else if github.problem && !github.pulls.length}
      <div class="empty">
        <span>{github.problem}</span>
        <button class="capsule" onclick={() => api.openSettings()}>Open Settings</button>
      </div>
    {:else if summary && !pr}
      <div class="empty">{error ?? (loading ? `Loading #${summary.number}…` : "")}</div>
    {:else if pr}
      <div class="title-block">
        <div class="title-row">
          <h1>{pr.title}</h1>
          <span class="number">#{pr.number}</span>
          {#if status.label}
            {@const [bg, fg] = toneOf(status.tone)}
            <span class="status" style:background={bg} style:color={fg}>{status.label}</span>
          {/if}
        </div>
        <div class="meta-row">
          {@render capsule(pr.head)}
          <svg class="icon small" viewBox="0 0 16 16"><path d="M13 8H3.5M7 4.5 3.5 8 7 11.5" /></svg>
          {@render capsule(pr.base)}
          <span class="ellipsis">{pr.author.login} opened {relativeTime(pr.created).toLowerCase()} · {plural(pr.commitCount, "commit")} · {plural(pr.changedFiles, "file")} · +{pr.additions} −{pr.deletions}</span>
        </div>
      </div>
      <div class="tabs" role="tablist" aria-label="Pull request">
        <button role="tab" class="tab" class:on={tab === "conversation"} aria-selected={tab === "conversation"} onclick={() => (tab = "conversation")}
          style:--on-bg={tint(colorOf(pr.head))} style:--on-fg={plate(colorOf(pr.head))}>Conversation</button>
        <button role="tab" class="tab" class:on={tab === "commits"} aria-selected={tab === "commits"} onclick={() => (tab = "commits")}
          style:--on-bg={tint(colorOf(pr.head))} style:--on-fg={plate(colorOf(pr.head))}>Commits {pr.commits.length}</button>
        {#if error}<span class="load-error">{error}</span>{/if}
      </div>

      <div class="scroll">
        {#if tab === "conversation"}
          <div class="timeline">
            <div class="card-row">
              {@render avatar(pr.author.login, pr.author.avatarUrl, 30)}
              <div class="card">
                <div class="card-head"><span class="who">{pr.author.login}</span><span class="ellipsis">opened this pull request</span><span class="when">· {relativeTime(pr.created)}</span></div>
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
                      <span class="who">{first.author.login}</span><span class="ellipsis">commented on {e.path}</span><span class="when">· {relativeTime(e.time)}</span>
                      <span class="grow"></span>
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
                    <p class="text selectable">{first.body}</p>
                    {#each e.comments.slice(1) as reply, j (j)}
                      <div class="reply">
                        {@render avatar(reply.author.login, reply.author.avatarUrl, 20)}
                        <span class="who">{reply.author.login}</span>
                        <span class="reply-text selectable">{reply.body}</span>
                      </div>
                    {/each}
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
    {:else if form && gh}
      <div class="title-block">
        <h1>New pull request</h1>
        <span class="muted small-text">
          {#if toPush}{branch} has {plural(toPush, "commit")} that {toPush === 1 ? "is" : "are"} not on GitHub yet.{:else}{branch} is on GitHub; this opens a pull request for it.{/if}
        </span>
      </div>
      <div class="form">
        <label class="form-row">
          <span class="form-label">Title</span>
          <input class="field" bind:value={form.title} placeholder="What it does" />
        </label>
        <div class="form-row">
          <span class="form-label">Base</span>
          <span class="chips">
            {#each bases as b (b)}
              <button class="chip" class:on={form.base === b} style:background={form.base === b ? tint(colorOf(b), "label") : undefined} style:color={form.base === b ? plate(colorOf(b)) : undefined} onclick={() => pickBase(b)}>{b}</button>
            {/each}
            {#if below && form.base === below}<span class="note">Picked from the stack: {branch} is built on {below}</span>{/if}
          </span>
        </div>
        <label class="form-row">
          <span class="form-label">Description</span>
          <textarea class="field" rows="5" bind:value={form.body} placeholder="Optional"></textarea>
        </label>
        {#if stackLines}
          <div class="form-row">
            <span class="form-label"></span>
            <span class="note">Stack: {stackLines.map((l) => (l.branch === branch ? "this one" : (l.number ?? l.branch))).join(" → ")} (added by Oxbow)</span>
          </div>
        {/if}
        <div class="form-row">
          <span class="form-label">Reviewers</span>
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
        </div>
      </div>
    {:else}
      <div class="empty">{loading ? "Loading…" : ""}</div>
    {/if}
  </section>

  <aside class="box" aria-label="Merge">
    {#if pr && pr.state === "open"}
      {#if blocks.length}
        <div class="blocks">
          <span class="blocks-title">Can’t merge yet</span>
          {#each blocks as b (b)}
            <span class="block"><span class="bullet">•</span><span>{b}</span></span>
          {/each}
          {#if pr.draft && pr.settings.canPush}
            <button class="small-btn" onclick={() => pr && act(readyRequest(pr))}>Ready for Review…</button>
          {/if}
        </div>
      {/if}
      {#if pr.checks.length}
        <div class="group">
          <div class="group-head"><span class="grow">Checks</span><span class="tone-{checksSummary.tone}">{checksSummary.text}</span></div>
          {#each pr.checks as ck (ck.name)}
            {@const [bg, fg] = toneOf(ck.state === "ok" ? "ok" : ck.state === "fail" ? "bad" : ck.state === "run" ? "warn" : "grey")}
            <button class="check" onclick={() => openCheck(ck)} disabled={!ck.url?.startsWith("https://github.com/")} title={ck.url ? "Open on GitHub" : ck.name}>
              {@render dot(ck.state, bg, fg)}
              <span class="ellipsis grow">{ck.name}{#if ck.detail}<span class="muted"> {ck.detail}</span>{/if}</span>
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
      </div>
      {#if baseLine}
        {@const [bg, fg] = baseLine.tone === "stack" ? [tint(colorOf(pr.head)), plate(colorOf(pr.head))] : toneOf(baseLine.tone)}
        <div class="base-line" style:background={bg}>
          <svg class="icon small" viewBox="0 0 16 16" style:color={fg}
            ><path d={baseLine.tone === "ok" ? ICONS.ok : baseLine.tone === "stack" ? "M3 6.5h10v7H3zM4.5 4.5h7M6 2.5h4" : "M8 3.5v5.5M8 12.2v.1"} /></svg
          >
          <span class="grow">{baseLine.text}</span>
          {#if mergedBelow}<button class="small-btn" onclick={() => mergedBelow && startRestack(mergedBelow)}>Restack…</button>{/if}
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
        {:else}
          <span class="muted">Each commit goes onto {pr.base} as it is.</span>
        {/if}
      </div>
      {#if canMerge}
        {@render toggle(deleteBranch, `Delete ${pr.head} after merging`, pr.settings.deleteBranchOnMerge ? "GitHub deletes it there itself; here too" : "On GitHub and here", false, () => (deleteBranch = !deleteBranch))}
      {:else}
        {@render toggle(!!pr.autoMerge, "Merge automatically when ready", autoMergeNote, autoMergeOff, toggleAutoMerge)}
      {/if}
      <button class="primary merge" disabled={!canMerge} onclick={merge}>
        {pr.autoMerge && !canMerge ? "Will Merge When Ready" : METHODS.find((m) => m.id === chosen)?.button}
      </button>
      <span class="note center">
        {#if canMerge}Merges on GitHub, then updates your local {pr.base}.{:else if pr.autoMerge}GitHub merges it once it is ready.{:else if checking}GitHub is still checking it.{:else}Merging is blocked until the items above are done.{/if}
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
      {#if restack && !restackSkipped}
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
            <button class="primary small" disabled={!!restack.problem} onclick={() => restack && startRestack(restack.merged)}>Restack…</button>
          </div>
        </div>
      {:else if restack}
        <div class="skipped">
          <span class="grow">Restack skipped. The pull requests built on it still say “Restack” until it is done.</span>
          <button class="small-btn" onclick={() => (restackSkipped = false)}>Undo</button>
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
          >Open a pull request into {form.base}{form.base === below ? `, so reviewers see only these ${plural(newPull.commits, "commit")}` : ""}</span
        ></span
      >
      {#if newPull.stack && others.length}
        <span class="step"><span class="num">{newPull.push ? 3 : 2}</span><span>Add the stack list to {others.map((o) => `#${o.number}`).join(", ")} and the new pull request</span></span>
      {/if}
      <div class="sep"></div>
      {@render toggle(form.draft, "Create as draft", "Reviewers hear about it when you mark it ready", false, () => form && (form.draft = !form.draft))}
      <button class="primary merge" disabled={!form.title.trim()} onclick={create}>{newPull.push ? "Push and Create" : "Create Pull Request"}</button>
    {/if}
  </aside>
</div>

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
    font-size: 12px;
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
    font-size: 11px;
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
    font-size: 12px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .arrow-sub {
    font-size: 11px;
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
    font-size: 18px;
    font-weight: 600;
    line-height: 1.3;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .number {
    font-size: 18px;
    color: var(--text2);
    flex-shrink: 0;
  }
  .status {
    line-height: 22px;
    height: 22px;
    padding: 0 10px;
    border-radius: 11px;
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
    flex-shrink: 0;
  }
  .meta-row {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text2);
    white-space: nowrap;
    min-width: 0;
  }
  .capsule-label {
    line-height: 20px;
    height: 20px;
    padding: 0 9px;
    border-radius: 10px;
    font-size: 11px;
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
    border-radius: 13px;
    border: 1px solid var(--sep);
    font-size: 12px;
    font-weight: 500;
  }
  .tab.on {
    background: var(--on-bg);
    border-color: color-mix(in srgb, var(--on-fg) 30%, transparent);
    color: var(--on-fg);
    font-weight: 600;
  }
  .load-error {
    font-size: 12px;
    color: var(--red);
    margin-left: 8px;
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
    border-radius: 12px;
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
    font-size: 12px;
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
    font-size: 11px;
    color: var(--text2);
  }
  .card-head .when {
    font-size: 12px;
  }
  .badge {
    line-height: 20px;
    height: 20px;
    padding: 0 9px;
    border-radius: 10px;
    font-size: 11px;
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
    border-radius: 9px;
    background: var(--field);
    font-size: 12px;
  }
  .stack-title {
    font-size: 11px;
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
    font-size: 12px;
    color: var(--text2);
    min-width: 0;
  }
  .event .who {
    font-weight: 500;
  }
  .dot {
    width: 16px;
    height: 16px;
    border-radius: 8px;
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
    border-radius: 9px;
    overflow: hidden;
    border: 0.5px solid var(--panel-border);
    background: var(--win);
  }
  .code-head {
    height: 26px;
    display: flex;
    align-items: center;
    padding: 0 10px;
    font-size: 11px;
    background: var(--field);
  }
  .code-line {
    display: flex;
    align-items: center;
    min-height: 19px;
    font-family: var(--mono);
    font-size: 11.5px;
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
  .reply {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-top: 6px;
    border-top: 0.5px solid var(--sep);
    font-size: 12px;
    min-width: 0;
  }
  .reply-text {
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
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
    border-radius: 10px;
    background: var(--panel);
  }
  .commit-dot {
    width: 10px;
    height: 10px;
    border-radius: 5px;
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
    font-size: 11px;
    color: var(--text2);
  }
  .add {
    font-size: 11px;
    color: var(--green);
  }
  .del {
    font-size: 11px;
    color: var(--red);
  }
  .sha {
    font-size: 11px;
    color: var(--text2);
    width: 56px;
    text-align: right;
  }

  /* New pull request */
  .form {
    display: flex;
    flex-direction: column;
    border-radius: 12px;
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
    border-radius: 7px;
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
    border-radius: 12px;
    background: var(--field);
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
  }
  .chip.person {
    padding-left: 3px;
  }
  .remove {
    color: var(--text2);
    font-size: 14px;
    line-height: 1;
  }
  .add-reviewer {
    width: 90px;
    height: 24px;
    border: 0;
    background: transparent;
    color: var(--accent-text);
    font: inherit;
    font-size: 12px;
    outline: none;
  }
  .note {
    font-size: 11px;
    color: var(--text2);
    line-height: 1.4;
  }
  .note.center {
    text-align: center;
  }

  /* The box on the right */
  .blocks {
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 10px 12px;
    border-radius: 12px;
    background: var(--orange-soft);
    font-size: 12px;
  }
  .blocks-title {
    font-weight: 600;
    color: var(--orange);
  }
  .block {
    display: flex;
    gap: 7px;
    line-height: 1.4;
  }
  .bullet {
    color: var(--orange);
  }
  .blocks .small-btn {
    align-self: flex-start;
    margin-top: 4px;
  }
  .small-btn {
    flex-shrink: 0;
    height: 26px;
    padding: 0 12px;
    border-radius: 13px;
    background: var(--win);
    border: 0.5px solid var(--glass-border);
    font-weight: 500;
    font-size: 12px;
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
    font-size: 11px;
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
    font-size: 12px;
    border-radius: 6px;
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
    font-size: 12px;
  }
  .base-line {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border-radius: 10px;
    font-size: 12px;
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
    font-size: 12px;
  }
  .segmented {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: 9px;
    background: var(--field);
  }
  .segmented button {
    min-width: 70px;
    height: 24px;
    padding: 0 10px;
    border-radius: 7px;
    text-align: center;
    font-size: 12px;
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
    border-radius: 9px;
    background: var(--win);
    border: 0.5px solid var(--glass-border);
    font-size: 12px;
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
    font-size: 12px;
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
    border-radius: 9px;
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
    border-radius: 7px;
    background: #fff;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.25);
    transition: transform 0.15s;
  }
  .switch.on .knob {
    transform: translateX(14px);
  }
  .primary {
    border-radius: 17px;
    text-align: center;
    background: var(--accent);
    color: #fff;
    font-weight: 600;
    flex-shrink: 0;
  }
  .primary.merge {
    height: 34px;
  }
  .primary:disabled {
    background: var(--field);
    color: var(--text2);
  }
  .primary.small {
    min-width: 88px;
    height: 30px;
    padding: 0 16px;
    border-radius: 15px;
  }
  .merged {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px;
    border-radius: 12px;
    background: var(--green-soft);
  }
  .merged-icon {
    width: 28px;
    height: 28px;
    border-radius: 14px;
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
    font-size: 12px;
  }
  .merged-title {
    font-weight: 600;
    font-size: 13px;
  }
  .restack {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    border-radius: 12px;
    border: 1.5px solid;
  }
  .restack-title {
    font-weight: 600;
  }
  .step {
    display: flex;
    gap: 8px;
    font-size: 12px;
    line-height: 1.4;
  }
  .step .dot {
    margin-top: 1px;
  }
  .num {
    width: 18px;
    height: 18px;
    border-radius: 9px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--field);
    color: var(--text2);
    font-size: 10px;
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
    border-radius: 15px;
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
    border-radius: 12px;
    background: var(--field);
    font-size: 12px;
    line-height: 1.4;
  }
</style>
