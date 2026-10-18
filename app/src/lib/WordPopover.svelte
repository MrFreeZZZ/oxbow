<script lang="ts">
  // The popover of a clicked word in a terminal block (lens.svelte.ts), over everything else,
  // sheets included. Escape or a click elsewhere closes it, and only it.

  import { plate, relativeTime, tint } from "./format";
  import { lens } from "./lens.svelte";

  const WIDTH = 300;
  const GAP = 6;

  let box = $state<HTMLDivElement>();
  let height = $state(0);

  const open = $derived(lens.open);
  const place = $derived.by(() => {
    if (!open) return { left: 0, top: 0 };
    const { at } = open;
    const left = Math.max(12, Math.min(at.left - 10, innerWidth - WIDTH - 12));
    const below = at.bottom + GAP;
    // Above the word when the window has no room under it.
    const top = below + height > innerHeight - 12 && at.top - GAP - height > 12 ? at.top - GAP - height : below;
    return { left, top };
  });

  const KIND = { local: "Branch", remote: "Server branch", tag: "Tag" } as const;

  // Escape closes the popover before the sheet under it hears it.
  function onKey(event: KeyboardEvent) {
    if (lens.open && event.key === "Escape") {
      event.preventDefault();
      event.stopImmediatePropagation();
      lens.close();
    }
  }

  function onPointer(event: PointerEvent) {
    if (!lens.open) return;
    const target = event.target as Node;
    if (box?.contains(target) || lens.open.from.contains(target)) return;
    lens.close();
  }

  $effect(() => {
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("pointerdown", onPointer, true);
    const close = () => lens.close();
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("resize", close);
    };
  });

  // The word scrolled away with its block: the popover would point at nothing.
  $effect(() => {
    if (!open) return;
    // Only a scroll that moves the word: History scrolling to the lit-up commit doesn't.
    const close = (event: Event) => event.target instanceof Node && lens.open && event.target.contains(lens.open.from) && lens.close();
    document.addEventListener("scroll", close, true);
    return () => document.removeEventListener("scroll", close, true);
  });
</script>

{#if open}
  {@const look = open.look}
  <div class="pop" role="dialog" aria-label="What it means" bind:this={box} bind:clientHeight={height} style:left="{place.left}px" style:top="{place.top}px">
    {#if look.kind === "word"}
      <span class="head"><span class="mono token" class:sub={look.word.kind === "sub"}>{look.word.text.replace(/=.*$/, "")}</span></span>
      <span class="title">{look.meaning.title}</span>
      <span class="text">{look.meaning.text}</span>
      {#if look.manual}<span class="manual">More in <span class="mono">{look.manual}</span></span>{/if}
    {:else}
      {@const row = look.row}
      <span class="head">
        {#if look.kind === "ref"}
          {#if look.ref === "tag"}
            <span class="tag">{look.name}</span>
          {:else}
            <span class="capsule" style:background={tint(look.color, "label")} style:color={plate(look.color)}>{look.name}</span>
          {/if}
          <span class="kind">{KIND[look.ref]}</span>
        {:else}
          <span class="mono sha">{look.row.id.slice(0, 7)}</span>
          <span class="kind">Commit</span>
        {/if}
      </span>
      {#if row}
        <span class="summary">{row.summary}</span>
        <span class="meta">
          {#if look.kind === "ref"}<span class="mono">{row.id.slice(0, 7)}</span>{" · "}{/if}{row.authorName} · {relativeTime(row.time)}
        </span>
        {#if look.kind === "commit" && row.labels.some((l) => l.kind !== "stash" && l.kind !== "head")}
          <span class="labels">
            {#each row.labels.filter((l) => l.kind !== "stash" && l.kind !== "head") as label (label.kind + label.name)}
              {#if label.kind === "tag"}<span class="tag">{label.name}</span>{:else}<span class="capsule" style:background={tint(label.color, "label")} style:color={plate(label.color)}>{label.name}</span>{/if}
            {/each}
          </span>
        {/if}
        <span class="lit">
          <svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="3" /><circle cx="8" cy="8" r="6" /></svg>
          Lit up in History
        </span>
      {:else}
        <span class="meta">Its commit isn’t loaded in History.</span>
      {/if}
    {/if}
  </div>
{/if}

<style>
  .pop {
    position: fixed;
    z-index: 60;
    width: 300px;
    max-width: calc(100vw - 24px);
    padding: 12px 14px;
    border-radius: var(--radius-xl);
    background: var(--sheet);
    -webkit-backdrop-filter: blur(30px) saturate(1.8);
    backdrop-filter: blur(30px) saturate(1.8);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--shadow-pop);
    display: flex;
    flex-direction: column;
    gap: 5px;
    text-align: left;
    color: var(--text);
    font-family: var(--font);
    animation: show 0.12s ease-out;
  }
  @keyframes show {
    from {
      opacity: 0;
      transform: translateY(-3px);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .pop {
      animation: none;
    }
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .token {
    font-size: var(--text-small);
    color: var(--term-flag);
    font-weight: 600;
  }
  .token.sub {
    color: var(--term-sub);
  }
  .title {
    font-size: var(--text-body);
    font-weight: 600;
  }
  .text,
  .meta {
    font-size: var(--text-small);
    line-height: 1.45;
    color: var(--text2);
  }
  .manual {
    margin-top: 3px;
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .manual .mono,
  .meta .mono {
    font-size: var(--text-caption);
  }
  .kind {
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .capsule,
  .tag {
    padding: 1px 7px;
    border-radius: var(--radius-m);
    font-size: var(--text-caption);
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .tag {
    background: var(--tag-bg);
    color: var(--tag-fg);
  }
  .sha {
    font-size: var(--text-small);
    font-weight: 600;
    color: var(--term-sha);
  }
  .summary {
    font-size: var(--text-body);
    font-weight: 500;
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }
  .labels {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .lit {
    display: flex;
    align-items: center;
    gap: 5px;
    margin-top: 2px;
    font-size: var(--text-caption);
    color: var(--text2);
  }
  .lit svg {
    width: 12px;
    height: 12px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.3;
  }
</style>
