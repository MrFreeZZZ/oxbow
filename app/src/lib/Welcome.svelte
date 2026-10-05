<script lang="ts">
  let { loading, error, onOpen }: { loading: boolean; error: string | null; onOpen: () => void } = $props();
</script>

<div class="welcome" data-tauri-drag-region>
  <div class="card">
    <h1>Oxbow</h1>
    <p>Open a Git repository to see its history.</p>
    <button class="primary" onclick={onOpen} disabled={loading}>{loading ? "Opening…" : "Open Repository…"}</button>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </div>
</div>

<style>
  .welcome {
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    text-align: center;
  }
  h1 {
    margin: 0;
    font-size: 26px;
    font-weight: 700;
  }
  p {
    margin: 0;
    color: var(--text2);
  }
  .primary {
    margin-top: 8px;
    height: 34px;
    padding: 0 16px;
    border-radius: 17px;
    background: var(--accent);
    color: #fff;
    font-weight: 600;
  }
  .primary:disabled {
    opacity: 0.6;
  }
  .error {
    color: var(--red);
    max-width: 420px;
  }
</style>
