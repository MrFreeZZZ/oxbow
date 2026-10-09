// settings.json as its editor shows it: colored pieces of text, and what is wrong where. The
// text is read the way JSON.parse reads it, but a mistake gets a place to underline instead of
// only a message, and each setting's value is checked against what the setting can hold.

export type TokenKind = "key" | "str" | "num" | "lit" | "punct" | "com" | "space" | "bad";

export interface Token {
  kind: TokenKind;
  from: number;
  to: number;
  /** Why the token itself can't be JSON, e.g. a string without its closing quote. */
  wrong?: string;
}

export interface Problem {
  from: number;
  to: number;
  message: string;
  /** An error keeps the file from being saved; a warning doesn't. */
  level: "error" | "warn";
}

/** A run of text with one color and at most one problem under it. */
export interface Segment {
  text: string;
  kind: TokenKind;
  /** Index into the problems. */
  problem: number | null;
}

export interface Checked {
  tokens: Token[];
  problems: Problem[];
}

const SPACE = /\s+/y;
// JSON strings can't hold raw control characters (U+0000 to U+001F), a Tab included.
const STRING = /"(?:[^"\\\u0000-\u001f]|\\["\\/bfnrt]|\\u[0-9a-fA-F]{4})*"/y;
const CONTROL = /[\u0000-\u001f]/;
const LOOSE_STRING = /"(?:[^"\\\n]|\\.)*"?/y;
const NUMBER = /-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/y;
const WORD = /[A-Za-z_$][\w$]*/y;

function at(pattern: RegExp, text: string, from: number): number {
  pattern.lastIndex = from;
  return pattern.test(text) ? pattern.lastIndex : from;
}

export function tokenize(text: string): Token[] {
  const tokens: Token[] = [];
  let i = 0;
  while (i < text.length) {
    const c = text[i];
    let to = at(SPACE, text, i);
    if (to > i) {
      tokens.push({ kind: "space", from: i, to });
    } else if (c === '"') {
      to = at(STRING, text, i);
      if (to > i) tokens.push({ kind: "str", from: i, to });
      else {
        to = at(LOOSE_STRING, text, i);
        const closed = to - i > 1 && text[to - 1] === '"';
        const wrong = !closed
          ? "Missing the closing quote"
          : CONTROL.test(text.slice(i, to))
            ? "A Tab or other control character must be written as an escape, e.g. \\t"
            : "Not a valid escape, e.g. \\\\ for a backslash";
        tokens.push({ kind: "str", from: i, to, wrong });
      }
    } else if (c === "/" && (text[i + 1] === "/" || text[i + 1] === "*")) {
      const end = text[i + 1] === "/" ? text.indexOf("\n", i) : text.indexOf("*/", i + 2);
      to = end < 0 ? text.length : text[i + 1] === "/" ? end : end + 2;
      tokens.push({ kind: "com", from: i, to, wrong: "Comments are not allowed in settings.json" });
    } else if (c === "-" || (c >= "0" && c <= "9")) {
      to = at(NUMBER, text, i);
      if (to > i) tokens.push({ kind: "num", from: i, to });
      else tokens.push({ kind: "bad", from: i, to: (to = i + 1) });
    } else if ((to = at(WORD, text, i)) > i) {
      const word = text.slice(i, to);
      tokens.push(["true", "false", "null"].includes(word) ? { kind: "lit", from: i, to } : { kind: "bad", from: i, to, wrong: `Unknown word “${word}”: text needs quotes` });
    } else if ("{}[]:,".includes(c)) {
      tokens.push({ kind: "punct", from: i, to: (to = i + 1) });
    } else {
      tokens.push({ kind: "bad", from: i, to: (to = i + 1) });
    }
    i = to;
  }
  return tokens;
}

const STOP = Symbol("stop");

/** Read settings.json; `known` says whether Oxbow has a setting, `problemOf` what is wrong with
 *  its value, if anything. */
