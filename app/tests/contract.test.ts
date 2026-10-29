// The app and its shell agree: every command the shell lists has its wrapper in api.ts and every
// wrapper calls a command the shell has; and the JSON the core sends (its shapes in
// contract.json, written by crates/oxbow-core/tests/contract.rs) has the fields types.ts says.
import assert from "node:assert/strict";
import fs from "node:fs";

const read = (path: string) => fs.readFileSync(new URL(path, import.meta.url), "utf8");

// Every command in generate_handler! has a wrapper, and every wrapper a command.
{
  const main = read("../src-tauri/src/main.rs");
  const list = main.slice(main.indexOf("generate_handler!["));
  const names = list.slice("generate_handler![".length, list.indexOf("]")).match(/[a-z_0-9]+(::[a-z_0-9]+)*/g) ?? [];
  const commands = new Set(names.map((name) => name.split("::").pop()!));
  const api = read("../src/lib/api.ts");
  const wrapped = new Set([...api.matchAll(/invoke(?:<[\s\S]*?>)?\(\s*"([a-z_0-9]+)"/g)].map((m) => m[1]));
  assert.ok(commands.size > 100, `only ${commands.size} commands found in main.rs`);
  assert.deepEqual([...commands].filter((c) => !wrapped.has(c)), [], "commands without a wrapper in api.ts");
  assert.deepEqual([...wrapped].filter((c) => !commands.has(c)), [], "wrappers of commands main.rs doesn't list");
}

/** The fields of each interface of types.ts, those of the one it extends included, and whether
 *  each may be left out (`name?:`). */
function interfaces(source: string): Map<string, Map<string, boolean>> {
  const out = new Map<string, Map<string, boolean>>();
  const bases = new Map<string, string>();
  for (const m of source.matchAll(/^export interface (\w+)(?: extends (\w+))? \{\n([\s\S]*?)^\}/gm)) {
    const fields = new Map<string, boolean>();
    for (const f of m[3].matchAll(/^ {2}(\w+)(\?)?:/gm)) fields.set(f[1], !!f[2]);
    out.set(m[1], fields);
    if (m[2]) bases.set(m[1], m[2]);
  }
  for (const [name, base] of bases) for (const [field, optional] of out.get(base) ?? []) out.get(name)!.set(field, optional);
  return out;
}

// Each shape has the fields of its interface: none it doesn't declare, and every one it doesn't
// mark as one that may be left out.
{
  const shapes = JSON.parse(read("contract.json")) as Record<string, string[]>;
  const types = interfaces(read("../src/lib/types.ts"));
  assert.ok(Object.keys(shapes).length >= 20, "too few shapes in contract.json");
  for (const [name, fields] of Object.entries(shapes)) {
    const declared = types.get(name);
    assert.ok(declared, `types.ts has no interface ${name}`);
    const unknown = fields.filter((f) => !declared.has(f));
    assert.deepEqual(unknown, [], `${name}: the core sends fields types.ts doesn't declare`);
    const missing = [...declared].filter(([f, optional]) => !optional && !fields.includes(f)).map(([f]) => f);
    assert.deepEqual(missing, [], `${name}: types.ts expects fields the core doesn't send`);
  }
}
