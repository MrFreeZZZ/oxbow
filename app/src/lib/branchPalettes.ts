// Branch palettes (Settings › Themes › Branches & graph). A palette colors branches everywhere:
// the graph, the sidebar, capsules, stacks and selection. Color 0 is the production branch (the
// trunk), the others are what branch names hash into. Values from the Branch palettes v2 spec;
// B7 of Mineral, Paper and Signal is Clay. Oxbow is the original palette and lives in app.css.
// Stash (10) and no-branch (11) grays are the same in every palette, also in app.css.

export type BranchPaletteId = "mineral" | "paper" | "signal" | "oxbow";

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
      bg: "#181D27",
      lines: ["#73B4EB", "#CDAFF5", "#6DD7E4", "#C59175", "#DADEEB", "#789CAB", "#9890DB", "#F9C191"],
      plates: ["#C2E2FE", "#ECE2FC", "#BBF5FD", "#F7CBB3", "#F7F9FF", "#B2D8E8", "#D2CFFA", "#F9C191"],
    },
  },
  {
    id: "paper",
    name: "Paper",
    note: "Warm neutral, iris production",
    light: {
      bg: "#F8F5F0",
      lines: ["#544380", "#4468A0", "#147C7F", "#694936", "#3F312A", "#5C8BAA", "#9976B7", "#AC704E"],
      plates: ["#484063", "#445A7B", "#276163", "#5F402D", "#362821", "#3B617B", "#685578", "#975D3B"],
    },
    dark: {
      bg: "#211F24",
      lines: ["#9E98E1", "#A3D3FE", "#5DC2C9", "#B18D6D", "#F2DFD8", "#6C9AAF", "#D6AFF4", "#F4B390"],
      plates: ["#D5D3FF", "#E4F2FF", "#AFEBEF", "#F0CBAB", "#FFFCFA", "#A8D8EE", "#F1E3FC", "#F4B390"],
    },
  },
  {
    id: "signal",
    name: "Signal",
    note: "Graphite, cyan production",
    light: {
      bg: "#F3F6F7",
      lines: ["#04808A", "#9777BF", "#3C609A", "#6B4222", "#31363C", "#6F889C", "#4E4171", "#A57543"],
      plates: ["#286167", "#66567B", "#435778", "#5C3B22", "#282D33", "#475F72", "#443B5E", "#8F612E"],
    },
    dark: {
      bg: "#131B20",
      lines: ["#81DBE7", "#D2B7F9", "#80BAF5", "#ECB699", "#DAE1EB", "#629BB3", "#A093DD", "#B2814E"],
      plates: ["#C6F7FE", "#EFE7FD", "#CCE4FE", "#FCE4D8", "#F7FAFF", "#A5D7ED", "#D7D1FC", "#CE9B68"],
    },
  },
  {
    id: "oxbow",
    name: "Oxbow",
    note: "The original muted palette, steel production",
    light: {
      bg: "#F7F6F2",
      lines: ["#4A6680", "#487E79", "#77BBC3", "#4D90AC", "#7FA0D0", "#64619B", "#9B8DB5", "#77546F", "#CC986E", "#846047"],
      plates: ["#3B5266", "#04534E", "#0B5158", "#064F67", "#174687", "#43388A", "#513975", "#68305D", "#6A3A07", "#6D3705"],
    },
    dark: {
      bg: "#252422",
      lines: ["#94B2CC", "#5D938E", "#8DD2DA", "#63A6C2", "#94B6E7", "#7875B1", "#B1A2CB", "#8C6884", "#E3AE83", "#99745B"],
      plates: ["#94B2CC", "#96D9D3", "#93D8E0", "#9AD4EE", "#AECDF9", "#C5C5F7", "#D2C1F2", "#E8BADD", "#EDC09D", "#EFBF9F"],
    },
  },
];

export const branchPaletteById = (id: string): BranchPalette => BRANCH_PALETTES.find((p) => p.id === id) ?? BRANCH_PALETTES[0];

/** CSS variables for a palette in a look. Oxbow's are app.css's own, so it sets none and the
 *  others' are removed (`null`). */
export function branchVariables(palette: BranchPalette, look: "light" | "dark"): Record<string, string | null> {
  const vars: Record<string, string | null> = { "--graph-bg": null };
  for (let i = 0; i < 10; i++) {
    vars[`--lane-${i}`] = null;
    vars[`--plate-${i}`] = null;
  }
  if (palette.id === "oxbow") return vars;
  const { bg, lines, plates } = palette[look];
  vars["--graph-bg"] = bg;
  lines.forEach((color, i) => (vars[`--lane-${i}`] = color));
  plates.forEach((color, i) => (vars[`--plate-${i}`] = color));
  return vars;
}