export function check(text: string, known: (key: string) => boolean, problemOf: (key: string, value: unknown) => string | null): Checked {
  const tokens = tokenize(text);
  const problems: Problem[] = [];
  const error = (token: Token, message: string) => problems.push({ from: token.from, to: token.to, message, level: "error" });
  for (const token of tokens) if (token.kind === "com") error(token, token.wrong!);
  const meaningful = tokens.filter((token) => token.kind !== "space" && token.kind !== "com");
  let i = 0;
  const peek = (): Token | undefined => meaningful[i];
  const word = (token: Token) => text.slice(token.from, token.to);

  /** Stop reading at the first mistake, as JSON.parse does: what follows can't be trusted. */
  function fail(token: Token | undefined, message: string): never {
    // Past the end: underline what came last.
    const last = token ?? meaningful[meaningful.length - 1];
    if (last) error(last, message);
    else problems.push({ from: 0, to: 0, message, level: "error" });
    throw STOP;
  }

  function expect(punct: string, message: string): Token {
    const token = meaningful[i];
    if (token?.kind !== "punct" || word(token) !== punct) fail(token, token ? message : `${message}, but the file ends here`);
    i += 1;
    return token;
  }

  /** JSON.parse of one token; what it can't read is a problem under the token, not a throw. */
  function parse(token: Token): unknown {
    try {
      return JSON.parse(word(token));
    } catch (err) {
      fail(token, err instanceof Error ? err.message : String(err));
    }
  }

  function string(token: Token): string {
    if (token.wrong) fail(token, token.wrong);
    return parse(token) as string;
  }

  function value(): unknown {
    const token = meaningful[i];
    if (!token) fail(undefined, "Expected a value, but the file ends here");
    if (token.kind === "punct" && word(token) === "{") return object(null);
    if (token.kind === "punct" && word(token) === "[") return array();
    i += 1;
    if (token.kind === "str") return string(token);
    if (token.kind === "num" || token.kind === "lit") return parse(token);
    fail(token, token.wrong ?? `Expected a value, not “${word(token)}”`);
  }

  function array(): unknown[] {
    expect("[", "Expected [");
    const items: unknown[] = [];
    if (peek() && word(peek()!) === "]") {
      i += 1;
      return items;
    }
    for (;;) {
      items.push(value());
      const comma = peek();
      if (comma && word(comma) === ",") {
        i += 1;
        if (peek() && word(peek()!) === "]") fail(comma, "A comma before ] is not allowed in JSON");
        continue;
      }
      expect("]", "Expected , or ] after the value");
      return items;
    }
  }

  type Entry = { key: string; keyToken: Token; from: number; to: number; value: unknown };

  /** An object; the top one's settings go into `entries`. */
  function object(entries: Entry[] | null): Record<string, unknown> {
    expect("{", "settings.json must be one object: { \"oxbow.…\": … }");
    const out: Record<string, unknown> = {};
    if (peek() && word(peek()!) === "}") {
      i += 1;
      return out;
    }
    for (;;) {
      const keyToken = meaningful[i];
      if (!keyToken || keyToken.kind !== "str") fail(keyToken, keyToken ? `Expected a setting name in quotes, not “${word(keyToken)}”` : "Expected a setting name, but the file ends here");
      keyToken.kind = "key";
      i += 1;
      const key = string(keyToken);
      expect(":", `Expected : after "${key}"`);
      const start = meaningful[i];
      const item = value();
      out[key] = item;
      if (entries && start) entries.push({ key, keyToken, from: start.from, to: meaningful[i - 1].to, value: item });
      const comma = peek();
      if (comma && word(comma) === ",") {
        i += 1;
        if (peek() && word(peek()!) === "}") fail(comma, "A comma before } is not allowed in JSON");
        continue;
      }
      expect("}", "Expected , or } after the value");
      return out;
    }
  }

  const entries: Entry[] = [];
  try {
    object(entries);
    if (i < meaningful.length) fail(meaningful[i], "Nothing may follow the closing }");
  } catch (err) {
    // Anything else is a mistake of the reader: still a problem to show, never a broken editor.
    if (err !== STOP) problems.push({ from: 0, to: 0, message: String(err), level: "error" });
  }

  // Each setting against what it can hold.
  const seen = new Map<string, Entry>();
  for (const entry of entries) {
    const earlier = seen.get(entry.key);
    if (earlier) problems.push({ from: earlier.keyToken.from, to: earlier.keyToken.to, message: "Set again further down, which wins", level: "warn" });
    seen.set(entry.key, entry);
    if (!known(entry.key)) {
      problems.push({ from: entry.keyToken.from, to: entry.keyToken.to, message: `Oxbow has no setting “${entry.key}”`, level: "warn" });
      continue;
    }
    const problem = problemOf(entry.key, entry.value);
    if (problem) problems.push({ from: entry.from, to: entry.to, message: problem, level: "error" });
  }
  problems.sort((a, b) => a.from - b.from);
  return { tokens, problems };
}

/** The text cut into runs of one color and one problem, for drawing under the editor. */
export function segments(text: string, { tokens, problems }: Checked): Segment[] {
  const cuts = new Set<number>([0, text.length]);
  for (const token of tokens) cuts.add(token.from).add(token.to);
  for (const problem of problems) cuts.add(problem.from).add(problem.to);
  const points = [...cuts].sort((a, b) => a - b);
  const out: Segment[] = [];
  let t = 0;
  for (let k = 0; k + 1 < points.length; k++) {
    const [from, to] = [points[k], points[k + 1]];
    if (from === to) continue;
    while (t < tokens.length && tokens[t].to <= from) t += 1;
    const kind = tokens[t]?.kind ?? "space";
    // An error wins over a warning on the same text.
    let problem: number | null = null;
    problems.forEach((p, index) => {
      if (p.from <= from && p.to >= to && (problem === null || (p.level === "error" && problems[problem].level === "warn"))) problem = index;
    });
    const last = out[out.length - 1];
    if (last && last.kind === kind && last.problem === problem) last.text += text.slice(from, to);
    else out.push({ text: text.slice(from, to), kind, problem });
  }
  return out;
}

/** The line a position is on, counting from 1. */
export function lineOf(text: string, at: number): number {
  let line = 1;
  for (let i = 0; i < at && i < text.length; i++) if (text[i] === "\n") line += 1;
  return line;
}
