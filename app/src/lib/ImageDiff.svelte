<script lang="ts" module>
  export type ImageMode = "2up" | "swipe" | "onion" | "difference";
  /** The way pictures are compared, kept for every picture until changed. */
  export const imageView = $state<{ mode: ImageMode }>({ mode: "2up" });
</script>

<script lang="ts">
  // Two versions of a picture, the way GitHub and Kaleidoscope compare them: side by side,
  // a divider to swipe, one faded over the other, or their difference (black where nothing changed).

  import { api } from "./api";
  import type { FileDiff, Source } from "./types";
  import { bytes, imageType } from "./unusual";

  let { diff }: { diff: FileDiff } = $props();

  type Side = { url: string; width: number; height: number; size: number };
  let before = $state<Side | null>(null);
  let after = $state<Side | null>(null);
  let error = $state<string | null>(null);
  let swipe = $state(50);
  let onion = $state(50);

  async function load(source: Source | null, type: string): Promise<Side | null> {
    if (!source) return null;
    const data = await api.sourceBytes(source);
    const url = URL.createObjectURL(new Blob([data as Uint8Array<ArrayBuffer>], { type }));
    const img = new Image();
    img.src = url;
    await img.decode().catch(() => {});
    return { url, width: img.naturalWidth, height: img.naturalHeight, size: data.byteLength };
  }

  $effect(() => {
    const d = diff;
    const type = imageType(d.file.path) ?? "application/octet-stream";
    let gone = false;
    let urls: string[] = [];
    error = null;
    Promise.all([load(d.old, type), load(d.new, type)]).then(
      ([b, a]) => {
        urls = [b, a].filter((s): s is Side => !!s).map((s) => s.url);
        if (gone) return urls.forEach((u) => URL.revokeObjectURL(u));
        before = b;
        after = a;
      },
      (err) => {
        if (!gone) error = String(err);
      },
    );
    return () => {
      gone = true;
      urls.forEach((u) => URL.revokeObjectURL(u));
      before = after = null;
    };
  });

  const both = $derived(!!before && !!after);
  const mode = $derived(both ? imageView.mode : "2up");
  /** The stacked views draw both at the size of the newer one. */
  const ratio = $derived(after && after.height ? `${after.width} / ${after.height}` : "1");

  const scale = $derived.by(() => {
    if (!before || !after || !before.width || !after.width) return null;
    if (before.width === after.width && before.height === after.height) return null;
    const k = after.width / before.width;
    const word = k >= 1 ? `${+k.toFixed(1)}× bigger` : `${+(1 / k).toFixed(1)}× smaller`;
    return `Shown at the same size; the new one is ${word} in pixels.`;
  });

  function drag(event: PointerEvent) {
    const box = event.currentTarget as HTMLElement;
    const move = (e: PointerEvent) => {
      const r = box.getBoundingClientRect();
      swipe = Math.min(100, Math.max(0, ((e.clientX - r.left) / r.width) * 100));
    };
    move(event);
    box.setPointerCapture(event.pointerId);
    box.onpointermove = (e) => e.buttons && move(e);
    box.onpointerup = () => (box.onpointermove = null);
  }

  const MODES: [ImageMode, string][] = [
    ["2up", "2-Up"],
    ["swipe", "Swipe"],
    ["onion", "Onion Skin"],
    ["difference", "Difference"],
  ];
</script>

