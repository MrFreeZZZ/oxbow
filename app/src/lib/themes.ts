// Code themes: Oxbow's own and the ten most installed color themes of the VS Code Marketplace
// (by installs, checked 2026-10-01). A theme colors code views only; the window keeps the macOS
// look. Colors come from the Themes design.

/** Background, text, keyword, string, type, function, number, comment, added, removed. */
export interface Palette {
  bg: string;
  fg: string;
  kw: string;
  str: string;
  type: string;
  fn: string;
  num: string;
  com: string;
  add: string;
  del: string;
}

export interface Theme {
  id: string;
  name: string;
  /** Place by installs; none for Oxbow's own. */
  rank: number | null;
  installs: string | null;
  light?: Palette;
  dark?: Palette;
}

const p = (bg: string, fg: string, kw: string, str: string, type: string, fn: string, num: string, com: string, add: string, del: string): Palette => ({
  bg, fg, kw, str, type, fn, num, com, add, del,
});

export const THEMES: Theme[] = [
  {
    id: "oxbow", name: "Oxbow", rank: null, installs: null,
    light: p("#FFFFFF", "#24292F", "#8A5DB0", "#4F8A5E", "#A0663A", "#3E6A9E", "#B0587A", "#8C8C92", "#2E9E4F", "#D73A3A"),
    dark: p("#1E1E21", "#E6EDF3", "#C4A0DE", "#8FCB9F", "#E0B184", "#94B8E0", "#E69AB8", "#8B8B92", "#3FB950", "#F85149"),
  },
  {
    id: "github", name: "GitHub", rank: 1, installs: "18.8M",
    light: p("#FFFFFF", "#1F2328", "#CF222E", "#0A3069", "#953800", "#8250DF", "#0550AE", "#6E7781", "#1A7F37", "#CF222E"),
    dark: p("#0D1117", "#E6EDF3", "#FF7B72", "#A5D6FF", "#FFA657", "#D2A8FF", "#79C0FF", "#8B949E", "#3FB950", "#F85149"),
  },
  { id: "onedark", name: "One Dark Pro", rank: 2, installs: "12.7M", dark: p("#282C34", "#ABB2BF", "#C678DD", "#98C379", "#E5C07B", "#61AFEF", "#D19A66", "#7F848E", "#98C379", "#E06C75") },
  { id: "dracula", name: "Dracula", rank: 3, installs: "10.9M", dark: p("#282A36", "#F8F8F2", "#FF79C6", "#F1FA8C", "#8BE9FD", "#50FA7B", "#BD93F9", "#6272A4", "#50FA7B", "#FF5555") },
  {
    id: "ayu", name: "Ayu", rank: 4, installs: "4.2M",
    light: p("#FCFCFC", "#5C6166", "#FA8D3E", "#86B300", "#399EE6", "#F2AE49", "#A37ACC", "#ADAEB1", "#6CBF43", "#E65050"),
    dark: p("#0B0E14", "#BFBDB6", "#FF8F40", "#AAD94C", "#59C2FF", "#FFB454", "#D2A6FF", "#636A72", "#7FD962", "#F26D78"),
  },
  {
    id: "monokai", name: "Monokai Pro", rank: 5, installs: "4.1M",
    light: p("#FAF4F2", "#29242A", "#E14775", "#CC7A0A", "#1C8CA8", "#269D69", "#7058BE", "#A59FA0", "#269D69", "#E14775"),
    dark: p("#2D2A2E", "#FCFCFA", "#FF6188", "#FFD866", "#78DCE8", "#A9DC76", "#AB9DF2", "#727072", "#A9DC76", "#FF6188"),
  },
  {
    id: "nightowl", name: "Night Owl", rank: 6, installs: "3.5M",
    light: p("#FBFBFB", "#403F53", "#994CC3", "#C96765", "#0C969B", "#4876D6", "#AA0982", "#989FB1", "#08916A", "#DE3D3B"),
    dark: p("#011627", "#D6DEEB", "#C792EA", "#ECC48D", "#FFCB8B", "#82AAFF", "#F78C6C", "#637777", "#ADDB67", "#EF5350"),
  },
  { id: "onemonokai", name: "One Monokai", rank: 7, installs: "3.0M", dark: p("#282C34", "#ABB2BF", "#F92672", "#E6DB74", "#66D9EF", "#A6E22E", "#AE81FF", "#676F7D", "#A6E22E", "#F92672") },
  {
    id: "tokyo", name: "Tokyo Night", rank: 8, installs: "2.9M",
    light: p("#D5D6DB", "#343B58", "#5A4A78", "#485E30", "#166775", "#34548A", "#965027", "#9699A3", "#485E30", "#8C4351"),
    dark: p("#1A1B26", "#A9B1D6", "#BB9AF7", "#9ECE6A", "#0DB9D7", "#7AA2F7", "#FF9E64", "#565F89", "#9ECE6A", "#F7768E"),
  },
  { id: "purple", name: "Shades of Purple", rank: 9, installs: "2.3M", dark: p("#2D2B55", "#FFFFFF", "#FF9D00", "#A5FF90", "#FAD000", "#FAD000", "#FF628C", "#B362FF", "#3AD900", "#EC3A37") },
  { id: "andromeda", name: "Andromeda", rank: 10, installs: "1.7M", dark: p("#23262E", "#D5CED9", "#C74DED", "#96E072", "#FFE66D", "#FFE66D", "#F39C12", "#A0A1A7", "#96E072", "#EE5D43") },
];

export function themeById(id: string): Theme {
  return THEMES.find((t) => t.id === id) ?? THEMES[0];
}

/** The palette a theme uses: `variant` picks light or dark, `auto` follows the window. A
 *  dark-only theme is dark in both. */
export function paletteOf(theme: Theme, variant: "auto" | "light" | "dark", look: "light" | "dark"): { palette: Palette; dark: boolean } {
  const want = variant === "auto" ? look : variant;
  if (want === "light" && theme.light) return { palette: theme.light, dark: false };
  if (theme.dark) return { palette: theme.dark, dark: true };
  return { palette: theme.light!, dark: false };
}

function rgba(hex: string, alpha: number): string {
  const n = parseInt(hex.slice(1), 16);
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}

/** CSS variables for code views; an empty value means the app's own. Oxbow's theme in the
 *  window's own look keeps the window's background and the approved diff tints; anything else
 *  brings its own background. */
export function themeVariables(theme: Theme, palette: Palette, dark: boolean, look: "light" | "dark"): Record<string, string> {
  const own = theme.id === "oxbow" && dark === (look === "dark");
  const tint = dark ? { line: 0.13, word: 0.3 } : { line: 0.09, word: 0.22 };
  const vars: Record<string, string> = {
    "--syn-kw": palette.kw,
    "--syn-str": palette.str,
    "--syn-type": palette.type,
    "--syn-fn": palette.fn,
    "--syn-num": palette.num,
    "--syn-com": palette.com,
  };
  if (own) {
    for (const name of ["--code-bg", "--code-fg", "--code-dim", "--add", "--del", "--add-word", "--del-word", "--fold-bg"]) vars[name] = "";
    return vars;
  }
  return {
    ...vars,
    "--code-bg": palette.bg,
    "--code-fg": palette.fg,
    "--code-dim": palette.com,
    "--add": rgba(palette.add, tint.line + 0.04),
    "--del": rgba(palette.del, tint.line + 0.04),
    "--add-word": rgba(palette.add, tint.word + 0.08),
    "--del-word": rgba(palette.del, tint.word + 0.08),
    "--fold-bg": rgba(palette.fg, 0.08),
  };
}
