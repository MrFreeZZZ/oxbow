import { test } from "node:test";
import assert from "node:assert/strict";
import { defaultTemplate, exactPerson, glabCreate, matchLabels, squashChosen, squashStarts } from "../src/lib/mrForm.ts";

test("the Default template fills a new merge request, whatever its case", () => {
  assert.equal(defaultTemplate([{ name: "Bug", text: "b" }, { name: "default", text: "d" }])?.text, "d");
  assert.equal(defaultTemplate([{ name: "Bug", text: "b" }]), null);
});

test("squashing starts as the project says, and is a choice only when it leaves it to each", () => {
  assert.deepEqual(["always", "default_on", "default_off", "never"].map(squashStarts), [true, true, false, false]);
  assert.deepEqual(["always", "default_on", "default_off", "never"].map(squashChosen), [false, true, true, false]);
});

test("labels are suggested by what is typed, the chosen ones left out, starts first", () => {
  const labels = [{ name: "backend" }, { name: "ui" }, { name: "needs-ui-review" }, { name: "bug" }];
  assert.deepEqual(matchLabels(labels, [], "ui").map((l) => l.name), ["ui", "needs-ui-review"]);
  assert.deepEqual(matchLabels(labels, ["ui"], "ui").map((l) => l.name), ["needs-ui-review"]);
  assert.deepEqual(matchLabels(labels, [], "", 2).map((l) => l.name), ["backend", "ui"]);
});

test("a typed name picks its account, or the only one found", () => {
  const people = [
    { id: 8, username: "boris", name: "Boris Lind" },
    { id: 9, username: "borislav", name: "Borislav K" },
  ];
  assert.equal(exactPerson(people, "@Boris")?.id, 8);
  assert.equal(exactPerson(people, "bor"), null);
  assert.equal(exactPerson([people[1]], "bor")?.id, 9);
  assert.equal(exactPerson(people, "  "), null);
});

test("glab mr create says the same as the form", () => {
  assert.equal(
    glabCreate({ branch: "csv", base: "main", draft: true, reviewers: ["boris"], assignee: "anna", labels: ["backend", "needs review"], removeSourceBranch: true, squash: true }),
    "glab mr create --source-branch csv --target-branch main --draft --reviewer boris --assignee anna --label backend --label 'needs review' --remove-source-branch --squash-before-merge",
  );
  assert.equal(
    glabCreate({ branch: "feature/a", base: "main", draft: false, reviewers: [], assignee: null, labels: [], removeSourceBranch: false, squash: false }),
    "glab mr create --source-branch feature/a --target-branch main",
  );
});