{#snippet caption(label: string, side: Side, kind: "before" | "after")}
  <span class="cap {kind}">{label}</span>
  <span class="dims">{side.width}×{side.height} · {bytes(side.size)}</span>
{/snippet}

<div class="image">
  {#if both}
    <div class="modes" role="tablist" aria-label="Compare pictures">
      {#each MODES as [value, label] (value)}
        <button role="tab" aria-selected={mode === value} class:on={mode === value} onclick={() => (imageView.mode = value)}>{label}</button>
      {/each}
    </div>
  {/if}

  {#if error}
    <p class="note">Could not load the picture: {error}</p>
  {:else if !before && !after}
    <p class="note">Loading…</p>
  {:else if mode === "2up"}
    <div class="two">
      {#if before}
        <figure>
          <figcaption>{@render caption(after ? "Before" : "Deleted", before, "before")}</figcaption>
          <div class="frame checker"><img src={before.url} alt="Before" /></div>
        </figure>
      {/if}
      {#if after}
        <figure>
          <figcaption>{@render caption(before ? "After" : "Added", after, "after")}</figcaption>
          <div class="frame checker"><img src={after.url} alt="After" /></div>
        </figure>
      {/if}
    </div>
  {:else if before && after}
    <div class="stack-wrap">
      <div class="labels">
        <span class="cap before">Before</span>
        <span class="cap after">After</span>
      </div>
      {#if mode === "swipe"}
        <div class="stack checker swipe" style:aspect-ratio={ratio} onpointerdown={drag} role="slider" aria-label="Divider" aria-valuenow={Math.round(swipe)} tabindex="0"
          onkeydown={(e) => {
            if (e.key === "ArrowLeft") swipe = Math.max(0, swipe - 5);
            if (e.key === "ArrowRight") swipe = Math.min(100, swipe + 5);
          }}>
          <img src={before.url} alt="Before" />
          <img src={after.url} alt="After" style:clip-path="inset(0 0 0 {swipe}%)" />
          <span class="divider" style:left="{swipe}%"><span class="knob">‹›</span></span>
        </div>
        <p class="hint">Drag across the picture to move the divider.</p>
      {:else if mode === "onion"}
        <div class="stack checker" style:aspect-ratio={ratio}>
          <img src={before.url} alt="Before" />
          <img src={after.url} alt="After" style:opacity={onion / 100} />
        </div>
        <label class="slider">
          <span>Before</span>
          <input type="range" min="0" max="100" bind:value={onion} aria-label="Opacity of the new version" />
          <span>After</span>
        </label>
      {:else}
        <div class="stack black" style:aspect-ratio={ratio}>
          <img src={before.url} alt="Before" />
          <img src={after.url} alt="After" class="diff" />
        </div>
        <p class="hint">Black where nothing changed; the brighter a spot, the more it changed.</p>
      {/if}
    </div>
  {/if}
  {#if scale}<p class="hint">{scale}</p>{/if}
</div>

<style>
  .image {
    position: relative;
    padding: 8px 6px 4px;
  }
  .modes {
    display: flex;
    gap: 2px;
    width: max-content;
    margin: 0 0 12px auto;
    padding: 2px;
    border-radius: 9px;
    background: var(--field);
  }
  .modes button {
    height: 22px;
    padding: 0 10px;
    border-radius: 7px;
    font-size: 12px;
    color: var(--text2);
  }
  .modes button.on {
    background: var(--win);
    color: var(--text);
    font-weight: 600;
    box-shadow: 0 0.5px 2px rgba(0, 0, 0, 0.12);
  }
  .two {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    gap: 14px;
  }
  figure {
    margin: 0;
    min-width: 0;
  }
  figcaption {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }
  .cap {
    height: 18px;
    padding: 0 8px;
    border-radius: 9px;
    font-size: 11px;
    font-weight: 600;
    display: inline-flex;
    align-items: center;
  }
  .cap.before {
    background: var(--del-word);
    color: var(--red);
  }
  .cap.after {
    background: var(--add-word);
    color: var(--green);
  }
  .dims {
    font-size: 11px;
    color: var(--text2);
  }
  .frame {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 220px;
    padding: 12px;
    border-radius: 10px;
    border: 1px solid var(--sep);
  }
  .frame img {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
  }
  /* Transparent parts read as transparent, not as white. */
  .checker {
    background-color: var(--win);
    background-image:
      linear-gradient(45deg, var(--field) 25%, transparent 25%, transparent 75%, var(--field) 75%),
      linear-gradient(45deg, var(--field) 25%, transparent 25%, transparent 75%, var(--field) 75%);
    background-size: 16px 16px;
    background-position:
      0 0,
      8px 8px;
  }
  .stack-wrap {
    max-width: 360px;
    margin: 0 auto;
  }
  .labels {
    display: flex;
    justify-content: space-between;
    margin-bottom: 8px;
  }
  .stack {
    position: relative;
    width: 100%;
    max-height: 360px;
    border-radius: 10px;
    border: 1px solid var(--sep);
    overflow: hidden;
  }
  .stack img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
  }
  .swipe {
    cursor: ew-resize;
    touch-action: none;
  }
  .divider {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    margin-left: -1px;
    background: var(--text);
    pointer-events: none;
  }
  .knob {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 22px;
    height: 22px;
    border-radius: 11px;
    background: var(--win);
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.25);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    color: var(--text);
  }
  .black {
    background: #000;
  }
  .diff {
    mix-blend-mode: difference;
  }
  .slider {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
    font-size: 11px;
    color: var(--text2);
  }
  .slider input {
    flex-grow: 1;
  }
  .hint,
  .note {
    margin: 10px 0 0;
    text-align: center;
    font-size: 11px;
    color: var(--text2);
  }
</style>
