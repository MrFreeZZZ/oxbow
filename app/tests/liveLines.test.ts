import { addLine, cutLine, HEAD, TAIL, type LiveLine } from "../src/lib/liveLines.ts";
import assert from "node:assert/strict";

const out = (text: string, progress = false): LiveLine => ({ kind: "out", text, progress });

// A progress line is replaced by the next, as in a terminal.
{
  const lines: LiveLine[] = [{ kind: "cmd", text: "git fetch" }];
  addLine(lines, out("Receiving objects: 10%", true), cutLine);
  addLine(lines, out("Receiving objects: 100%", true), cutLine);
  assert.deepEqual(lines.map((l) => l.text), ["git fetch", "Receiving objects: 100%"]);
  addLine(lines, out("Receiving objects: 100%, done."), cutLine);
  addLine(lines, out("From github.com:acme/app"), cutLine);
  assert.deepEqual(lines.map((l) => l.text), ["git fetch", "Receiving objects: 100%, done.", "From github.com:acme/app"]);
}

// A remote that prints without end: the start and the end of the output stay, one line says
// how many went between, and the commands before it are kept whole.
{
  const lines: LiveLine[] = [{ kind: "cmd", text: "git status" }, out("clean"), { kind: "cmd", text: "git fetch" }];
  const count = 100_000;
  for (let i = 0; i < count; i++) addLine(lines, out(`remote: ${i}`), cutLine);
  assert.ok(lines.length <= 3 + HEAD + 1 + 2 * TAIL);
  assert.deepEqual(lines.slice(0, 4).map((l) => l.text), ["git status", "clean", "git fetch", "remote: 0"]);
  assert.equal(lines[3 + HEAD - 1].text, `remote: ${HEAD - 1}`);
  const marker = lines[3 + HEAD];
  const kept = lines.length - 3 - 1;
  assert.equal(marker.cut, count - kept);
  assert.equal(marker.text, `⋯ ${count - kept} more lines ⋯`);
  assert.equal(lines[lines.length - 1].text, `remote: ${count - 1}`);
  assert.equal(lines[3 + HEAD + 1].text, `remote: ${count - (kept - HEAD)}`);
  // The next command's output starts a block of its own.
  addLine(lines, { kind: "cmd", text: "git push" }, cutLine);
  addLine(lines, out("Everything up-to-date"), cutLine);
  assert.equal(lines[lines.length - 1].text, "Everything up-to-date");
}
