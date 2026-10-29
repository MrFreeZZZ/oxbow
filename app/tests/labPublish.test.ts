import { labPublishCommands, labPublishNote, validProjectName, visibilityText } from "../src/lib/labPublish.ts";
import type { GitLabPublish } from "../src/lib/types.ts";
import assert from "node:assert/strict";
import { test } from "node:test";

const publish: GitLabPublish = { namespaceId: 31, namespace: "acme/platform", name: "billing", visibility: "internal", description: " Invoices ", branch: "main" };

test("the sheet shows the request and commands as the backend runs them", () => {
  assert.deepEqual(labPublishCommands(publish, "https://git.corp", true), [
    `curl -X POST -H "PRIVATE-TOKEN: $GITLAB_TOKEN" https://git.corp/api/v4/projects -d '{"description":"Invoices","name":"billing","namespace_id":31,"path":"billing","visibility":"internal"}'`,
    "git remote add origin https://git.corp/acme/platform/billing.git",
    "git push -u origin main",
  ]);
  assert.equal(labPublishCommands({ ...publish, description: "" }, "https://git.corp", false)[2], "git push --no-verify -u origin main");
  assert.equal(labPublishNote(publish), "glab repo create acme/platform/billing --internal makes the same project");
});

test("GitLab's rules for a project's path", () => {
  assert.ok(validProjectName("billing.api_2"));
  assert.ok(!validProjectName("-billing"));
  assert.ok(!validProjectName("billing.git"));
  assert.ok(!validProjectName("my project"));
  assert.ok(!validProjectName(".."));
});

test("who can see it", () => {
  assert.equal(visibilityText("private", "anna", true, "git.corp"), "Private: only you, and people you add, can see it.");
  assert.equal(visibilityText("private", "acme", false, "git.corp"), "Private: members of acme can see it.");
  assert.equal(visibilityText("internal", "acme", false, "git.corp"), "Internal: anyone signed in to git.corp can see it.");
});
