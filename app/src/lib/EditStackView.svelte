<script lang="ts">
  // Edit Stack: the stack's commits on its line, newest at the top, with a branch line above
  // each branch's commits. Each commit gets an action (pick, reword, edit, squash, fixup, drop)
  // and can be dragged, or moved with ⌥↑↓, also across a branch line into that branch. On the
  // right, what the stack looks like afterwards, from a dry run of the plan.
  import { untrack } from "svelte";
  import { api } from "./api";
  import type { Request } from "./confirm.svelte";
  import { confirm } from "./confirm.svelte";
  import type { BranchContext } from "./branches";
  import { lane, plate, shortId, tint } from "./format";
  import { keys, withKeys } from "./keys";
  import Menu, { type MenuEntry } from "./Menu.svelte";
  import { oplog, undoRequest } from "./oplog.svelte";
  import { softArrows } from "./shapes";
  import {
    ACTIONS,
    combinedMessage,
    dragItem,
    editStackRequest,
    foldTarget,
    foldsInto,
    freshDraft,
    itemKey,
    keptCount,
    moveItem,
    planOf,
    pushBranchesRequest,
    squashKey,
    type Draft,
    type Item,
  } from "./stack";
  import type { Stack, StackPreview, StepAction } from "./types";

  let {
    branch,
    ctx,
    colorOf,
    version,
    run,
    onClose,
  }: {
    /** A branch of the stack; the checked-out one when null. */
    branch: string | null;
    ctx: BranchContext;
    colorOf: (name: string) => number;
    /** Goes up on every reload, so the stack loads again. */
    version: number;
    run: (request: Request | Promise<Request>) => Promise<boolean>;
    onClose: () => void;
  } = $props();

  let stack = $state<Stack | null>(null);
  let error = $state<string | null>(null);
  let draft = $state<Draft>({ items: [], acts: {}, messages: {}, onto: false });
  let selected = $state<string | null>(null);
  let preview = $state<StackPreview | null>(null);
  let previewError = $state<string | null>(null);
  let checking = $state(false);
  let menu = $state<{ x: number; y: number; entries: MenuEntry[] } | null>(null);
  /** After a rebase: what it did, until the plan changes again. */
  let done = $state<{ line: string; push: string[]; moved: string[] } | null>(null);

  // The stack loads again after any change to the repository, unless a plan is being made.
  let loadedFor = "";
  $effect(() => {
    const key = `${branch}`;
    version;
    untrack(() => {
      if (key === loadedFor && edited()) return;
      loadedFor = key;
      load();
    });
  });

  /** Moving onto the newest trunk is on at first, as in the design; after that it stays as set. */
  let firstLoad = true;

  async function load() {
    try {
      const next = await api.stack(branch);
      stack = next;
      error = null;
      draft = freshDraft(next, next.newer > 0 && (firstLoad || draft.onto));
      firstLoad = false;
      selected ??= next.commits[0]?.id ?? null;
      if (selected && !next.commits.some((c) => c.id === selected)) selected = next.commits[0]?.id ?? null;
    } catch (err) {
      stack = null;
      error = String(err);
    }
  }

  /** The plan differs from the stack as it is. */
  function edited(): boolean {
    if (!stack) return false;
    const fresh = freshDraft(stack).items.map(itemKey).join();
    return draft.onto || Object.keys(draft.acts).length > 0 || draft.items.map(itemKey).join() !== fresh;
  }

  // The dry run follows the plan, a moment after it stops changing.
  let timer: ReturnType<typeof setTimeout> | undefined;
  let asked = 0;
  $effect(() => {
    if (!stack) return;
    const plan = planOf(stack, draft);
    JSON.stringify(plan);
    clearTimeout(timer);
    const mine = ++asked;
    checking = true;
    timer = setTimeout(async () => {
      try {
        const next = await api.stackPreview(plan);
        if (mine !== asked) return;
        preview = next;
        previewError = null;
      } catch (err) {
        if (mine !== asked) return;
        preview = null;
        previewError = String(err);
      } finally {
        if (mine === asked) checking = false;
      }
    }, 250);
  });

  const byId = $derived(new Map(stack?.commits.map((c) => [c.id, c]) ?? []));
  const actOf = (id: string): StepAction => draft.acts[id] ?? "pick";
  const isFold = (id: string) => (actOf(id) === "squash" || actOf(id) === "fixup") && !!foldTarget(draft, id);

  /** The branch each item sits in on screen: the branch line above it. */
  const owners = $derived.by(() => {
    const out: string[] = [];
    let current = stack?.top ?? "";
    for (const item of draft.items) {
      if (item.kind === "branch") current = item.name;
      out.push(current);
    }
    return out;
  });

  const commitCount = $derived(draft.items.filter((i) => i.kind === "commit").length);
  const kept = $derived(keptCount(draft));
  const changed = $derived(!!preview?.changed);

  function setAct(id: string, act: StepAction) {
    done = null;
    const next = { ...draft.acts };
    if (act === "pick") delete next[id];
    else next[id] = act;
    draft = { ...draft, acts: next };
    selected = id;
  }

  function move(index: number, dir: -1 | 1) {
    if (!stack) return;
    const next = moveItem(draft.items, index, dir, stack.top);
    if (next) {
      done = null;
      draft = { ...draft, items: next };
    }
  }

  function resetPlan() {
    if (!stack) return;
    done = null;
    draft = freshDraft(stack);
  }

  function toggleOnto() {
    if (!stack?.newer) return;
    done = null;
    draft = { ...draft, onto: !draft.onto };
  }

  /** The subject line of a reword, typed in place; the rest of the message stays. */
  function reword(id: string, subject: string) {
    const original = draft.messages[id] ?? byId.get(id)?.message ?? "";
    const rest = original.includes("\n") ? original.slice(original.indexOf("\n")) : "";
    draft = { ...draft, messages: { ...draft.messages, [id]: subject + rest } };
  }

  const subjectOf = (id: string) => (draft.messages[id] ?? byId.get(id)?.message ?? "").split("\n")[0];

  function openActions(event: MouseEvent, id: string) {
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    selected = id;
    menu = {
      x: box.left,
      y: box.bottom + 4,
      entries: ACTIONS.map((a) => ({ kind: "item", label: `${a.label}  ·  ${a.hint}`, icon: ICONS[a.id], keys: a.key, danger: a.id === "drop", run: () => setAct(id, a.id) })),
    };
  }

  const ICONS: Record<StepAction, string> = {
    pick: "M3.5 8.5 6.5 11.5 12.5 4.5",
    reword: "M10.5 2.5l3 3L6 13H3v-3z",
    edit: "M5 3v10M11 3v10",
    squash: "M4.5 2v12M4.5 4.5c0 3.5 7 2.5 7 6v3.5",
    fixup: "M4.5 2v12M4.5 4.5c0 3.5 7 2.5 7 6v3.5",
    drop: "M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.7 9h5.6l.7-9",
  };

  // --- Dragging a commit or a branch line ------------------------------------------------------
  let rows = $state<HTMLElement[]>([]);
  let drag = $state<{ from: number; to: number; dy: number; startY: number } | null>(null);

  function startDrag(event: PointerEvent, index: number) {
    if (event.button !== 0 || !stack) return;
    const item = draft.items[index];
    if (item.kind === "branch" && item.name === stack.top) return;
    event.preventDefault();
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    drag = { from: index, to: index, dy: 0, startY: event.clientY };
    if (item.kind === "commit") selected = item.id;
  }

  /** Where the dragged item lands: its index in the list without it. */
  function moveDrag(event: PointerEvent) {
    if (!drag) return;
    const y = event.clientY;
    let to = 0;
    rows.forEach((row, i) => {
      if (!row || i === drag!.from) return;
      const box = row.getBoundingClientRect();
      if (box.top + box.height / 2 < y) to++;
    });
    drag = { ...drag, to: Math.max(1, to), dy: y - drag.startY };
  }

  /** The row the drop line shows above, in the list as it is; the list's length for the end. */
  const dropAt = $derived(drag && drag.to !== drag.from ? (drag.to < drag.from ? drag.to : drag.to + 1) : null);

  function endDrag() {
    if (!drag || !stack) return;
    const { from, to } = drag;
    drag = null;
    if (to === from) return;
    const next = dragItem(draft.items, from, to, stack.top);
    if (next) {
      done = null;
      draft = { ...draft, items: next };
    }
  }

  // --- Keys --------------------------------------------------------------------------------------
  function onKey(event: KeyboardEvent) {
    if (!stack || confirm.request || menu || event.defaultPrevented) return;
    const target = event.target as HTMLElement | null;
    const typing = !!target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA");
    if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
      event.preventDefault();
      start();
      return;
    }
    if (typing) {
      if (event.key === "Escape" || event.key === "Enter") (target as HTMLElement).blur();
      return;
    }
    const index = draft.items.findIndex((i) => i.kind === "commit" && i.id === selected);
    if (event.altKey && (event.key === "ArrowUp" || event.key === "ArrowDown")) {
      event.preventDefault();
      if (index >= 0) move(index, event.key === "ArrowUp" ? -1 : 1);
      return;
    }
    if (event.key === "ArrowUp" || event.key === "ArrowDown") {
      event.preventDefault();
      const commits = draft.items.flatMap((i) => (i.kind === "commit" ? [i.id] : []));
      const at = commits.indexOf(selected ?? "");
      const next = commits[Math.max(0, Math.min(commits.length - 1, at + (event.key === "ArrowUp" ? -1 : 1)))];
      if (next) selected = next;
      return;
    }
    if (event.metaKey || event.ctrlKey || event.altKey || !selected) return;
    const action = ACTIONS.find((a) => a.key.toLowerCase() === event.key.toLowerCase());
    if (action) {
      event.preventDefault();
      setAct(selected, action.id);
    }
  }

  // --- Running it ----------------------------------------------------------------------------------
  async function start() {
    if (!stack || !preview || !changed || checking) return;
    const request = editStackRequest(ctx, stack, draft, preview);
    const before = preview;
    const ok = await run(request);
    if (!ok) return;
    const moved = before.branches.filter((b) => b.moves).length;
    const rewritten = before.rewritten + before.squashed;
    done = {
      line: `${rewritten} ${rewritten === 1 ? "commit" : "commits"} rewritten, ${moved} ${moved === 1 ? "branch" : "branches"} moved. Undo is in the Operation Log.`,
      push: before.branches.filter((b) => b.forcePush).map((b) => b.name),
      moved: before.branches.filter((b) => b.moves).map((b) => b.name),
    };
    await load();
  }

  async function undo() {
    await oplog.load();
    const latest = oplog.entries[0];
    if (!latest) return;
    if (await run(undoRequest(latest))) {
      done = null;
      await load();
    }
  }

  async function push() {
    if (!stack || !done) return;
    const request = pushBranchesRequest(ctx, stack, done.push);
    if (request && (await run(request))) done = { ...done, push: [] };
  }

  // --- The picture of the result ------------------------------------------------------------
  let stripWidth = $state(0);
  const STRIP_HEIGHT = 46;
  const strip = $derived.by(() => {
    if (!stack) return [];
    const branches = stack.branches;
    const shapes = softArrows(branches.length, stripWidth, STRIP_HEIGHT);
    return branches.map((b, i) => {
      const after = preview?.branches.find((a) => a.name === b.name);
      const count = after?.commits ?? draft.items.filter((it, k) => it.kind === "commit" && owners[k] === b.name).length;
      // Once it ran, the picture keeps showing what moved.
      const moves = done ? done.moved.includes(b.name) : !!after?.moves;
      return { name: b.name, count, moves, ...shapes[i] };
    });
  });

  const icons = {
    commits: "M8 5.5a2.5 2.5 0 1 1 0 5 2.5 2.5 0 0 1 0-5zM8 1.5v4M8 10.5v4",
    branch: "M4.5 2v12M11.5 4v1.5c0 3-7 2-7 5M9.5 4h4",
    push: "M8 12V3M4.5 6.5 8 3l3.5 3.5M3 14h10",
    ok: "M3.5 8.5 6.5 11.5 12.5 4.5",
    warn: "M8 2.5 14 13H2zM8 6.5v3M8 11.5v.1",
    stop: "M5 3v10M11 3v10",
  };
  type Fact = { icon: string; title: string; detail: string; tone?: "ok" | "warn" };

  const list = (xs: string[]) => (xs.length < 2 ? xs.join("") : `${xs.slice(0, -1).join(", ")} and ${xs[xs.length - 1]}`);
  const plural = (n: number, word: string) => `${n} ${n === 1 ? word : word === "branch" ? "branches" : `${word}s`}`;

  const facts = $derived.by((): Fact[] => {
    if (!stack) return [];
    if (previewError) return [{ icon: icons.warn, title: "The dry run failed", detail: previewError, tone: "warn" }];
    if (!preview) return [];
    if (!preview.changed) {
      return [
        {
          icon: icons.commits,
          title: "Nothing to do yet",
          detail: stack.newer
            ? `Change an action, move a commit, or switch on “Move onto latest ${stack.trunk}”.`
            : "Change an action or move a commit.",
        },
      ];
    }
    const out: Fact[] = [];
    const extra = [
      preview.squashed && `${preview.squashed} squashed`,
      preview.dropped && `${preview.dropped} dropped`,
      preview.reworded && `${preview.reworded} reworded`,
      preview.moved && `${preview.moved} moved`,
    ].filter(Boolean);
    const rewritten = preview.rewritten + preview.squashed;
    out.push({
      icon: icons.commits,
      title: `${plural(rewritten, "commit")} rewritten${extra.length ? ` · ${extra.join(", ")}` : ""}`,
      detail: draft.onto ? `Replayed on top of ${stack.trunk} ${shortId(stack.trunkTip.id)}, oldest first.` : "Commits below the first change keep their SHAs.",
    });
    const empty = preview.branches.filter((b) => b.commits === 0).map((b) => b.name);
    if (empty.length) {
      out.push({
        icon: icons.warn,
        title: `${list(empty)} ${empty.length === 1 ? "ends up empty" : "end up empty"}`,
        detail: `${empty.length === 1 ? "It points" : "They point"} at the same commit as the branch below. A pull request would show no changes.`,
        tone: "warn",
      });
    }
    const moving = preview.branches.filter((b) => b.moves).length;
    if (moving) {
      out.push({
        icon: icons.branch,
        title: moving === 1 ? "1 branch moves with its commits" : `${moving} branches move with their commits`,
        detail: "Each branch line stays on its own last commit, so the stack keeps its shape. Moving a commit across a line moves it to that branch.",
      });
    }
    const force = preview.branches.filter((b) => b.forcePush).map((b) => b.name);
    if (force.length) {
      out.push({
        icon: icons.push,
        title: `Force push needed for ${list(force)}`,
        detail: "Push them from here when the rebase is done: with --force-with-lease, so nobody else’s new commits get overwritten.",
      });
    }
    if (preview.emptied.length) {
      out.push({
        icon: icons.commits,
        title: `${plural(preview.emptied.length, "commit")} left out`,
        detail: `${stack.trunk} already has ${preview.emptied.length === 1 ? "its changes" : "their changes"}: ${preview.emptied.map((id) => `“${byId.get(id)?.summary ?? shortId(id)}”`).join(", ")}.`,
      });
    }
    if (stack.leftBehind.length) {
      out.push({
        icon: icons.warn,
        title: `${list(stack.leftBehind)} ${stack.leftBehind.length === 1 ? "stays" : "stay"} where ${stack.leftBehind.length === 1 ? "it is" : "they are"}`,
        detail: `${stack.leftBehind.length === 1 ? "It forks" : "They fork"} off the stack, so the rebase doesn’t move ${stack.leftBehind.length === 1 ? "it" : "them"}. Edit ${stack.leftBehind.length === 1 ? "its" : "each one’s"} stack on its own.`,
        tone: "warn",
      });
    }
    if (preview.edits) {
      out.push({
        icon: icons.stop,
        title: `Stops at ${plural(preview.edits, "commit")} for you to change`,
        detail: "Amend it, then Continue Rebase in the banner goes on with the rest.",
      });
    }
    if (preview.conflict) {
      const c = preview.conflict;
      out.push({
        icon: icons.warn,
        title: c.files.length === 1 ? "1 likely conflict" : `Conflicts in ${c.files.length} files`,
        detail: `${list(c.files.slice(0, 3))}${c.files.length > 3 ? " and more" : ""}, while applying “${c.summary}”. The rebase will stop there and open Resolve Conflicts.`,
        tone: "warn",
      });
    } else {
      out.push({
        icon: icons.ok,
        title: "No conflicts expected",
        detail: `Checked with a dry run against ${draft.onto ? `the latest ${stack.trunk}` : "the current base"} before anything changes.`,
        tone: "ok",
      });
    }
    if (stack.dirty) {
      out.push({ icon: icons.commits, title: "Your uncommitted changes come along", detail: "--autostash puts them aside for the rebase and back afterwards." });
    }
    return out;
  });

  /** The commit whose combined message is being written: the selected squash's target, or the selected target. */
  const squashTarget = $derived.by(() => {
    if (!selected) return null;
    const target = actOf(selected) === "squash" ? foldTarget(draft, selected) : selected;
    return target && foldsInto(draft, target).some((id) => actOf(id) === "squash") ? target : null;
  });

  function noteOf(id: string): string {
    const a = actOf(id);
    if (a === "squash" || a === "fixup") {
      const target = foldTarget(draft, id);
      if (!target) return "Nothing below to fold into, kept as is";
      return `${a === "squash" ? "Squash into" : "Fix up into"} “${byId.get(target)?.summary ?? shortId(target)}”`;
    }
    if (a === "edit") return "The rebase stops here so you can amend";
    if (a === "drop") return "Removed from the branch";
    if (preview?.emptied.includes(id)) return `${stack?.trunk} already has it: left out`;
    if (preview?.conflict?.id === id) return `Likely conflict in ${preview.conflict.files[0]}${preview.conflict.files.length > 1 ? ` and ${preview.conflict.files.length - 1} more` : ""}`;
    return "";
  }

  function branchSub(name: string, index: number): string {
    const b = stack?.branches.find((x) => x.name === name);
    if (!b) return "";
    let n = 0;
    for (let k = index + 1; k < draft.items.length && draft.items[k].kind === "commit"; k++) {
      const id = (draft.items[k] as { id: string }).id;
      if (actOf(id) !== "drop" && !isFold(id)) n++;
    }
    const where = b.upstream ? (b.ahead ? `${b.ahead} to push` : `on ${b.upstream.remote}`) : "not pushed";
    const head = name === stack?.head ? "checked out · " : "";
    return `${head}${where} · ${plural(n, "commit")}`;
  }

  const ontoLine = $derived(
    stack && draft.onto ? `${stack.trunk} ${shortId(stack.trunkTip.id)}` : stack ? `the current base ${shortId(stack.base.id)}` : "",
  );
