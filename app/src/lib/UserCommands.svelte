<script lang="ts">
  import { prefs } from "./prefs.svelte";
  import { commandsProblem, type UserCommand } from "./userCommands";
  let draft = $state<UserCommand | null>(null);
  let error = $state("");
  let saving = $state(false);
  const commands = $derived(prefs.get("oxbow.commands"));
  async function save(next: UserCommand[]) {
    error = commandsProblem(next) ?? "";
    if (error) return;
    saving = true;
    const saved = await prefs.set("oxbow.commands", next);
    saving = false;
    if (!saved) error = prefs.failure?.error ?? "Couldn’t save commands. Try again.";
    else draft = null;
  }
</script>

<div class="commands">
  <p>Run your own scripts from the Command Palette or the repository’s Commands menu. Assign keys in Keyboard Shortcuts.</p>
  {#each commands as c (c.id)}
    <div class="command">
      <div><strong>{c.name}</strong><pre>{c.script}</pre><small>{c.repository || "All repositories"}</small></div>
      <button class="button" disabled={saving} onclick={() => { draft = { ...c }; error = ""; }}>Edit</button>
      <button class="button" disabled={saving} onclick={() => save(commands.filter((item) => item.id !== c.id))}>Delete</button>
    </div>
  {/each}
  <button class="button" disabled={saving} onclick={() => { draft = { id: crypto.randomUUID(), name: "", script: "", repository: "" }; error = ""; }}>Add Command…</button>
  {#if draft}
    <form onsubmit={(e) => { e.preventDefault(); if (draft) save(commands.some((c) => c.id === draft!.id) ? commands.map((c) => c.id === draft!.id ? { ...draft! } : c) : [...commands, { ...draft }]); }}>
      <label>Name<input bind:value={draft.name} maxlength="200" required /></label>
      <label>Script<textarea bind:value={draft.script} rows="4" maxlength="65536" required spellcheck="false"></textarea></label>
      <label>Repository path<input bind:value={draft.repository} placeholder="All repositories" spellcheck="false" /></label>
      <p>Runs in the repository root using your login shell. Quote context variables: <code>"$OXBOW_REPOSITORY"</code>, <code>"$OXBOW_BRANCH"</code>, <code>"$OXBOW_COMMIT"</code>. Branch and commit describe your selection; files stay on the checked-out branch.</p>
      <p>Commands can change files and run repository programs. Restricted mode blocks them. Script changes have no Oxbow Undo. Output appears in Console; avoid putting secrets in scripts or output.</p>
      <div class="buttons"><button class="button" type="button" disabled={saving} onclick={() => draft = null}>Cancel</button><button class="button primary" disabled={saving}>Save</button></div>
    </form>
  {/if}
  {#if error}<p role="alert">{error}</p>{/if}
</div>

<style>
  .commands { max-width: 740px; }
  p, small { color: var(--text2); font-size: var(--text-small); line-height: 1.6; }
  .command { display: flex; gap: 12px; align-items: center; padding: 14px 0; border-bottom: 1px solid var(--sep); }
  .command > div { flex: 1; min-width: 0; }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; margin: 6px 0; font-size: var(--text-small); }
  small { overflow-wrap: anywhere; }
  form { margin-top: 20px; padding: 18px; border: 1px solid var(--sep); border-radius: var(--radius-m); background: var(--field); }
  label { display: grid; gap: 6px; margin-bottom: 14px; }
  input, textarea { width: 100%; box-sizing: border-box; }
  input, textarea { padding: 8px 10px; border: 1px solid var(--sep); border-radius: var(--radius-s); background: var(--win); color: var(--text); font: inherit; }
  textarea, code { font-family: var(--mono); }
  .buttons { display: flex; justify-content: flex-end; gap: 10px; }
</style>
