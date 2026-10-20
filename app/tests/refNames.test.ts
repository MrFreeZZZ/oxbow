import { sharedLeads } from "../src/lib/refNames.ts";
import assert from "node:assert/strict";

// Names with one long start give it way, cut after its last separator.
{
  const leads = sharedLeads(["claude/project-thread-rg2vy4", "claude/project-thread-97s3lf", "main", "release/0.9.2"]);
  assert.equal(leads.get("claude/project-thread-rg2vy4"), "claude/project-thread-".length);
  assert.equal(leads.get("claude/project-thread-97s3lf"), "claude/project-thread-".length);
  assert.equal(leads.get("main"), 0);
  assert.equal(leads.get("release/0.9.2"), 0);
}

// Only the part shared with the nearest name, and never the whole name.
{
  const leads = sharedLeads(["feature/login", "feature/login-form", "fix/a"]);
  assert.equal(leads.get("feature/login"), "feature/".length);
  // The shared "feature/login" ends inside a word: only "feature/" goes.
  assert.equal(leads.get("feature/login-form"), "feature/".length);
  // "f" alone is too short to be worth hiding.
  assert.equal(leads.get("fix/a"), 0);
}
