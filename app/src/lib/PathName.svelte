<script lang="ts">
  // A file's path in a list of files: the name always whole, the folders before it cut from the
  // front («…/lib/») when the row is too narrow. The row's tooltip gives the whole path.
  import { splitPath } from "./format";

  let { path, folders = true, grow = true }: { path: string; folders?: boolean; grow?: boolean } = $props();
  const p = $derived(splitPath(path));
</script>

<span class="path-name" class:grow>
  <!-- Right to left so the ellipsis cuts the start; the marks keep the slashes where they are. -->
  {#if folders && p.dir}<span class="dir">{`‎${p.dir}‎`}</span>{/if}<span class="file">{p.name}</span>
</span>

<style>
  .path-name {
    min-width: 0;
    display: flex;
    white-space: nowrap;
    text-align: left;
  }
  .grow {
    flex-grow: 1;
  }
  .dir {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl;
    color: var(--text2);
  }
  .file {
    flex: 0 0 auto;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
