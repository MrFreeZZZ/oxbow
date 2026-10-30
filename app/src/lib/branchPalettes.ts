// Branch palettes (Settings › Appearance › Branches & graph). A palette colors branches everywhere:
// the graph, the sidebar, capsules, stacks and selection. Color 0 is the production branch (the
// trunk), the seven others are what branch names hash into (graph::BRANCH_COLORS in the core).
// Mineral is from the Branch palettes v2 spec, its B7 is Clay. Each palette has a character of
// its own: Paper is warm, Signal vivid, Winter cold on white. Oxbow is the original palette and
// lives in app.css. Stash (10) and no-branch (11) grays are the same in every palette, also in
// app.css.

export type BranchPaletteId = "mineral" | "paper" | "signal" | "winter" | "oxbow";

interface Look {
  /** Background of the graph and the other surfaces drawn on `--graph-bg`. */
  bg: string;
  /** Lines, dots and small marks, production first. */
  lines: string[];
  /** Small text on a surface or on a tint of the line color. */
  plates: string[];
}

export interface BranchPalette {
  id: BranchPaletteId;
  name: string;
  /** One line about its character, for Settings. */
  note: string;
  light: Look;
  dark: Look;
}

export const BRANCH_PALETTES: BranchPalette[] = [
  {
    id: "mineral",
    name: "Mineral",
    note: "Cool neutral, steel blue production",
    light: {
      bg: "#F5F7FB",
      lines: ["#286499", "#8868AC", "#038086", "#64442A", "#36383D", "#6B899D", "#484277", "#A77445"],
      plates: ["#3B5A77", "#645478", "#286266", "#5A3B21", "#2D2F34", "#436073", "#3F3C60", "#916030"],
    },
    dark: {
      bg: "#1E2026",
      lines: ["#73B4EB", "#CDAFF5", "#6DD7E4", "#C59175", "#DADEEB", "#789CAB", "#9890DB", "#F9C191"],
      plates: ["#C2E2FE", "#ECE2FC", "#BBF5FD", "#F7CBB3", "#F7F9FF", "#B2D8E8", "#D2CFFA", "#F9C191"],
    },
  },
  {
    id: "paper",
    name: "Paper",
    note: "Warm earth tones, iris production",
    light: {
      bg: "#F8F3EA",
      lines: ["#3C3469", "#AD5B33", "#367C78", "#5F4024", "#AA813C", "#84577D", "#335B83", "#362C24"],
      plates: ["#3C3663", "#924E2E", "#336B68", "#5C412A", "#7E5D24", "#7A5575", "#3A5B7D", "#342C25"],
    },
    dark: {
      bg: "#24201B",
      lines: ["#B6ADF3", "#EB9A75", "#80C6C5", "#A17E61", "#F0CB8D", "#B57DA8", "#6C96C2", "#EEE3D7"],
      plates: ["#C9C5F0", "#EDBBA4", "#A9D0D0", "#CFB9A7", "#E4CEAA", "#D8B4CF", "#A7C1DC", "#EAE3DC"],
    },
  },
  {
    id: "signal",
    name: "Signal",
    note: "Vivid and high contrast, cyan production",
    light: {
      bg: "#F3F6F7",
      lines: ["#0E8395", "#8552C2", "#2F5BC2", "#C56C09", "#33393E", "#47358D", "#834A2B", "#488ACB"],
      plates: ["#0F6C7B", "#764DAA", "#375EB5", "#975106", "#34383D", "#463983", "#7E4D33", "#30669B"],
    },
    dark: {
      bg: "#131B20",
      lines: ["#58E0F6", "#C399FF", "#618BF9", "#FFA659", "#E0E5EB", "#7B67CC", "#B87C59", "#5FBCF4"],
      plates: ["#92D8E5", "#CFB6F8", "#A0BDFE", "#F0BB90", "#E2E5E8", "#B9B1F1", "#DBB49F", "#98CCEE"],
    },
  },
  {
    id: "winter",
    name: "Winter",
    note: "White, cold blues and violets, navy production",
    light: {
      bg: "#FFFFFF",
      lines: ["#1D4176", "#4493D0", "#19838D", "#826BB3", "#5D646C", "#4947A0", "#055959", "#B47548"],
      plates: ["#24426F", "#2E6E9F", "#21737B", "#725F9B", "#5E646A", "#4A4B95", "#1C5757", "#925E39"],
    },
    dark: {
      bg: "#121A24",
      lines: ["#D4E7FC", "#65B2F1", "#89DAE3", "#B9A2EE", "#7D8792", "#7981DF", "#68ADA8", "#EAB084"],
      plates: ["#DAE6F2", "#9AC7ED", "#A7D4D9", "#C9BCEA", "#B5BBC2", "#B1B9F4", "#A0C7C4", "#E3BFA5"],
    },
  },
  {
    id: "oxbow",
    name: "Oxbow",
    note: "The original muted palette, steel production",
    light: {
      bg: "#F7F6F2",
      lines: ["#4A6680", "#487E79", "#77BBC3", "#4D90AC", "#64619B", "#77546F", "#CC986E", "#846047"],
      plates: ["#3B5266", "#04534E", "#0B5158", "#064F67", "#43388A", "#68305D", "#6A3A07", "#6D3705"],
    },
    dark: {
      bg: "#252422",
      lines: ["#94B2CC", "#5D938E", "#8DD2DA", "#63A6C2", "#7875B1", "#8C6884", "#E3AE83", "#99745B"],
      plates: ["#94B2CC", "#96D9D3", "#93D8E0", "#9AD4EE", "#C5C5F7", "#E8BADD", "#EDC09D", "#EFBF9F"],
    },
  },
];

export const branchPaletteById = (id: string): BranchPalette => BRANCH_PALETTES.find((p) => p.id === id) ?? BRANCH_PALETTES[0];

/** In the dark the graph keeps only a trace of the palette's paper: mostly the neutral gray of the
 *  window, so the graph and the details beside it don't read as two temperatures. */
const darkPaper = (bg: string) => `color-mix(in srgb, ${bg} 30%, #222225)`;

/** CSS variables for a palette in a look. Oxbow's are app.css's own, so it sets none and the
 *  others' are removed (`null`). */
export function branchVariables(palette: BranchPalette, look: "light" | "dark"): Record<string, string | null> {
  const vars: Record<string, string | null> = { "--graph-bg": null };
  for (let i = 0; i < 8; i++) {
    vars[`--lane-${i}`] = null;
    vars[`--plate-${i}`] = null;
  }
  if (palette.id === "oxbow") return vars;
  const { bg, lines, plates } = palette[look];
  vars["--graph-bg"] = look === "dark" ? darkPaper(bg) : bg;
  lines.forEach((color, i) => (vars[`--lane-${i}`] = color));
  plates.forEach((color, i) => (vars[`--plate-${i}`] = color));
  return vars;
}
