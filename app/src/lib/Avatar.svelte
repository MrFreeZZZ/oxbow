<script lang="ts">
  // A GitHub account's picture, or its first letter on its own color while there is none.
  import { person } from "./format";

  let { login, url = "", size = 24 }: { login: string; url?: string; size?: number } = $props();
</script>

<span class="avatar" style:width="{size}px" style:height="{size}px" style:background={person(login)} style:font-size="{Math.round(size * 0.42)}px">
  {(login[0] ?? "?").toUpperCase()}
  {#if url}<img src={url} alt="" onerror={(e) => ((e.currentTarget as HTMLImageElement).style.display = "none")} />{/if}
</span>

<style>
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
</style>
