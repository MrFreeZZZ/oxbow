<script lang="ts" module>
  /** A pick shown as a chip: a person (with an initial) or a label (with its color). */
  export interface Pick {
    key: string;
    label: string;
    /** A person's full name, beside the username in the list. */
    sub?: string;
    /** A label's color. */
    color?: string;
  }
</script>

<script lang="ts">
  // Chips of what is picked, then a box that suggests more as it is typed: GitLab's people for
  // reviewers and the assignee, the project's labels. ↑ ↓ move in the list, Return picks, Escape
  // closes it.
  import { person } from "./format";

  let {
    chosen,
    find,
    onPick,
    onRemove,
    placeholder = "+ Add",
    label,
    people = false,
    single = false,
  }: {
    chosen: Pick[];
    /** What to suggest for the typed text; people come from GitLab, so this can wait. */
    find: (text: string) => Promise<Pick[]> | Pick[];
    onPick: (pick: Pick) => void;
    onRemove: (key: string) => void;
    placeholder?: string;
    label: string;
    /** Chips with an initial, as people. */
    people?: boolean;
    /** One at most: the box goes once one is picked. */
    single?: boolean;
  } = $props();

  const listId = $props.id();
  let text = $state("");
  let found = $state<Pick[]>([]);
  let at = $state(0);
  let open = $state(false);
  let asked = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  function look(value: string, wait: number) {
    const ask = ++asked;
    clearTimeout(timer);
    timer = setTimeout(async () => {
      const list = await Promise.resolve(find(value)).catch(() => [] as Pick[]);
      if (ask !== asked) return;
      found = list.filter((p) => !chosen.some((c) => c.key === p.key));
      at = 0;
    }, wait);
  }

  function typed(value: string) {
    text = value;
    open = true;
    look(value, people ? 200 : 0);
  }

  function pick(p: Pick | undefined) {
    if (!p) return;
    onPick(p);
    text = "";
    found = [];
    open = false;
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!found.length) return;
      at = (at + (e.key === "ArrowDown" ? 1 : found.length - 1)) % found.length;
      open = true;
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (open) pick(found[at]);
    } else if (e.key === "Escape" && open) {
      e.preventDefault();
      e.stopPropagation();
      open = false;
    } else if (e.key === "Backspace" && !text && chosen.length) {
      onRemove(chosen[chosen.length - 1].key);
    }
  }
</script>

<span class="chips">
  {#each chosen as c (c.key)}
    <span class="chip" class:person={people}>
      {#if people}
        <span class="avatar" style:background={person(c.key)}>{(c.label[0] ?? "?").toUpperCase()}</span>
      {:else if c.color}
        <span class="dot" style:background={c.color}></span>
      {/if}
      {c.label}
      <button class="remove" aria-label="Remove {c.label}" onclick={() => onRemove(c.key)}>×</button>
    </span>
  {/each}
  {#if !single || !chosen.length}
    <span class="box">
      <input
        class="add"
        {placeholder}
        aria-label={label}
        role="combobox"
        aria-controls={listId}
        aria-expanded={open && found.length > 0}
        aria-autocomplete="list"
        value={text}
        oninput={(e) => typed((e.currentTarget as HTMLInputElement).value)}
        onfocus={() => typed(text)}
        onblur={() => setTimeout(() => (open = false), 150)}
        onkeydown={keydown}
      />
      {#if open && found.length}
        <span class="list" id={listId} role="listbox" aria-label={label}>
          {#each found as f, i (f.key)}
            <button class="option" class:on={i === at} role="option" aria-selected={i === at} onmousedown={(e) => e.preventDefault()} onclick={() => pick(f)} onmouseenter={() => (at = i)}>
              {#if people}
                <span class="avatar" style:background={person(f.key)}>{(f.label[0] ?? "?").toUpperCase()}</span>
              {:else if f.color}
                <span class="dot" style:background={f.color}></span>
              {/if}
              <span class="name">{f.label}</span>
              {#if f.sub}<span class="sub">{f.sub}</span>{/if}
            </button>
          {/each}
        </span>
      {/if}
    </span>
  {/if}
</span>

<style>
  .chips {
    flex-grow: 1;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding-top: 1px;
    min-width: 0;
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
  .avatar {
    width: 18px;
    height: 18px;
    border-radius: 50%;
    color: #fff;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 8px;
    font-weight: 700;
    flex-shrink: 0;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .remove {
    color: var(--text2);
    font-size: var(--text-heading);
    line-height: 1;
  }
  .box {
    position: relative;
    display: inline-flex;
  }
  .add {
    width: 110px;
    height: 24px;
    padding: 0 6px;
    border: 0;
    border-radius: var(--radius-pill);
    background: transparent;
    color: var(--accent-text);
    font: inherit;
    font-size: var(--text-small);
    outline: none;
  }
  .list {
    position: absolute;
    top: 26px;
    left: -6px;
    z-index: 20;
    min-width: 220px;
    max-width: 320px;
    display: flex;
    flex-direction: column;
    padding: 4px;
    border-radius: var(--radius-m);
    background: var(--sheet);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--shadow-pop);
  }
  .option {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 26px;
    padding: 0 8px;
    border-radius: var(--radius-s);
    text-align: left;
    font-size: var(--text-small);
  }
  .option.on {
    background: var(--accent);
    color: #fff;
  }
  .option.on .sub {
    color: rgba(255, 255, 255, 0.8);
  }
  .name {
    white-space: nowrap;
  }
  .sub {
    min-width: 0;
    color: var(--text2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
