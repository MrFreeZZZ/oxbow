import assert from "node:assert/strict";
import test from "node:test";
import { channelDescription, channelUnavailable, installedVersion, selectedChannel, updateChannels, updateNotice } from "../src/lib/updates.ts";
import type { UpdateInfo, UpdateStatus } from "../src/lib/types.ts";

const info: UpdateInfo = { version: "0.1.550", commit: "abcdef0", buildChannel: "preview", channel: "preview", platform: "darwin-aarch64", canInstall: true, availableChannels: ["release", "preview"] };
const status = (state: UpdateStatus): UpdateStatus => state;

test("channel choices follow the build and preserve an unsupported saved choice", () => {
  assert.equal(selectedChannel(info, "auto"), "preview");
  assert.equal(selectedChannel(info, "release"), "release");
  const intel: UpdateInfo = { ...info, platform: "darwin-x86_64", availableChannels: ["release"] };
  assert.deepEqual(updateChannels(intel, "auto").map((o) => o.value), ["auto", "release"]);
  assert.deepEqual(updateChannels(intel, "preview").map((o) => o.value), ["auto", "release", "preview"]);
  assert.equal(channelUnavailable(intel, "auto"), true);
  assert.match(channelDescription(intel, "auto"), /Select Release/);
  assert.equal(channelUnavailable(intel, "release"), false);
});

test("old results cannot announce an update for a newly selected channel", () => {
  const old = status({ channel: "preview", state: "available", release: { version: "0.1.551", notes: "Notes" }, checkedAt: 1 });
  const view = updateNotice(info, "release", old, null);
  assert.equal(view.title, "No update check yet");
  assert.match(view.message, /Release/);
  const finishing = updateNotice(info, "release", { channel: "preview", state: "checking", checkedAt: null }, null);
  assert.match(finishing.message, /Finishing/);
});

test("metadata, errors and unsupported channels take precedence over cached update results", () => {
  const old = status({ channel: "preview", state: "available", release: { version: "0.1.551", notes: null }, checkedAt: 1 });
  assert.equal(updateNotice(info, "auto", old, "Cannot read settings").error, true);
  assert.equal(updateNotice(null, "auto", null, null).title, "Reading update settings…");
  assert.equal(updateNotice({ ...info, availableChannels: [] }, "auto", old, null).title, "Channel unavailable on this device");
});

test("manual checks distinguish available, no newer build, checking and failure", () => {
  assert.match(installedVersion(info), /Oxbow 0.1.550 · Preview · Apple Silicon/);
  const make = (value: Partial<UpdateStatus>) => ({ channel: "preview", checkedAt: null, ...value }) as UpdateStatus;
  assert.equal(updateNotice(info, "auto", make({ state: "checking" }), null).title, "Checking for updates…");
  assert.equal(updateNotice(info, "auto", make({ state: "current" }), null).title, "No newer update available");
  assert.equal(updateNotice(info, "auto", make({ state: "failed", message: "No connection" }), null).message, "No connection");
  assert.equal(updateNotice(info, "auto", make({ state: "available", release: { version: "0.1.551", notes: null } }), null).title, "Oxbow 0.1.551 is available");
});

test("download progress handles missing lengths and never promises more than 100 percent", async () => {
  const { downloadProgress } = await import("../src/lib/updates.ts");
  assert.deepEqual(downloadProgress(0, null), { percent: null, label: "0.0 MB downloaded" });
  assert.equal(downloadProgress(1024, 0).percent, null);
  assert.equal(downloadProgress(15 * 1024 * 1024, 20 * 1024 * 1024).percent, 75);
  assert.equal(downloadProgress(11, 10).percent, 100);
  assert.equal(downloadProgress(-1, 10).percent, 0);
});

test("only verified bytes are presented as ready; failures explain the retry step", () => {
  const release = { version: "0.1.551", notes: null };
  const make = (state: UpdateStatus["state"], extra = {}) => ({ channel: "preview", checkedAt: 1, state, release, ...extra }) as UpdateStatus;
  assert.match(updateNotice(info, "auto", make("downloading", { received: 1024, total: null }), null).title, /Downloading/);
  assert.match(updateNotice(info, "auto", make("verifying"), null).title, /Verifying/);
  assert.match(updateNotice(info, "auto", make("ready"), null).message, /Signature verified/);
  assert.match(updateNotice(info, "auto", make("preparing"), null).message, /saving drafts/);
  assert.match(updateNotice(info, "auto", make("installing"), null).title, /Installing/);
  for (const state of ["downloadFailed", "installFailed"] as const) {
    const notice = updateNotice(info, "auto", make(state, { message: "Try again" }), null);
    assert.equal(notice.error, true);
    assert.equal(notice.message, "Try again");
  }
  assert.match(updateNotice({ ...info, canInstall: false }, "auto", make("available"), null).message, /release page/);
});
