import { confirm, type Request } from "./confirm.svelte";
import { prefs } from "./prefs.svelte";
import type { PrefGroup } from "./settingsRows";

export function confirmSettingReset(keys: PrefGroup["keys"], label: string, warning: string) {
  if (confirm.request) return;
  const request: Request = {
    title: `Reset ${label}?`,
    body: [warning],
    icon: "reset",
    button: "Reset",
    always: true,
    danger: true,
    status: `Resetting ${label}…`,
    local: {
      commands: [],
      run: async () => {
        await prefs.saveGroupReset(keys);
        return `${label} reset to defaults.`;
      },
    },
    recover: (failure) => ({
      title: `Couldn’t reset ${label}`,
      body: [failure.output],
      icon: "reset",
      tone: "err",
      alt: { label: "Try Again…", request: () => request },
    }),
  };
  return confirm.run(request);
}
