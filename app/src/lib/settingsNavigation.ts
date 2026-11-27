export const REPOSITORY_TAB_TAGS_GROUP = "repository-tab-tags";
export type SettingsTarget = "commands" | "updates" | typeof REPOSITORY_TAB_TAGS_GROUP;
type Destination = { section: string; group?: string };

// Window startup and live navigation share the same destinations.
export function settingsSection(target: string): Destination | null {
  switch (target) {
    case "general":
    case "updates": return { section: "general" };
    case "commands": return { section: "commands" };
    case REPOSITORY_TAB_TAGS_GROUP: return { section: "theme", group: REPOSITORY_TAB_TAGS_GROUP };
    default: return null;
  }
}

export function settingsRoute(hash: string): Destination | null {
  if (hash === "#settings") return settingsSection("general");
  return hash.startsWith("#settings/") ? settingsSection(hash.slice("#settings/".length)) : null;
}
