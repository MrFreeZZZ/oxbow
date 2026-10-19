import { focusedView, segmentsAt } from "../src/lib/graph.ts";
import type { History, HistoryRow, RowLayout } from "../src/lib/types.ts";
import assert from "node:assert/strict";

const layout = (column: number, color: number): RowLayout => ({ column, color, forkColors: [], mergeColors: [], segments: [], width: column, branch: null });
const row = (id: string, color: number): HistoryRow =>
  ({ id, summary: id, parents: [], labels: [{ name: id, kind: "local", color, head: false }], graph: layout(1, color) }) as unknown as HistoryRow;

// Current Branch Only: the rows it shows, each with its own graph, and its own straight lines.
{
  const history = {
    rows: [row("a", 3), row("b", 5), row("c", 0)],
    runs: [{ column: 1, color: 3, dashed: false, thick: false, first: 0, last: 2 }],
    leadIns: [{ column: 1, row: 1 }],
    focus: {
      rows: [0, 2],
      graph: [layout(0, 6), layout(0, 0)],
      runs: [{ column: 0, color: 0, dashed: false, thick: true, first: 0, last: 1 }],
      leadIns: [],
    },
  } as unknown as History;
  const view = focusedView(history);
  assert.deepEqual(view.rows.map((r) => [r.id, r.graph.column]), [["a", 0], ["c", 0]]);
  assert.deepEqual(view.leadIns, []);
  assert.deepEqual(segmentsAt(view, 0), [{ from: 0, to: 0, color: 0, dashed: false, thick: true }]);
  // Labels take the color of the line they sit on in the focused graph.
  assert.equal(view.rows[0].labels[0].color, 6);
  assert.equal(history.rows[0].labels[0].color, 3);
  // The same history, the same view; the whole one is left alone.
  assert.equal(focusedView(history), view);
  assert.equal(history.rows[0].graph.column, 1);
}
// No focus: the whole history.
{
  const history = { rows: [row("a", 1)], runs: [], leadIns: [], focus: null } as unknown as History;
  assert.equal(focusedView(history), history);
}
