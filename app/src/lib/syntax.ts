// Syntax colors for code views. highlight.js tokenizes each line on its own: a diff shows pieces of
// a file, so a line can't rely on the ones before it (a comment that started above the hunk shows
// as code). The theme picks the colors through --syn-* variables (see themes.ts).

import hljs from "highlight.js/lib/core";
import bash from "highlight.js/lib/languages/bash";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";
import csharp from "highlight.js/lib/languages/csharp";
import css from "highlight.js/lib/languages/css";
import dockerfile from "highlight.js/lib/languages/dockerfile";
import go from "highlight.js/lib/languages/go";
import ini from "highlight.js/lib/languages/ini";
import java from "highlight.js/lib/languages/java";
import javascript from "highlight.js/lib/languages/javascript";
import json from "highlight.js/lib/languages/json";
import kotlin from "highlight.js/lib/languages/kotlin";
import makefile from "highlight.js/lib/languages/makefile";
import markdown from "highlight.js/lib/languages/markdown";
import objectivec from "highlight.js/lib/languages/objectivec";
import php from "highlight.js/lib/languages/php";
import python from "highlight.js/lib/languages/python";
import ruby from "highlight.js/lib/languages/ruby";
import rust from "highlight.js/lib/languages/rust";
import scss from "highlight.js/lib/languages/scss";
import sql from "highlight.js/lib/languages/sql";
import swift from "highlight.js/lib/languages/swift";
import typescript from "highlight.js/lib/languages/typescript";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";
import type { WordPart } from "./types";

const LANGUAGES = { bash, c, cpp, csharp, css, dockerfile, go, ini, java, javascript, json, kotlin, makefile, markdown, objectivec, php, python, ruby, rust, scss, sql, swift, typescript, xml, yaml };
for (const [name, language] of Object.entries(LANGUAGES)) hljs.registerLanguage(name, language);

const BY_EXTENSION: Record<string, keyof typeof LANGUAGES> = {
  sh: "bash", bash: "bash", zsh: "bash",
  c: "c", h: "c",
  cc: "cpp", cpp: "cpp", cxx: "cpp", hpp: "cpp", hh: "cpp",
  cs: "csharp",
  css: "css", scss: "scss", less: "scss",
  go: "go",
  toml: "ini", ini: "ini", cfg: "ini", conf: "ini",
  java: "java", kt: "kotlin", kts: "kotlin",
  js: "javascript", mjs: "javascript", cjs: "javascript", jsx: "javascript",
  ts: "typescript", mts: "typescript", cts: "typescript", tsx: "typescript",
  json: "json", jsonc: "json",
  md: "markdown", markdown: "markdown",
  m: "objectivec", mm: "objectivec",
  php: "php", py: "python", rb: "ruby", rs: "rust", sql: "sql", swift: "swift",
  html: "xml", htm: "xml", xml: "xml", svg: "xml", svelte: "xml", vue: "xml", plist: "xml",
  yml: "yaml", yaml: "yaml",
};

const BY_NAME: Record<string, keyof typeof LANGUAGES> = {
  dockerfile: "dockerfile",
  makefile: "makefile",
  gnumakefile: "makefile",
  gemfile: "ruby",
  rakefile: "ruby",
};

/** The language of a file, from its name; `null` for plain text. */
export function languageOf(path: string): string | null {
  const name = path.split("/").pop()!.toLowerCase();
  // Own keys only: a file can be called `constructor`, or end in `.tostring`.
  if (Object.hasOwn(BY_NAME, name)) return BY_NAME[name];
  const dot = name.lastIndexOf(".");
  const ext = name.slice(dot + 1);
  return dot > 0 && Object.hasOwn(BY_EXTENSION, ext) ? BY_EXTENSION[ext] : null;
}

/** What a piece of code is, as the theme colors it. */
export type Role = "kw" | "str" | "type" | "fn" | "num" | "com" | null;

/** A run of text with one color, whether it is a changed word of the line, and whether it is
 *  text being searched for. */
export interface Piece {
  text: string;
  role: Role;
  changed: boolean;
  found?: boolean;
  /** Which occurrence of the searched text in the line this piece belongs to. */
  hit?: number;
}

