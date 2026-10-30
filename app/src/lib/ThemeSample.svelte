<script lang="ts">
  // Settings › Appearance: a sample diff drawn by the real code view, in the chosen theme.

  import DiffView from "./DiffView.svelte";
  import type { FileDiff } from "./types";

  const sample: FileDiff = {
    file: { path: "src/config.rs", oldPath: null, status: "modified", additions: 1, deletions: 1, binary: false },
    tooLarge: false,
    limited: false,
    old: null,
    new: null,
    hunks: [
      {
        header: "@@ -1,3 +1,3 @@",
        oldStart: 1,
        oldLines: 3,
        newStart: 1,
        newLines: 3,
        lines: [
          { kind: "context", oldLine: 1, newLine: 1, text: "// Give up on a slow login after this long", words: null },
          {
            kind: "removed",
            oldLine: 2,
            newLine: null,
            text: "const TIMEOUT: Duration = Duration::from_secs(10);",
            words: [
              { text: "const TIMEOUT: Duration = Duration::from_secs(", changed: false },
              { text: "10", changed: true },
              { text: ");", changed: false },
            ],
          },
          {
            kind: "added",
            oldLine: null,
            newLine: 2,
            text: "const TIMEOUT: Duration = Duration::from_secs(30);",
            words: [
              { text: "const TIMEOUT: Duration = Duration::from_secs(", changed: false },
              { text: "30", changed: true },
              { text: ");", changed: false },
            ],
          },
          { kind: "context", oldLine: 3, newLine: 3, text: 'const NAME: &str = "oxbow";', words: null },
        ],
      },
    ],
  };
</script>

<div class="theme-preview">
  <DiffView diffs={[sample]} color={0} whole={false} onToggleWhole={() => {}} openable={false} />
</div>

<style>
  .theme-preview {
    border-radius: var(--radius-l);
    border: 0.5px solid var(--panel-border);
    padding-top: 8px;
    background: var(--win);
  }
</style>