</script>

<svelte:window onkeydown={onKey} />

{#if error}
  <div class="empty-state">
    <p>{error}</p>
    <button class="plain" onclick={onClose}>Back to History</button>
  </div>
{:else if stack}
  {@const trunkColor = colorOf(stack.trunk)}
  <div class="editor">
    <div class="strip">
      <svg class="icon" viewBox="0 0 16 16" style:color={lane(colorOf(stack.top))}><rect x="3" y="6.5" width="10" height="7" rx="1.5" /><path d="M4.5 4.5h7M6 2.5h4" /></svg>
      <span class="top-name">{stack.top}</span>
      <span class="dim">onto</span>
      <span class="cap" style:background={tint(trunkColor, "label")} style:color={plate(trunkColor)}>{stack.trunk}</span>
      <span class="grow"></span>
      <button class="switch-row" onclick={toggleOnto} disabled={!stack.newer} role="switch" aria-checked={draft.onto}>
        <span class="switch-words">
          <span class="switch-title">Move onto latest {stack.trunk}</span>
          <span class="switch-sub">{stack.newer ? `${plural(stack.newer, "new commit")} since the stack left it` : `Already on the newest ${stack.trunk}`}</span>
        </span>
        <span class="switch" class:on={draft.onto}><span class="knob"></span></span>
      </button>
    </div>

    <div class="columns">
      <section class="plan" aria-label="Commits">
        <div class="plan-head">
          <span class="grow">Newest at the top · drag commits, or a branch line, to move them</span>
          <span class="count">{kept} of {commitCount} commits kept</span>
        </div>
        <div class="list" role="list" aria-label="Rebase plan" onpointermove={moveDrag} onpointerup={endDrag} onpointercancel={endDrag}>
          {#each draft.items as item, index (itemKey(item))}
            {@const color = colorOf(owners[index])}
            {@const first = index === 0}
            <div
              class="item"
              class:branch-item={item.kind === "branch"}
              class:dragging={drag?.from === index}
              class:drop-above={dropAt === index}
              class:drop-below={dropAt === draft.items.length && index === draft.items.length - 1}
              style:transform={drag?.from === index ? `translateY(${drag.dy}px)` : undefined}
              role="listitem"
              bind:this={rows[index]}
            >
              <span class="line" style:top={first ? "50%" : "0"} style:background={lane(color)}></span>
              {#if item.kind === "branch"}
                {@const movable = item.name !== stack.top}
                <span class="branch-dot" style:border-color={lane(colorOf(item.name))}></span>
                <button class="grip" class:hidden={!movable} aria-label="Drag {item.name}" title={movable ? "Drag to move the branch line" : undefined} onpointerdown={(e) => startDrag(e, index)}>
                  <svg class="icon small" viewBox="0 0 16 16"><circle cx="6" cy="4" r=".6" /><circle cx="10" cy="4" r=".6" /><circle cx="6" cy="8" r=".6" /><circle cx="10" cy="8" r=".6" /><circle cx="6" cy="12" r=".6" /><circle cx="10" cy="12" r=".6" /></svg>
                </button>
                <span class="branch-label">
                  <span class="cap" style:background={tint(colorOf(item.name), "label")} style:color={plate(colorOf(item.name))}>{item.name}</span>
                  <span class="dim small-text">{branchSub(item.name, index)}</span>
                </span>
              {:else}
                {@const c = byId.get(item.id)!}
                {@const a = actOf(item.id)}
                {@const folded = isFold(item.id)}
                {@const on = selected === item.id}
                {@const note = noteOf(item.id)}
                <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
                <div class="row" class:on style:background={on ? tint(color) : undefined} class:tall={!!note} onclick={() => (selected = item.id)}>
                  <button class="grip" aria-label="Drag {c.summary}" title="Drag to move, or ⌥↑ ⌥↓" onpointerdown={(e) => startDrag(e, index)}>
                    <svg class="icon small" viewBox="0 0 16 16"><circle cx="6" cy="4" r=".6" /><circle cx="10" cy="4" r=".6" /><circle cx="6" cy="8" r=".6" /><circle cx="10" cy="8" r=".6" /><circle cx="6" cy="12" r=".6" /><circle cx="10" cy="12" r=".6" /></svg>
                  </button>
                  <span
                    class="dot"
                    class:hollow={a === "drop" || folded}
                    style:border-color={a === "drop" ? "var(--text2)" : lane(color)}
                    style:background={a === "drop" || folded ? "var(--win)" : lane(color)}
                  ></span>
                  <span class="cells" class:indent={folded}>
                    <button class="pill act-{a}" aria-haspopup="menu" title="Action ({ACTIONS.find((x) => x.id === a)?.key})" onclick={(e) => { e.stopPropagation(); openActions(e, item.id); }}>
                      {ACTIONS.find((x) => x.id === a)?.label}
                      <svg viewBox="0 0 16 16"><path d="M4 6l4 4 4-4" /></svg>
                    </button>
                    {#if a === "reword" && on}
                      <!-- svelte-ignore a11y_autofocus -->
                      <input class="reword" aria-label="New commit message" value={subjectOf(item.id)} oninput={(e) => reword(item.id, e.currentTarget.value)} onclick={(e) => e.stopPropagation()} autofocus />
                    {:else}
                      <span class="words">
                        <span class="msg" class:struck={a === "drop"}>{a === "reword" ? subjectOf(item.id) : c.summary}</span>
                        {#if note}<span class="note" class:warn={preview?.conflict?.id === item.id}>{note}</span>{/if}
                      </span>
                    {/if}
                    <span class="sha mono" title={c.pushed ? "Already pushed" : "Not pushed yet"}>{shortId(c.id)}</span>
                    <span class="stat">{c.additions ? `+${c.additions}` : ""}{c.deletions ? ` −${c.deletions}` : ""}</span>
                    {#if on}
                      <span class="moves">
                        <button aria-label="Move up" title={withKeys("Move Up", "Alt+Up")} disabled={index <= 1} onclick={(e) => { e.stopPropagation(); move(index, -1); }}>
                          <svg class="icon" viewBox="0 0 16 16"><path d="M8 12.5v-9M4.5 7 8 3.5 11.5 7" /></svg>
                        </button>
                        <button aria-label="Move down" title={withKeys("Move Down", "Alt+Down")} disabled={index >= draft.items.length - 1} onclick={(e) => { e.stopPropagation(); move(index, 1); }}>
                          <svg class="icon" viewBox="0 0 16 16"><path d="M8 3.5v9M4.5 9 8 12.5 11.5 9" /></svg>
                        </button>
                      </span>
                    {/if}
                  </span>
                </div>
              {/if}
            </div>
          {/each}
          <div class="item base-item" role="listitem">
            <span class="line" style:top="0" style:bottom="50%" style:background={lane(colorOf(owners[owners.length - 1] ?? stack.top))}></span>
            <span class="base-dot" style:background={lane(trunkColor)}></span>
            <span class="base-words">
              <span class="base-line">
                <span class="cap" style:background={tint(trunkColor, "label")} style:color={plate(trunkColor)}>{stack.trunk}</span>
                <span class="msg">{draft.onto ? stack.trunkTip.summary : stack.base.summary}</span>
              </span>
              <span class="dim small-text">
                {draft.onto
                  ? `${shortId(stack.trunkTip.id)} · latest ${stack.trunk}, ${plural(stack.newer, "commit")} newer than the old base`
                  : `${shortId(stack.base.id)} · current base, the stack stays where it is`}
              </span>
            </span>
          </div>
        </div>
        <span class="grow"></span>
        <div class="legend">
          {#each ACTIONS as a (a.id)}<span><span class="key mono">{a.key}</span>{a.label}</span>{/each}
          <span><span class="key mono">{keys("Alt+Up")}{keys("Down").slice(-1)}</span>Move</span>
        </div>
      </section>

      <section class="after" aria-label="Result preview">
        {#if done}
          <div class="done">
            <span class="done-icon"><svg viewBox="0 0 16 16"><path d="M3.5 8.5 6.5 11.5 12.5 4.5" /></svg></span>
            <span class="done-words"><span class="done-title">Stack updated</span><span class="dim small-text">{done.line}</span></span>
          </div>
        {:else}
          <div class="after-head">
            <span class="after-title">After</span>
            <span class="dim">{kept} {kept === 1 ? "commit" : "commits"} in {plural(stack.branches.length, "branch")}, onto {ontoLine}{checking ? " · checking…" : ""}</span>
          </div>
        {/if}
        <div class="arrows" bind:clientWidth={stripWidth}>
          {#if stripWidth > 0}
            {#each strip as s (s.name)}
              {@const color = colorOf(s.name)}
              <svg class="shape" aria-hidden="true" width={stripWidth} height={STRIP_HEIGHT} viewBox="0 0 {stripWidth} {STRIP_HEIGHT}">
                <path d={s.path} style:fill={s.moves ? tint(color, "bar") : "var(--field)"} style:stroke={s.moves ? `color-mix(in srgb, ${lane(color)} 40%, transparent)` : "transparent"} />
              </svg>
              <div class="arrow-words" style:left="{s.left}px" style:width="{s.width}px" title="{s.name}: {plural(s.count, "commit")}, {s.moves ? "rewritten" : "unchanged"}">
                <span class="arrow-name" style:color={s.moves ? plate(color) : undefined}>{s.name}</span>
                <span class="dim small-text">{plural(s.count, "commit")}</span>
              </div>
            {/each}
          {/if}
        </div>
        {#if !done}
          <div class="facts">
            {#each facts as f (f.title)}
              <div class="fact" class:warn={f.tone === "warn"}>
                <span class="fact-icon" class:ok={f.tone === "ok"} class:warn={f.tone === "warn"}><svg class="icon" viewBox="0 0 16 16"><path d={f.icon} /></svg></span>
                <span class="fact-words"><span class="fact-title">{f.title}</span><span class="fact-detail">{f.detail}</span></span>
              </div>
            {/each}
          </div>
          {#if squashTarget}
            <label class="combined">
              <span class="combined-title">Message for the combined commit</span>
              <textarea
                rows="4"
                value={draft.messages[squashKey(squashTarget)] ?? combinedMessage(stack, draft, squashTarget)}
                oninput={(e) => (draft = { ...draft, messages: { ...draft.messages, [squashKey(squashTarget!)]: e.currentTarget.value } })}
              ></textarea>
            </label>
          {/if}
        {/if}
        <span class="grow"></span>
        {#if done}
          <div class="buttons">
            <button class="plain" onclick={undo}>Undo</button>
            <span class="grow"></span>
            <button class="plain" onclick={onClose}>Back to History</button>
            {#if done.push.length}
              <button class="primary" onclick={push}>Push {done.push.length} {done.push.length === 1 ? "Branch" : "Branches"}</button>
            {/if}
          </div>
        {:else}
          <div class="buttons">
            <button class="plain" onclick={resetPlan} disabled={!edited()}>Reset Plan</button>
            <span class="grow"></span>
            <button class="plain" onclick={onClose}>Cancel</button>
            <button class="primary" onclick={start} disabled={!changed || checking} title={changed ? undefined : "Change the plan first"}>
              Rebase Stack <span class="key-hint">{keys("Mod+Enter")}</span>
            </button>
          </div>
        {/if}
      </section>
    </div>
  </div>
{/if}

{#if menu}
  <Menu x={menu.x} y={menu.y} label="Commit action" entries={menu.entries} onClose={() => (menu = null)} />
{/if}

<style>
  .editor {
    flex-grow: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    padding: 0 12px 12px 4px;
  }
  .empty-state {
    flex-grow: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: var(--text2);
    padding: 40px;
    text-align: center;
  }
  .grow {
    flex-grow: 1;
  }
  .dim {
    color: var(--text2);
  }
  .small-text {
    font-size: 11px;
  }
  .strip {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 50px;
    flex-shrink: 0;
    padding: 0 6px 0 14px;
    border-radius: 16px;
    background: var(--field);
  }
  .top-name {
    font-weight: 600;
  }
  .cap {
    display: inline-flex;
    align-items: center;
    height: 20px;
    padding: 0 9px;
    border-radius: 10px;
    font-size: 11px;
    font-weight: 600;
    white-space: nowrap;
  }
  .switch-row {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 4px 8px;
    border-radius: 12px;
  }
  .switch-row:disabled {
    opacity: 0.55;
  }
  .switch-words {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    line-height: 1.25;
  }
  .switch-title {
    font-weight: 500;
  }
  .switch-sub {
    font-size: 11px;
    color: var(--text2);
  }
  .switch {
    position: relative;
    width: 30px;
    height: 18px;
    border-radius: 9px;
    background: var(--switch-off);
    flex-shrink: 0;
    transition: background 0.15s;
  }
  .switch.on {
    background: var(--accent);
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    border-radius: 7px;
    background: #fff;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.25);
    transition: left 0.15s;
  }
  .switch.on .knob {
    left: 14px;
  }
  .columns {
    flex-grow: 1;
    min-height: 0;
    display: flex;
    margin-top: 10px;
    border-top: 1px solid var(--sep);
  }
  .plan {
    flex: 1 1 58%;
    min-width: 420px;
    display: flex;
    flex-direction: column;
    padding: 8px 10px 0 0;
    border-right: 1px solid var(--sep);
    min-height: 0;
  }
  .plan-head {
    display: flex;
    align-items: center;
    height: 30px;
    padding: 0 10px 0 16px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
    flex-shrink: 0;
  }
  .count {
    font-weight: 400;
  }
  .list {
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    min-height: 0;
    padding-bottom: 6px;
  }
  .item {
    position: relative;
    display: flex;
    align-items: center;
    min-height: 40px;
    flex-shrink: 0;
  }
  .item.branch-item {
    min-height: 34px;
  }
  .item.dragging {
    z-index: 5;
    border-radius: 10px;
    background: var(--win);
    box-shadow: 0 6px 18px rgba(0, 0, 0, 0.14);
  }
  .item.dragging .line {
    visibility: hidden;
  }
  .item.drop-above::before,
  .item.drop-below::after {
    content: "";
    position: absolute;
    left: 44px;
    right: 8px;
    height: 2px;
    border-radius: 1px;
    background: var(--accent);
    z-index: 4;
  }
  .item.drop-above::before {
    top: -1px;
  }
  .item.drop-below::after {
    bottom: -1px;
  }
  .line {
    position: absolute;
    left: 31px;
    width: 2px;
    bottom: 0;
    border-radius: 1px;
  }
  .branch-dot {
    position: absolute;
    left: 26px;
    top: 50%;
    margin-top: -6px;
    width: 12px;
    height: 12px;
    border-radius: 6px;
    box-sizing: border-box;
    background: var(--win);
    border: 2.5px solid;
  }
  .grip {
    position: relative;
    width: 18px;
    height: 24px;
    margin-left: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text2);
    opacity: 0.6;
    cursor: grab;
    touch-action: none;
  }
  .grip.hidden {
    visibility: hidden;
  }
  .branch-item .grip {
    position: absolute;
    left: 4px;
  }
  .icon.small {
    width: 14px;
    height: 14px;
  }
  .branch-label {
    margin-left: 52px;
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .row {
    position: relative;
    flex-grow: 1;
    display: flex;
    align-items: center;
    min-height: 36px;
    margin: 2px 0;
    padding-right: 10px;
    border-radius: 10px;
  }
  .row.tall {
    min-height: 44px;
  }
  .dot {
    position: absolute;
    left: 27px;
    top: 50%;
    margin-top: -5px;
    width: 10px;
    height: 10px;
    border-radius: 5px;
    box-sizing: border-box;
    border: 2px solid;
  }
  .cells {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-grow: 1;
    min-width: 0;
    margin-left: 26px;
  }
  .cells.indent {
    margin-left: 40px;
  }
  .pill {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 4px;
    width: 74px;
    flex-shrink: 0;
    height: 22px;
    padding: 0 7px 0 9px;
    border-radius: 11px;
    font-size: 11px;
    font-weight: 600;
    background: var(--field);
    color: var(--text);
  }
  .pill svg {
    width: 9px;
    height: 9px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .pill.act-reword {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    color: var(--accent-text);
  }
  .pill.act-edit {
    background: var(--orange-soft);
    color: var(--orange);
  }
  .pill.act-squash,
  .pill.act-fixup {
    color: var(--text2);
  }
  .pill.act-drop {
    background: var(--red-soft);
    color: var(--red);
  }
  .words {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    line-height: 1.25;
  }
  .msg {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .msg.struck {
    text-decoration: line-through;
    color: var(--text2);
  }
  .note {
    font-size: 11px;
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .note.warn {
    color: var(--orange);
  }
  .reword {
    flex-grow: 1;
    min-width: 0;
    height: 26px;
    box-sizing: border-box;
    padding: 0 8px;
    border-radius: 7px;
    border: 1px solid var(--accent);
    background: var(--win);
    outline: 0;
    font: inherit;
    color: var(--text);
  }
  .sha {
    font-size: 11px;
    color: var(--text2);
    flex-shrink: 0;
  }
  .stat {
    font-size: 11px;
    color: var(--text2);
    width: 58px;
    text-align: right;
    flex-shrink: 0;
    white-space: nowrap;
  }
  .moves {
    display: flex;
    gap: 2px;
    flex-shrink: 0;
  }
  .moves button {
    width: 22px;
    height: 22px;
    border-radius: 11px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--win);
    border: 0.5px solid var(--glass-border);
    color: var(--text);
  }
  .moves button:disabled {
    color: var(--text2);
    opacity: 0.6;
  }
  .moves .icon {
    width: 12px;
    height: 12px;
  }
  .base-item {
    min-height: 52px;
  }
  .base-dot {
    position: absolute;
    left: 25px;
    top: 50%;
    margin-top: -7px;
    width: 14px;
    height: 14px;
    border-radius: 7px;
    box-sizing: border-box;
    border: 3px solid var(--win);
    box-shadow: 0 0 0 2px var(--lane-0);
  }
  .base-words {
    margin-left: 52px;
    display: flex;
    flex-direction: column;
    line-height: 1.3;
    min-width: 0;
  }
  .base-line {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 14px;
    margin: 8px 0 0 16px;
    padding: 10px 12px;
    border-radius: 12px;
    background: var(--field);
    font-size: 11px;
    color: var(--text2);
    flex-shrink: 0;
  }
  .legend > span {
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .key {
    min-width: 16px;
    height: 16px;
    padding: 0 3px;
    box-sizing: border-box;
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--win);
    border: 0.5px solid var(--glass-border);
    color: var(--text);
  }
  .after {
    flex: 1 1 42%;
    min-width: 340px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 14px 4px 0 18px;
    min-height: 0;
    overflow-y: auto;
  }
  .after-head {
    display: flex;
    align-items: baseline;
    gap: 8px;
    flex-wrap: wrap;
  }
  .after-title {
    font-size: 15px;
    font-weight: 700;
  }
  .done {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 14px;
    border-radius: 14px;
    background: var(--green-soft);
  }
  .done-icon {
    width: 28px;
    height: 28px;
    border-radius: 9px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--green);
    color: #fff;
    flex-shrink: 0;
  }
  .done-icon svg {
    width: 14px;
    height: 14px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2.2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .done-words {
    display: flex;
    flex-direction: column;
    line-height: 1.3;
  }
  .done-title {
    font-weight: 700;
  }
  .arrows {
    position: relative;
    height: 46px;
    flex-shrink: 0;
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
  .arrow-words {
    position: absolute;
    top: 0;
    height: 46px;
    padding-left: 12px;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 1px;
    line-height: 1.2;
    min-width: 0;
  }
  .arrow-words > span {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .arrow-name {
    font-size: 12px;
    font-weight: 600;
  }
  .facts {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .fact {
    display: flex;
    gap: 10px;
    padding: 9px 10px;
    border-radius: 12px;
  }
  .fact.warn {
    background: var(--orange-soft);
  }
  .fact-icon {
    width: 22px;
    height: 22px;
    border-radius: 7px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--field);
    color: var(--text2);
    flex-shrink: 0;
  }
  .fact-icon .icon {
    width: 14px;
    height: 14px;
  }
  .fact-icon.ok {
    background: var(--green-soft);
    color: var(--green);
  }
  .fact-icon.warn {
    background: transparent;
    color: var(--orange);
  }
  .fact-words {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    line-height: 1.35;
  }
  .fact-title {
    font-weight: 600;
  }
  .fact.warn .fact-title {
    color: var(--orange);
  }
  .fact-detail {
    font-size: 12px;
    color: var(--text2);
    overflow-wrap: anywhere;
  }
  .combined {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .combined-title {
    font-size: 11px;
    font-weight: 600;
    color: var(--text2);
  }
  .combined textarea {
    resize: vertical;
    border-radius: 10px;
    padding: 8px 10px;
    background: var(--win);
    border: 0.5px solid var(--glass-border);
    outline: 0;
    font: inherit;
    line-height: 1.45;
    color: var(--text);
  }
  .buttons {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-bottom: 2px;
    flex-shrink: 0;
  }
  .buttons button {
    height: 30px;
    padding: 0 16px;
    border-radius: 15px;
    font-weight: 500;
    white-space: nowrap;
    flex-shrink: 0;
  }
  .buttons .plain {
    background: var(--field);
  }
  .buttons .primary {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--accent);
    color: #fff;
    font-weight: 600;
  }
  .buttons button:disabled {
    opacity: 0.45;
  }
  .key-hint {
    font-size: 11px;
    font-weight: 500;
    opacity: 0.85;
  }
</style>
