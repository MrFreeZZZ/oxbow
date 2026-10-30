<script lang="ts">
  // One row of the Settings window: its label and sub line, the gear with its menu, and its
  // control (segments, popup, switch, a value with Edit, a button or a command's keys).

  import { model } from "./settingsModel.svelte";
  import ShortcutKeys from "./ShortcutKeys.svelte";
  import { GEAR, text, withCurrent, type Row } from "./settingsRows";

  let {
    row,
    menuOn,
    onMenu,
    editing = $bindable(),
  }: {
    row: Row;
    /** Its gear menu is open. */
    menuOn: boolean;
    onMenu: (x: number, y: number) => void;
    /** The text value being edited, in any row. */
    editing: { id: string; value: string } | null;
  } = $props();

  const off = $derived(row.off?.() || (model.loading && row.setting?.kind === "git" && row.setting.scope === "local"));
  const control = $derived(row.control);
  const badge = $derived(row.badge?.());

  function saveEdit() {
    if (!editing || row.control?.type !== "edit") return;
    row.control.save(editing.value);
    editing = null;
  }
</script>

<div
  class="row"
  role="group"
  aria-label={text(row.label)}
  oncontextmenu={(event) => {
    event.preventDefault();
    onMenu(event.clientX, event.clientY);
  }}
