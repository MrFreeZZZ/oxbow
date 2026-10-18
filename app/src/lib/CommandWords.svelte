<script lang="ts">
  // The words of one command in a terminal block, colored. A subcommand or flag the dictionary
  // knows, a branch name and a SHA open a popover on click: what it means, or which commit it is.

  import { manual, meaning } from "./gitWords";
  import { lens, type Look } from "./lens.svelte";
  import { tokens } from "./term";

  let { text }: { text: string } = $props();

  const parts = $derived.by(() => {
    const list = tokens(text);
    const all = list.map((t) => t.word);
    const git = all[0]?.text === "git";
    return list.map((token) => {
      const { word } = token;
      let look: Look | null = null;
      if (word.kind === "sub" || word.kind === "flag") {
        const said = meaning(word, all);
        if (said) look = { kind: "word", word, meaning: said, manual: manual(all) };
      } else if (git && word.kind === "arg") {
        look = lens.find(word.text);
      }
      return { ...token, look };
    });
  });

  function open(event: MouseEvent, look: Look) {
    event.stopPropagation();
    lens.toggle(look, event.currentTarget as Element);
  }
</script>

{#each parts as part, k (k)}{#if part.look}{@const look = part.look}<button
      class="word"
      class:ref={look.kind !== "word"}
      style:color={part.color}
      class:bold={part.bold}
      aria-haspopup="dialog"
      onclick={(e) => open(e, look)}>{part.text}</button
    >{:else}<span style:color={part.color} class:bold={part.bold}>{part.text}</span>{/if}{" "}{/each}

<style>
  .word {
    all: unset;
    /* A button is laid out as an inline block, which would take the hanging indent of the line. */
    text-indent: 0;
    cursor: pointer;
    white-space: pre-wrap;
    text-decoration: underline dotted;
    text-decoration-color: color-mix(in srgb, currentColor 40%, transparent);
    text-underline-offset: 3px;
    text-decoration-thickness: 1px;
    border-radius: var(--radius-xs);
  }
  .word:hover {
    text-decoration-color: currentColor;
    background: var(--term-button);
  }
  .word:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .bold {
    font-weight: 600;
  }
</style>
