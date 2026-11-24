import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import {
  applyDetailsChrome,
  CHANGES_EXPANDED_KEY,
  DETAILS_HIDDEN_KEY,
  DETAILS_PANEL_MIN,
  DETAILS_WIDTH_KEY,
  detailsChromeEqual,
  type DetailsChrome,
} from "../src/lib/detailsChrome.ts";

const fresh = (): DetailsChrome => ({ panelWidth: null, detailsHidden: false, changesExpanded: false });

test("one details panel is what every open repository tab reads", () => {
  let shared = fresh();
  const tabA = () => shared;
  const tabB = () => shared;
  shared = applyDetailsChrome(shared, DETAILS_WIDTH_KEY, 480);
  shared = applyDetailsChrome(shared, DETAILS_HIDDEN_KEY, true);
  assert.equal(tabA().panelWidth, 480);
  assert.equal(tabB().panelWidth, 480);
  assert.equal(tabA().detailsHidden, true);
  shared = applyDetailsChrome(shared, CHANGES_EXPANDED_KEY, true);
  assert.equal(tabB().changesExpanded, true);
  assert.equal(tabA().panelWidth, 480);
});

test("a later edit keeps the width a stale tab used to remember", () => {
  const saved = applyDetailsChrome(fresh(), DETAILS_WIDTH_KEY, 200);
  const fromDisk = applyDetailsChrome(fresh(), DETAILS_WIDTH_KEY, 510);
  assert.ok(saved.panelWidth !== null && saved.panelWidth >= DETAILS_PANEL_MIN);
  assert.equal(fromDisk.panelWidth, 510);
  assert.equal(applyDetailsChrome(fromDisk, "oxbow.fetch.auto", false).panelWidth, 510);
  assert.equal(applyDetailsChrome(fromDisk, DETAILS_WIDTH_KEY, null).panelWidth, null);
  assert.equal(applyDetailsChrome(fromDisk, DETAILS_HIDDEN_KEY, null).detailsHidden, false);
  assert.ok(!detailsChromeEqual(saved, fromDisk));
});

test("the repository window keeps a single details panel", () => {
  const app = fs.readFileSync(new URL("../src/App.svelte", import.meta.url), "utf8");
  assert.match(app, /detailsChrome\.panelWidth/);
  assert.match(app, /detailsChrome\.detailsHidden/);
  assert.match(app, /detailsChrome\.changesExpanded/);
  assert.doesNotMatch(app, /let panelWidth = \$state/);
  assert.doesNotMatch(app, /let detailsHidden = \$state/);
  assert.doesNotMatch(app, /let changesExpanded = \$state/);
});
