import { mergeRequestLink, mergeRequestTitle, newMergeRequestUrl, tokenExpiry } from "../src/lib/gitlab.ts";
import type { GitLabProject } from "../src/lib/types.ts";
import assert from "node:assert/strict";

const project: GitLabProject = { remote: "origin", host: "gitlab.com", path: "acme/api", webUrl: "https://gitlab.com/acme/api" };

// GitLab answers the push with the merge request's link, beside git's own words.
{
  const output = [
    "remote: ",
    "remote: View merge request for csv:",
    "remote:   https://gitlab.com/acme/api/-/merge_requests/12",
    "remote: ",
    "To gitlab.com:acme/api.git",
    " * [new branch]      csv -> csv",
    "branch 'csv' set up to track 'origin/csv'.",
  ].join("\n");
  assert.deepEqual(mergeRequestLink(output, project), { url: "https://gitlab.com/acme/api/-/merge_requests/12", number: 12 });
}

// The link to create one, printed when the options didn't come through, isn't one; nor is
// another project's.
{
  const output = "remote: To create a merge request for csv, visit:\nremote:   https://gitlab.com/acme/api/-/merge_requests/new?merge_request%5Bsource_branch%5D=csv";
  assert.equal(mergeRequestLink(output, project), null);
  assert.equal(mergeRequestLink("remote:   https://gitlab.com/acme/web/-/merge_requests/3", project), null);
  assert.equal(mergeRequestLink("Everything up-to-date", project), null);
}

// The New merge request page with both branches and the title picked.
{
  assert.equal(
    newMergeRequestUrl(project, "feature/csv", "main", " Export CSV & more "),
    "https://gitlab.com/acme/api/-/merge_requests/new?merge_request%5Bsource_branch%5D=feature%2Fcsv&merge_request%5Btarget_branch%5D=main&merge_request%5Btitle%5D=Export%20CSV%20%26%20more",
  );
  assert.equal(newMergeRequestUrl(project, "csv", "main", ""), "https://gitlab.com/acme/api/-/merge_requests/new?merge_request%5Bsource_branch%5D=csv&merge_request%5Btarget_branch%5D=main");
}

// The title starts from the newest commit, or the branch's name without one.
{
  assert.equal(mergeRequestTitle("Export CSV", "csv"), "Export CSV");
  assert.equal(mergeRequestTitle(undefined, "csv"), "csv");
}

// A signed-in token's last days: Settings warns two weeks ahead.
{
  const now = new Date(Date.UTC(2026, 9, 28, 15));
  assert.equal(tokenExpiry(null, now), null);
  assert.deepEqual(tokenExpiry("2027-01-15", now), { text: "token until 15 Jan 2027", soon: false, expired: false });
  assert.deepEqual(tokenExpiry("2026-11-05", now), { text: "token until 5 Nov 2026", soon: true, expired: false });
  assert.deepEqual(tokenExpiry("2026-10-28", now), { text: "token expired on 28 Oct 2026", soon: true, expired: true });
}
