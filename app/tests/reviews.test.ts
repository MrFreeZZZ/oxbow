import { listFor } from "../src/lib/reviews.ts";
import type { PullSummary } from "../src/lib/types.ts";
import assert from "node:assert/strict";

const pull = (number: number, author: string, extra: Partial<PullSummary> = {}): PullSummary => ({
  number,
  title: `#${number}`,
  state: "open",
  draft: false,
  head: `b${number}`,
  headSha: "",
  headOwner: null,
  base: "main",
  htmlUrl: "",
  body: "",
  author: { login: author, avatarUrl: "" },
  created: 0,
  updated: 0,
  nodeId: "",
  requested: [],
  ...extra,
});

const titles = (sections: ReturnType<typeof listFor>) => sections.map((s) => [s.title, s.pulls.map((p) => p.number)]);

// Asked for a review comes first; a draft asking doesn't wait for anyone yet.
{
  const pulls = [
    pull(424, "kirill", { draft: true, requested: ["alexander"] }),
    pull(423, "maria", { requested: ["alexander", "kirill"] }),
    pull(421, "dmitry", { requested: ["alexander"] }),
    pull(420, "alexander"),
    pull(418, "maria", { state: "merged" }),
  ];
  assert.deepEqual(titles(listFor(pulls, "alexander", false)), [
    ["Waiting for your review", [423, 421]],
    ["Yours", [420]],
    ["Others", [424]],
  ]);
  assert.deepEqual(titles(listFor(pulls, "alexander", true)), [["Closed and merged", [418]]]);
  // Signed out: nothing is anyone's.
  assert.deepEqual(titles(listFor(pulls, null, false))[2], ["Others", [424, 423, 421, 420]]);
}
