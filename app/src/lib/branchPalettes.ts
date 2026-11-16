// Branch palettes (Settings › Appearance › Branches & graph). A palette colors branches everywhere:
// the graph, the sidebar, capsules, stacks and selection. Color 0 is the production branch (the
// trunk), the seven others are what branch names hash into (graph::BRANCH_COLORS in the core).
// Mineral is from the Branch palettes v2 spec, its B7 is Clay. Each palette has a character of
// its own: Paper is warm, Signal vivid, Winter cold on white. Oxbow is the original palette and
// lives in app.css. Stash (10) and no-branch (11) grays are the same in every palette, also in
// app.css.

export type BranchPaletteId =
  | "mineral" | "paper" | "signal" | "winter" | "oxbow"
  | "violet"
  | "walnut"
  | "moss"
  | "blue-amber"
  | "sage-brass"
  | "teal-copper"
  | "berry-coral"
  | "iris-gold"
  | "slate-pink";

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
  {
    id: "violet",
    name: "Violet",
    note: "Cool violet and lavender, iris production",
    light: {
      bg: "#F7F4FC",
      lines: ["#6452A3", "#8A6D9E", "#5A3B8C", "#3A3A52", "#237881", "#A64E81", "#58773B", "#9E582F"],
      plates: ["#5D4C98", "#685276", "#5A3B8C", "#3A3A52", "#1D626A", "#88406A", "#476030", "#834927"],
    },
    dark: {
      bg: "#0E0B1D",
      lines: ["#8D7FC8", "#C9C2E3", "#B274D0", "#8E98B5", "#5BA1AC", "#CD8AB5", "#AABF87", "#CA936D"],
      plates: ["#C0B9E1", "#D0CAE7", "#D4B1E5", "#BBC1D3", "#9EC8CE", "#E0B8D2", "#BFCFA5", "#DEBCA4"],
    },
  },
  {
    id: "walnut",
    name: "Walnut",
    note: "Warm wood and stone, walnut production",
    light: {
      bg: "#FAF5EE",
      lines: ["#8B623E", "#4A4A46", "#8C7766", "#6B4A2E", "#36776D", "#AC5950", "#657434", "#716099"],
      plates: ["#765335", "#4A4A46", "#68584B", "#6B4A2E", "#2D635A", "#8A4740", "#535F2B", "#605282"],
    },
    dark: {
      bg: "#1A120E",
      lines: ["#B68A61", "#8A9295", "#D5C4AE", "#DE976A", "#82ADA2", "#C77D71", "#A7AD6B", "#A394C4"],
      plates: ["#D6BDA7", "#BDC2C4", "#DCCEBC", "#EBBFA3", "#B2CCC5", "#E0B8B1", "#C7CBA0", "#C9C0DC"],
    },
  },
  {
    id: "moss",
    name: "Moss",
    note: "Forest green and soft olive, moss production",
    light: {
      bg: "#F4F6EF",
      lines: ["#526B4D", "#1E3A2E", "#777B63", "#3E5A3F", "#946D30", "#397690", "#94596F", "#AD5134"],
      plates: ["#485E44", "#1E3A2E", "#585B49", "#3E5A3F", "#725425", "#2E5E73", "#7B4A5C", "#8E422B"],
    },
    dark: {
      bg: "#0F1F17",
      lines: ["#93AC86", "#589B88", "#C7CBB8", "#8F9B55", "#BD9A62", "#7EACC4", "#BC8FA2", "#D38563"],
      plates: ["#BFCEB8", "#A8CBC1", "#D2D5C5", "#C3C9A3", "#D9C5A6", "#B3CEDC", "#D9C0CB", "#E7BCA9"],
    },
  },
  {
    id: "blue-amber",
    name: "Blue & Amber",
    note: "Clear blue and amber, cyan production",
    light: {
      bg: "#F5F7FC",
      lines: ["#1D7E99", "#B86402", "#3A506B", "#90734F", "#646BB0", "#367D65", "#8D588E", "#B74E4A"],
      plates: ["#176277", "#8A4B02", "#3A506B", "#6D573C", "#51578F", "#2C6552", "#784B79", "#943F3C"],
    },
    dark: {
      bg: "#0B132B",
      lines: ["#5BC0DE", "#D97706", "#8291A2", "#F4D6B0", "#9DA1E1", "#80BEA6", "#BB8EB4", "#D97568"],
      plates: ["#98D7EA", "#ECBA80", "#BEC6CF", "#F4D6B0", "#C7C9EE", "#B0D7C8", "#DAC1D6", "#ECB9B2"],
    },
  },
  {
    id: "sage-brass",
    name: "Sage & Brass",
    note: "Soft greens and brass, sage production",
    light: {
      bg: "#F7F7F0",
      lines: ["#577656", "#946E32", "#3E5A3F", "#877D63", "#38758A", "#A25164", "#74618F", "#AC5231"],
      plates: ["#486247", "#735627", "#3E5A3F", "#645C49", "#2E6173", "#8A4555", "#64537B", "#904529"],
    },
    dark: {
      bg: "#0B0F0D",
      lines: ["#8CA98C", "#B08D57", "#7B8E7B", "#D9C7A3", "#78A6B9", "#C3828D", "#A093BC", "#C97753"],
      plates: ["#B7C9B7", "#D2BE9F", "#B6C1B6", "#DECEAE", "#ABC8D4", "#DDB8BE", "#C7BFD7", "#E1B4A0"],
    },
  },
  {
    id: "teal-copper",
    name: "Teal & Copper",
    note: "Muted teal and copper, teal production",
    light: {
      bg: "#F8F6F2",
      lines: ["#3E7880", "#A44E1B", "#205C61", "#867D6D", "#967131", "#705C9C", "#607F42", "#3A6D9F"],
      plates: ["#336269", "#8F4417", "#205C61", "#625B50", "#725625", "#635189", "#4B6333", "#315D87"],
    },
    dark: {
      bg: "#0A0F14",
      lines: ["#7FB3B8", "#DB782F", "#669699", "#EADCC8", "#C5A268", "#AEA2D7", "#9DBD80", "#6F9CC7"],
      plates: ["#B2D1D4", "#ECB993", "#B1C9CB", "#EADCC8", "#DDC8A6", "#CFC8E7", "#C0D5AE", "#B1CAE1"],
    },
  },
  {
    id: "berry-coral",
    name: "Berry & Coral",
    note: "Berry pink and coral, raspberry production",
    light: {
      bg: "#FCF5F6",
      lines: ["#AC3C72", "#C04A37", "#7A2E6F", "#947071", "#5267AD", "#946B36", "#347872", "#59763B"],
      plates: ["#973564", "#9A3B2C", "#7A2E6F", "#705556", "#455791", "#75552B", "#2B625D", "#496130"],
    },
    dark: {
      bg: "#1A0F2E",
      lines: ["#DA6A9A", "#FF6B6B", "#AF82A9", "#F4D7D4", "#889BDD", "#C9A269", "#76AFA8", "#A0BD82"],
      plates: ["#EDB6CE", "#FFB4B4", "#D7C0D4", "#F4D7D4", "#BFC9ED", "#E0CAAA", "#B2D2CE", "#C4D6B2"],
    },
  },
  {
    id: "iris-gold",
    name: "Iris & Gold",
    note: "Iris and gold, violet production",
    light: {
      bg: "#FAF7F0",
      lines: ["#6A5ACD", "#AD6800", "#8E5C9F", "#87734E", "#2E7394", "#44765D", "#B94A61", "#776657"],
      plates: ["#5B4DB0", "#855000", "#764C84", "#6B5B3E", "#27627E", "#3A6550", "#983D50", "#68594C"],
    },
    dark: {
      bg: "#1A1A1F",
      lines: ["#9286DA", "#F4A62A", "#AC81BE", "#F0D9B5", "#79ADC7", "#8EB093", "#D57C87", "#A59583"],
      plates: ["#CAC4ED", "#F8CA7F", "#D6C1DF", "#F1DBB8", "#B5D2E0", "#C0D3C3", "#EABEC3", "#D1C9C0"],
    },
  },
  {
    id: "slate-pink",
    name: "Slate & Pink",
    note: "Cool slate and vivid pink, steel production",
    light: {
      bg: "#F5F7FA",
      lines: ["#566B89", "#B62D75", "#717C87", "#334155", "#237881", "#695D9D", "#93692F", "#4D7750"],
      plates: ["#495B74", "#9E2766", "#545C64", "#334155", "#1D626A", "#5C528A", "#765426", "#3F6242"],
    },
    dark: {
      bg: "#121212",
      lines: ["#7D9ABD", "#EF62A7", "#F1F5F9", "#9194A0", "#73B5BB", "#9C97DB", "#CFA46D", "#91B58B"],
      plates: ["#C1CFDF", "#F8B7D7", "#F1F5F9", "#CBCDD2", "#B6D9DC", "#CECCED", "#E6CFB2", "#C5D8C2"],
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
