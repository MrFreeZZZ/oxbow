# Settings reorganization — visual QA

Date: 2026-10-30. Scope: the Settings window, its navigation, grouping, search and previews.

final result: passed

The earlier native startup blocker was resolved after the user confirmed a file
access request and granted access. Both the unchanged installed app and the final
rebuilt package subsequently opened oxbow successfully. The final package's local
repository Settings were rechecked and recaptured, including the final Protected
branch label and Unprotect action. No application-code fix was needed for Opening…;
the observations are consistent with waiting for the pending access request.
The checked build did not replace the installed application.

## Visual sources and captures

Source directory:
`/Users/aleksandrmarin/.codex/visualizations/2026/10/30/01a18bf1-52eb-7741-ad8c-25f7fe2d41d2/settings-mockups/`

Implementation directory:
`/Users/aleksandrmarin/.codex/visualizations/2026/10/30/01a18bf1-52eb-7741-ad8c-25f7fe2d41d2/settings-implementation/`

| Source | Implementation | Comparison |
| --- | --- | --- |
| general.png | 01-general.png | compare-general.png; focused-general.png |
| appearance.png | 02-appearance.png | compare-appearance.png; focused-appearance.png |
| safety-recovery.png | 03-safety.png; 04-recovery.png | compare-safety-recovery.png; focused-safety-recovery.png |

Sources: 1484 × 1060 pixels. App-window captures: 1201 × 856 pixels, matching the
existing Settings window. Each source was normalized to the capture dimensions;
the aspect-ratio difference is less than 0.3%. Full comparisons are 2402 × 856 pixels,
with the source on the left and implementation on the right. Focused crops compare
the actual controls and theme swatches at the same normalized scale.

This is a native Tauri/WebKit release build, not a browser prototype; browser
deviceScaleFactor and a separate CSS viewport override do not apply. The capture
tool's window-sized output was used. The active state is light/System appearance.
Actual preferences (including the selected Signal palette, code font and custom
secret patterns) were preserved rather than changed to the mock's sample data.
Repository-specific sections were additionally captured after opening oxbow.

## Findings and comparison history

- Initial Appearance capture: the original eight-row sample pushed the branch palette
  below the first viewport. Fixed by using a four-row sample (two context rows and
  the old/new changed line), still rendered with the real DiffView. Rebuilt and
  recaptured: `02-appearance.png` and `compare-appearance.png` show the palette controls
  within the viewport. The graph continues below it and is reachable by scrolling.
- Initial local Branches capture: putting production and protected branches together
  removed the explicit Protected Branches heading. Fixed by identifying each row as
  “Protected branch” and naming its action “Unprotect”. The handler and protection
  settings are unchanged. The final grouping capture is `05-repository.png`;
  the final copy change was type-checked and verified in the rebuilt app.
- No remaining actionable P0/P1/P2 findings within the Settings reorganization scope.
- Earlier [P1, validation blocker] Opening… on native restart: reproduced in the
  unchanged installed version as well. A process sample showed a wait in the OS
  file-open call for Git metadata; the CLI example read the repository normally.
  The user confirmed the pending file access request and granted access. Reopened
  the final bundle successfully, inspected the repository sidebar/history, then
  rechecked local Settings and Git (`05-repository.png`, `10-git.png`). Blocker cleared.

## Required fidelity surfaces

- **Typography:** retained the application's existing system UI font, sizes and line
  heights. The generated mocks have larger type and spacing; preserving the existing
  design system is intentional. No typography changes were needed for this task.
- **Spacing and layout:** sidebar, grouped rows and header reuse existing components.
  Appearance has a responsive theme grid and adjacent previews. Long pages scroll
  without displacing the sidebar or persistent header actions. Code and console
  previews now follow the controls they demonstrate (`09-text-console.png`).
- **Colors and tokens:** retained the existing steel accent and theme tokens. The new
  safety entry uses the existing key icon and muted blue section token. The mock's
  shield and vivid red General-sidebar icon were not introduced as a separate style.
- **Assets:** retained ThemeCards, ThemeSample and BranchPreview, using actual theme
  palettes and the existing code-rendering components. No rasterized controls or
  generated mockup images were embedded in the application.
- **Copy and content:** moved all existing settings without changing their stored
  keys or defaults. Updated help paths, group names and the local protection label.
  The shared-file explanation precedes the file list (`07-repository-files.png`).

The user's explicit exception overrides the mock: Restore defaults and Edit in
settings.json remain in the header on every existing settings page. The ellipsis
from the mocks was not implemented. Global reset behavior was not changed.

## Interaction and build validation

- Native release build launched and the new settings navigation opened successfully.
- General, Appearance, Safety & Recovery, local repository settings, Repository Files,
  and Diff & Text were inspected. Header actions remained visible on these pages.
- Search for “fetch” found Git → Fetch, Git → Pull, branch-creation fetch and the
  keyboard command (`06-search-fetch.png`). No preview galleries appear in results.
- Scrolled to Undo history/Local History and Code text/Console & Activity; their
  controls and previews remain reachable and grouped correctly.
- Compared the inventory of 56 preference/Git edit/switch rows before and after:
  the keys and occurrence counts are unchanged.
- `scripts/check.sh ui`: green, 107 tests passed; svelte-check: 0 errors, 0 warnings.
- Production frontend and macOS app bundle built successfully. Vite's size advisory
  for a chunk above 500 kB remains; it did not fail the build.
- `git diff --check`: clean.
- The unsigned Tauri bundle initially failed `codesign --verify --deep --strict`.
  Applied a local ad-hoc signature to the complete bundle and verified it successfully,
  then repackaged the ZIP. This does not provide Developer ID signing or notarization.
  The access-request issue was resolved separately through the user's permission
  response; the later native repository restart succeeded.

Preferences, security switches, trusted repositories and Git config were not changed
during the UI inspection. Save/reset operations, a full VoiceOver audit, dark-mode
regression, minimum-window layout and unrelated Git workflows were not exercised.
This is a Settings QA result, not a full application release certification. Native
WebKit developer-console errors were not collected; no Settings failure banner or
blank settings screen was observed.

## Implementation checklist

- [x] Reorganize sections and keep storage/handlers unchanged.
- [x] Preserve both header buttons as requested.
- [x] Attach previews to group metadata instead of numeric group positions.
- [x] Update in-app references to moved settings.
- [x] Run UI checks and build the native app.
- [x] Inspect native screenshots and combined comparisons.
- [x] Package the local app as `target/release/bundle/macos/Oxbow-settings.zip`.

Follow-up polish: none required for this change; broader accessibility and other
application workflows remain outside this inspection.
