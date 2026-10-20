<script lang="ts" module>
  // Rows of a long diff or blame are drawn only near the visible part of the list they scroll
  // in: farther away a block keeps its place with an empty box of its height. Blocks of 64 lines
  // keep the boxes few and the drawing cheap.

  /** Lines a block holds. */
  export const BLOCK = 64;

  /** How far beyond the visible part blocks are drawn, so a scroll doesn't show them empty. */
  const AHEAD = 1200;

  /** Each block's call for whether it is near the visible part. */
  const watchers = new WeakMap<Element, (near: boolean) => void>();
  const observers = new WeakMap<Element, IntersectionObserver>();
  let windowObserver: IntersectionObserver | null = null;
  /** Height of a line in each scrolling list, for the boxes of blocks not drawn yet. */
  const lineHeights = new WeakMap<Element, number>();

  /** The element `el` scrolls in, or null for the window. */
  function scrollerOf(el: Element): Element | null {
    for (let up = el.parentElement; up; up = up.parentElement) {
      const overflow = getComputedStyle(up).overflowY;
      if (overflow === "auto" || overflow === "scroll") return up;
    }
    return null;
  }

  function observerOf(root: Element | null): IntersectionObserver {
    let observer = root ? observers.get(root) : windowObserver;
    if (!observer) {
      observer = new IntersectionObserver(
        (entries) => {
          for (const entry of entries) watchers.get(entry.target)?.(entry.isIntersecting);
        },
        { root, rootMargin: `${AHEAD}px 0px` },
      );
      if (root) observers.set(root, observer);
      else windowObserver = observer;
      // A block that sat at the top of the list must not move the code on screen when it is drawn.
      if (root instanceof HTMLElement) root.style.overflowAnchor = "none";
    }
    return observer;
  }

  /** Whether `el` is within reach of the visible part of `root`, measured now. */
  function nearNow(el: Element, root: Element | null): boolean {
    const box = el.getBoundingClientRect();
    const view = root ? root.getBoundingClientRect() : { top: 0, bottom: window.innerHeight };
    return box.bottom >= view.top - AHEAD && box.top <= view.bottom + AHEAD;
  }

  /** Blocks with matches of Find in file, in order, and how many each has. */
  export type FindHits = { key: string; count: number }[];

  /** Which block the `at`th match is in, and which of its matches it is. */
  export function placeOf(hits: FindHits, at: number): { key: string; nth: number } | null {
    for (const hit of hits) {
      if (at < hit.count) return { key: hit.key, nth: at };
      at -= hit.count;
    }
    return null;
  }

  /** The matches drawn in `el`, in order, each the spans it was cut into by the colors. */
  export function matchesIn(el: Element): HTMLElement[][] {
    const out: HTMLElement[][] = [];
    let line: Element | null = null;
    let hit: string | null = null;
    for (const span of el.querySelectorAll<HTMLElement>(".found")) {
      if (span.parentElement === line && span.dataset.hit === hit) out[out.length - 1].push(span);
      else out.push([span]);
      line = span.parentElement;
      hit = span.dataset.hit ?? null;
    }
    return out;
  }

  /** Whether the user has text selected inside `el`: its lines stay, or copying would lose them. */
  function holdsSelection(el: Element): boolean {
    const selection = document.getSelection();
    return !!selection && !selection.isCollapsed && selection.containsNode(el, true);
  }
</script>

<script lang="ts">
  import { tick, type Snippet } from "svelte";

  let {
    lines,
    of,
    force = false,
    name,
    marks = [],
    children,
  }: {
    /** Lines in the block, for its height before it is drawn. */
    lines: number;
    /** What the block shows: another one is measured afresh. */
    of: unknown;
    /** Draw the block wherever it is, e.g. as Find in file is on one of its matches. */
    force?: boolean;
    /** Name of the block, for Find in file to find it on the page. */
    name?: string;
    /** Rows of the block where a change starts, for Next Change to step to them drawn or not. */
    marks?: number[];
    children: Snippet;
  } = $props();

  let box = $state<HTMLDivElement>();
  let root: Element | null = null;
  let near = $state(false);
  let lineHeight = $state(18);
  /** Height the block had when last drawn. */
  let height = $state<number | null>(null);
  const live = $derived(near || force);

  // Lines about to go: their height keeps their place.
  let drawn = false;
  $effect.pre(() => {
    const now = live;
    if (drawn && !now && box) height = box.offsetHeight;
    drawn = now;
  });

  // Other content: its height is not known yet.
  $effect(() => {
    of;
    height = null;
  });

  $effect(() => {
    const el = box!;
    root = scrollerOf(el);
    if (root) {
      if (!lineHeights.has(root)) {
        const line = parseFloat(getComputedStyle(el).lineHeight);
        lineHeights.set(root, Number.isFinite(line) && line > 0 ? line : 18);
      }
      lineHeight = lineHeights.get(root)!;
    }
    // Measured at once, so the first frame already has the lines on screen.
    near = nearNow(el, root);
    const observer = observerOf(root);
    watchers.set(el, (now) => {
      if (now === near) return;
      if (!now) {
        if (!holdsSelection(el)) near = false;
        return;
      }
      const before = el.offsetHeight;
      near = true;
      // Drawn above the visible part, the block grows or shrinks from its estimated height:
      // the list scrolls by as much, so the code on screen stays where it is.
      tick().then(() => {
        const grew = el.offsetHeight - before;
        if (grew && root && el.getBoundingClientRect().top < root.getBoundingClientRect().top) root.scrollTop += grew;
      });
    });
    observer.observe(el);
    return () => {
      observer.unobserve(el);
      watchers.delete(el);
    };
  });

  const estimate = $derived(lines * lineHeight);
</script>

<div bind:this={box} class="lazy" data-block={name} data-marks={marks.length ? marks.join(",") : undefined} style:height={live ? undefined : `${height ?? estimate}px`}>
  {#if live}{@render children()}{/if}
</div>

<style>
  .lazy {
    overflow-anchor: none;
  }
</style>