function roleOf(classes: string): Role {
  if (/hljs-(comment|quote|meta)\b/.test(classes)) return "com";
  if (/hljs-(string|regexp|symbol|char)\b/.test(classes)) return "str";
  if (/hljs-(number|literal)\b/.test(classes)) return "num";
  if (/hljs-title\b.*\bfunction_|hljs-function\b/.test(classes)) return "fn";
  if (/hljs-(type|title|class|built_in|attr|selector-tag|tag|name)\b/.test(classes)) return "type";
  if (/hljs-(keyword|selector-class|selector-id|variable language_|operator)\b/.test(classes)) return "kw";
  return null;
}

const cache = new Map<string, { text: string; role: Role }[]>();

/** The line's text in runs of one color. */
function tokens(text: string, language: string): { text: string; role: Role }[] {
  const key = `${language}\u0000${text}`;
  const hit = cache.get(key);
  if (hit) return hit;
  const runs: { text: string; role: Role }[] = [];
  try {
    const html = hljs.highlight(text, { language, ignoreIllegals: true }).value;
    const box = document.createElement("template");
    box.innerHTML = html;
    const walk = (node: Node, role: Role) => {
      for (const child of node.childNodes) {
        if (child.nodeType === Node.TEXT_NODE) {
          const t = child.textContent ?? "";
          if (t) runs.push({ text: t, role });
        } else if (child instanceof Element) {
          walk(child, roleOf(child.className) ?? role);
        }
      }
    };
    walk(box.content, null);
  } catch {
    runs.length = 0;
    runs.push({ text, role: null });
  }
  if (cache.size > 20000) cache.clear();
  cache.set(key, runs);
  return runs;
}

/** The line in pieces to draw: syntax colors from `language`, changed words from `words`, and
 *  `find` (searched code, matched as git's -S does: exact case) marked. */
export function paint(text: string, words: WordPart[] | null, language: string | null, find: string | null = null, matchCase = true): Piece[] {
  const pieces = colored(text, words, language);
  if (!find) return pieces;
  const inside = occurrences(text, find, matchCase);
  return inside.length ? mark(pieces, text, inside) : pieces;
}

/** Where `find` is in `text`: start and end of each occurrence, left to right. */
export function occurrences(text: string, find: string, matchCase = true): [number, number][] {
  const hay = matchCase ? text : text.toLowerCase();
  const needle = matchCase ? find : find.toLowerCase();
  const out: [number, number][] = [];
  if (!needle) return out;
  for (let at = hay.indexOf(needle); at >= 0; at = hay.indexOf(needle, at + needle.length)) out.push([at, at + needle.length]);
  return out;
}

/** Cut the pieces where the occurrences start and end, and flag the ones inside them with the occurrence's number. */
function mark(pieces: Piece[], text: string, inside: [number, number][]): Piece[] {
  const out: Piece[] = [];
  let offset = 0;
  for (const piece of pieces) {
    const end = offset + piece.text.length;
    // Cut points that fall inside this piece.
    const cuts = [offset, ...inside.flat().filter((c) => c > offset && c < end), end];
    for (let i = 0; i + 1 < cuts.length; i++) {
      const [a, b] = [cuts[i], cuts[i + 1]];
      const hit = inside.findIndex(([s, e]) => a >= s && b <= e);
      out.push({ ...piece, text: text.slice(a, b), found: hit >= 0, hit: hit >= 0 ? hit : undefined });
    }
    offset = end;
  }
  return out;
}

function colored(text: string, words: WordPart[] | null, language: string | null): Piece[] {
  const runs = language ? tokens(text, language) : [{ text, role: null as Role }];
  if (!words) return runs.map((run) => ({ ...run, changed: false }));
  // Cut the colored runs where changed words start and end.
  const pieces: Piece[] = [];
  let r = 0;
  let used = 0;
  for (const word of words) {
    let left = word.text.length;
    while (left > 0 && r < runs.length) {
      const run = runs[r];
      const take = Math.min(left, run.text.length - used);
      pieces.push({ text: run.text.slice(used, used + take), role: run.role, changed: word.changed });
      left -= take;
      used += take;
      if (used === run.text.length) {
        r += 1;
        used = 0;
      }
    }
    // The words cover more than the runs (they should not): draw the rest uncolored.
    if (left > 0) pieces.push({ text: word.text.slice(word.text.length - left), role: null, changed: word.changed });
  }
  return pieces;
}