>
  {#if row.setting || row.more}
    <button
      class="gear"
      class:on={menuOn}
      aria-label="More actions for {text(row.label)}"
      onclick={(event) => {
        const box = event.currentTarget.getBoundingClientRect();
        onMenu(box.left, box.bottom + 4);
      }}
    >
      <svg class="icon" viewBox="0 0 16 16"><path d={GEAR} /></svg>
    </button>
  {/if}
  {#if row.file}
    {@const file = row.file()}
    <span class="file" class:missing={file.missing} aria-hidden="true">
      <svg class="icon" viewBox="0 0 16 16"><path d={file.missing ? "M8 4.5v7M4.5 8h7" : "M4 1.8h5l3 3v9.4H4zM9 1.8v3h3"} /></svg>
    </span>
  {/if}
  {#if row.image?.()}<img class="avatar" src={row.image()} alt="" />{/if}
  <span class="text" class:off>
    <span class="name" class:mono={row.mono}>{text(row.label)}</span>
    {#if text(row.sub)}<span class="sub {row.subTone?.() ?? ''}">{text(row.sub)}</span>{/if}
  </span>
  {#if badge}<span class="chip">{badge}</span>{/if}
  {#if control?.type === "seg"}
    <span class="seg" class:off role="radiogroup" aria-label={text(row.label)}>
      {#each control.options() as option (option.value)}
        <button role="radio" aria-checked={control.get() === option.value} disabled={off} onclick={() => control.set(option.value)}>{option.label}</button>
      {/each}
    </span>
  {:else if control?.type === "popup"}
    {@const options = withCurrent(control.options(), control.get())}
    <span class="popup" class:off>
      <select
        aria-label={text(row.label)}
        disabled={off}
        value={String(control.get())}
        onchange={(event) => {
          const option = options.find((o) => String(o.value) === event.currentTarget.value);
          if (option) control.set(option.value);
        }}
      >
        {#each options as option (option.value)}
          <option value={String(option.value)}>{option.label}</option>
        {/each}
      </select>
      <svg class="icon" viewBox="0 0 16 16"><path d="M5 6.5 8 3.5l3 3M5 9.5l3 3 3-3" /></svg>
    </span>
  {:else if control?.type === "switch"}
    <button class="switch" class:off role="switch" aria-checked={control.get()} aria-label={text(row.label)} disabled={off} onclick={() => control.set(!control.get())}>
      <span></span>
    </button>
  {:else if control?.type === "edit"}
    {#if editing?.id === row.id}
      <!-- svelte-ignore a11y_autofocus -->
      <input
        class="field"
        bind:value={editing.value}
        placeholder={control.placeholder}
        spellcheck="false"
        autofocus
        onkeydown={(event) => {
          if (event.key === "Enter") saveEdit();
          else if (event.key === "Escape") editing = null;
        }}
      />
      <button class="button small" onclick={() => (editing = null)}>Cancel</button>
      <button class="button small primary" onclick={() => saveEdit()}>Save</button>
    {:else}
      <span class="value" class:off>{control.shown?.() ?? control.get()}</span>
      <button class="button small" disabled={off} onclick={() => (editing = { id: row.id, value: control.get() })}>Edit</button>
    {/if}
  {:else if control?.type === "button"}
    <button class="button small" class:danger={control.danger} disabled={off} onclick={control.run}>{text(control.label)}</button>
  {:else if control?.type === "keys"}
    <ShortcutKeys id={control.id} />
  {/if}
</div>

<style>
  .row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 44px;
    padding: 7px 12px 7px 14px;
  }

  /* Rows are siblings in the panel, each its own component. */
  :global(.row) + .row {
    border-top: 0.5px solid var(--sep);
  }

  .gear {
    position: absolute;
    left: -27px;
    top: 50%;
    margin-top: -10px;
    width: 20px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-pill);
    color: var(--text2);
    opacity: 0;
  }

  .gear .icon {
    width: 14px;
    height: 14px;
    stroke-width: 1.3;
  }

  .row:hover .gear,
  .gear.on,
  .gear:focus-visible {
    opacity: 1;
  }

  .gear.on {
    background: var(--field);
  }

  .text {
    flex-grow: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    line-height: 1.3;
  }

  .name,
  .sub {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .name.mono {
    font-family: var(--mono);
    font-size: var(--text-small);
  }

  .sub {
    font-size: var(--text-caption);
    color: var(--text2);
  }

  .off {
    opacity: 0.4;
  }

  .avatar {
    width: 30px;
    height: 30px;
    border-radius: var(--radius-pill);
    flex-shrink: 0;
    background: var(--field);
  }

  .file {
    width: 26px;
    height: 26px;
    border-radius: 7px;
    background: var(--field);
    color: var(--icon);
    display: grid;
    place-items: center;
    flex-shrink: 0;
  }

  .file .icon {
    width: 14px;
    height: 14px;
  }

  .file.missing {
    background: none;
    border: 1px dashed var(--glass-border);
    color: var(--text2);
  }

  .chip {
    flex-shrink: 0;
    height: 20px;
    line-height: 20px;
    padding: 0 9px;
    border-radius: var(--radius-pill);
    font-size: var(--text-caption);
    font-weight: 500;
    background: var(--field);
    color: var(--text2);
    white-space: nowrap;
  }

  .value {
    flex-shrink: 0;
    max-width: 50%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--text-small);
    color: var(--text2);
  }

  .field {
    flex: 0 1 260px;
    min-width: 120px;
    height: 26px;
    padding: 0 9px;
    border-radius: var(--radius-m);
    border: 0.5px solid var(--glass-border);
    background: var(--win);
    color: var(--text);
    font: inherit;
    font-size: var(--text-small);
    outline: 2px solid var(--accent);
    outline-offset: 0;
  }

  .button.small {
    min-width: 72px;
  }

  .seg {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: var(--radius-m);
    background: var(--field);
    flex-shrink: 0;
  }

  .seg button {
    min-width: 72px;
    height: 24px;
    padding: 0 12px;
    border-radius: var(--radius-s);
    text-align: center;
    font-size: var(--text-small);
  }

  .seg button[aria-checked="true"] {
    background: var(--seg-on);
    box-shadow: var(--seg-shadow);
    font-weight: 600;
  }

  .popup {
    position: relative;
    flex-shrink: 0;
    display: flex;
    align-items: center;
  }

  .popup select {
    -webkit-appearance: none;
    appearance: none;
    height: 26px;
    padding: 0 26px 0 11px;
    border-radius: var(--radius-m);
    background: var(--glass);
    border: 0.5px solid var(--glass-border);
    box-shadow: var(--ctrl-shadow);
    color: var(--text);
    font: inherit;
    font-size: var(--text-small);
    outline: 0;
  }

  .popup .icon {
    position: absolute;
    right: 7px;
    width: 12px;
    height: 12px;
    color: var(--text2);
    pointer-events: none;
  }

  .switch {
    width: 36px;
    height: 20px;
    border-radius: var(--radius-pill);
    background: var(--switch-off);
    position: relative;
    flex-shrink: 0;
    transition: background 0.15s;
  }

  .switch span {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: var(--radius-pill);
    background: #ffffff;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.25);
    transition: left 0.15s;
  }

  .switch[aria-checked="true"] {
    background: var(--accent);
  }

  .switch[aria-checked="true"] span {
    left: 18px;
  }

  .sub.warn {
    color: var(--orange);
  }

  .sub.error {
    color: var(--danger);
  }

  .sub.warn,
  .sub.error {
    white-space: normal;
  }
</style>
