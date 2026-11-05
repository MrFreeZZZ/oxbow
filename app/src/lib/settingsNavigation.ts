// Window startup and live navigation share the same destinations.
export function settingsSection(target: string): string | null {
  switch (target) {
    case "general": return "general";
    case "commands": return "commands";
    case "updates": return "general";
    default: return null;
  }
}

export function settingsRoute(hash: string): string | null {
  if (hash === "#settings") return "general";
  return hash.startsWith("#settings/") ? settingsSection(hash.slice("#settings/".length)) : null;
}
