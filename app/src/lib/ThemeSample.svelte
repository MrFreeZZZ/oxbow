<script lang="ts">
  // Settings › Themes: a sample diff drawn by the real code view, in the chosen theme.

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
        header: "@@ -18,7 +18,7 @@",
        oldStart: 18,
        oldLines: 7,
        newStart: 18,
        newLines: 7,
        lines: [
          { kind: "context", oldLine: 18, newLine: 18, text: "impl Default for Config {", words: null },
          { kind: "context", oldLine: 19, newLine: 19, text: "    fn default() -> Self {", words: null },
          { kind: "context", oldLine: 20, newLine: 20, text: "        // Give up on a slow login after this long", words: null },
          {
            kind: "removed",
            oldLine: 21,
            newLine: null,
            text: "        Self { login_timeout: Duration::from_secs(10), name: \"oxbow\" }",
            words: [
              { text: "        Self { login_timeout: Duration::from_secs(", changed: false },
              { text: "10", changed: true },
              { text: "), name: \"oxbow\" }", changed: false },
            ],
          },
          {
            kind: "added",
            oldLine: null,
            newLine: 21,
            text: "        Self { login_timeout: Duration::from_secs(30), name: \"oxbow\" }",
            words: [
              { text: "        Self { login_timeout: Duration::from_secs(", changed: false },
              { text: "30", changed: true },
              { text: "), name: \"oxbow\" }", changed: false },
            ],
          },
          { kind: "context", oldLine: 22, newLine: 22, text: "    }", words: null },
          { kind: "context", oldLine: 23, newLine: 23, text: "}", words: null },
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
