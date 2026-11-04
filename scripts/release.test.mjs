import assert from "node:assert/strict";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { spawnSync } from "node:child_process";
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { buildConfig, collect, identity, manifest, platforms, verifyArtifact, verifyBuildMetadata } from "./release.mjs";
import { previewDecision, validatePublished } from "./publish-release.mjs";

const { publicKey, privateKey } = generateKeyPairSync("ed25519");
const id = Buffer.from("0102030405060708", "hex");
const rawKey = publicKey.export({ format: "der", type: "spki" }).subarray(-32);
const keyPacket = Buffer.concat([Buffer.from("Ed"), id, rawKey]);
const key = Buffer.from(`untrusted comment: test\n${keyPacket.toString("base64")}\n`).toString("base64");
const data = Buffer.from("Oxbow signed distribution fixture");
const sha = "a".repeat(40);

function signature(bytes, version = "0.1.550") {
  const raw = sign(null, createHash("blake2b512").update(bytes).digest(), privateKey);
  const packet = Buffer.concat([Buffer.from("ED"), id, raw]);
  const comment = `timestamp:1700000000\tfile:test\tversion:${version}`;
  const global = sign(null, Buffer.concat([raw, Buffer.from(comment)]), privateKey);
  return Buffer.from(`untrusted comment: test\n${packet.toString("base64")}\ntrusted comment: ${comment}\n${global.toString("base64")}\n`).toString("base64");
}

function fixture(t, channel = "preview") {
  const root = mkdtempSync(join(tmpdir(), "oxbow-distribution-test-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const info = identity(channel, "0.1.550", sha, channel === "preview" ? `preview-${sha}` : "v0.1.550");
  const directory = join(root, "release-assets");
  mkdirSync(join(directory, "metadata"), { recursive: true });
  for (const platform of platforms[channel]) {
    const filename = `${platform}.bin`;
    writeFileSync(join(directory, filename), data);
    writeFileSync(join(directory, `${filename}.sig`), signature(data));
    writeFileSync(join(directory, "metadata", `${platform}.json`), JSON.stringify({ ...info, platform, filename, installers: [], sha256: createHash("sha256").update(data).digest("hex") }));
  }
  return { root, directory, info };
}

test("Release requires a matching tag and Preview uses an immutable commit tag", () => {
  assert.throws(() => identity("release", "0.1.550", sha, "v0.1.549"), /Expected/);
  assert.throws(() => identity("preview", "0.1.550", sha, "preview"), /Expected/);
  assert.throws(() => identity("preview", "0.1.550", "abc", "preview-abc"), /full commit/);
  assert.throws(() => identity("other", "0.1.550", sha, "other"), /Unknown/);
});

test("signed data, public key and version must all match", () => {
  const sig = signature(data);
  verifyArtifact(data, sig, key, "0.1.550");
  assert.throws(() => verifyArtifact(Buffer.from("changed"), sig, key, "0.1.550"), /verification failed/);
  assert.throws(() => verifyArtifact(data, sig, key, "0.1.551"), /version/);
  const changed = Buffer.from(Buffer.from(sig, "base64").toString().replace("0.1.550", "0.1.551")).toString("base64");
  assert.throws(() => verifyArtifact(data, changed, key, "0.1.551"), /comment verification/);
  const wrongPacket = Buffer.from(keyPacket); wrongPacket[12] ^= 1;
  const wrongKey = Buffer.from(`untrusted comment: test\n${wrongPacket.toString("base64")}\n`).toString("base64");
  assert.throws(() => verifyArtifact(data, sig, wrongKey, "0.1.550"), /verification failed/);
});

test("build configurations bind numeric versions and the correct channel endpoint", () => {
  for (const channel of ["release", "preview"]) {
    const info = identity(channel, "0.1.550", sha, channel === "release" ? "v0.1.550" : `preview-${sha}`);
    const config = buildConfig(info);
    assert.equal(config.version, info.version);
    assert.equal(config.bundle.createUpdaterArtifacts, true);
    assert.deepEqual(config.plugins.updater.endpoints, [channel === "release"
      ? "https://github.com/MrFreeZZZ/oxbow/releases/latest/download/latest.json"
      : "https://github.com/MrFreeZZZ/oxbow/releases/download/preview/latest.json"]);
  }
  assert.throws(() => identity("release", "0.1.0550", sha, "v0.1.0550"), /Invalid distribution version/);
});

test("the built executable must match the package version, channel, architecture and source", () => {
  const info = identity("preview", "0.1.550", sha, `preview-${sha}`);
  const metadata = { version: info.version, commit: sha.slice(0, 7), buildChannel: "preview", channel: "preview", platform: "darwin-aarch64" };
  verifyBuildMetadata(metadata, info, "darwin-aarch64");
  for (const [field, value] of [["version", "0.1.0"], ["buildChannel", "release"], ["platform", "darwin-x86_64"], ["commit", "bbbbbbb"], ["commit", `${sha.slice(0, 7)}, modified`]]) {
    assert.throws(() => verifyBuildMetadata({ ...metadata, [field]: value }, info, "darwin-aarch64"));
  }
  verifyBuildMetadata({ ...metadata, commit: `${sha.slice(0, 7)}, modified` }, info, "darwin-aarch64", true);
});

for (const channel of ["release", "preview"]) {
  test(`${channel} manifest has exactly the supported platforms and pinned URLs`, (t) => {
    const { directory, info } = fixture(t, channel);
    const result = manifest(directory, info, "MrFreeZZZ/oxbow", "Release notes", key);
    assert.deepEqual(Object.keys(result.platforms).sort(), [...platforms[channel]].sort());
    for (const entry of Object.values(result.platforms)) {
      assert.ok(entry.url.startsWith(`https://github.com/MrFreeZZZ/oxbow/releases/download/${info.tag}/`));
      assert.equal(entry.signature, signature(data));
    }
    validatePublished(result, info, "MrFreeZZZ/oxbow", directory, key);
  });
}

test("incomplete and duplicate matrices cannot be published", (t) => {
  const { directory, info } = fixture(t, "release");
  const file = join(directory, "metadata", "windows-x86_64.json");
  unlinkSync(file);
  assert.throws(() => manifest(directory, info, "MrFreeZZZ/oxbow", "", key), /Incomplete/);
  copyFileSync(join(directory, "metadata", "darwin-aarch64.json"), file);
  assert.throws(() => manifest(directory, info, "MrFreeZZZ/oxbow", "", key), /duplicate/);
});

test("mixed commits, changed artifacts and path escapes are rejected", (t) => {
  const { directory, info } = fixture(t);
  const path = join(directory, "metadata", "darwin-aarch64.json");
  const entry = JSON.parse(readFileSync(path));
  writeFileSync(path, JSON.stringify({ ...entry, sha: "b".repeat(40) }));
  assert.throws(() => manifest(directory, info, "MrFreeZZZ/oxbow", "", key), /Mixed/);
  writeFileSync(path, JSON.stringify({ ...entry, filename: "../outside" }));
  assert.throws(() => manifest(directory, info, "MrFreeZZZ/oxbow", "", key), /filename/);
  writeFileSync(path, JSON.stringify(entry));
  writeFileSync(join(directory, entry.filename), "changed");
  assert.throws(() => manifest(directory, info, "MrFreeZZZ/oxbow", "", key), /changed after/);
});

test("Windows and Linux collection require valid signatures and preserve installers", (t) => {
  const { root } = fixture(t);
  const info = identity("release", "0.1.550", sha, "v0.1.550");
  for (const [platform, folder, suffix] of [["linux-x86_64", "appimage", ".AppImage"], ["windows-x86_64", "nsis", "-setup.exe"]]) {
    const bundle = join(root, platform);
    mkdirSync(join(bundle, folder), { recursive: true });
    const path = join(bundle, folder, `Oxbow${suffix}`);
    writeFileSync(path, data);
    if (platform.startsWith("linux")) { mkdirSync(join(bundle, "deb")); writeFileSync(join(bundle, "deb", "Oxbow.deb"), data); }
    assert.throws(() => collect(bundle, join(root, `out-${platform}`), platform, info, key), /ENOENT/);
    writeFileSync(`${path}.sig`, signature(data));
    const entry = collect(bundle, join(root, `out-${platform}`), platform, info, key);
    assert.equal(entry.platform, platform);
    assert.equal(entry.installers.length, platform.startsWith("linux") ? 1 : 0);
    writeFileSync(path, "tampered");
    assert.throws(() => collect(bundle, join(root, `bad-${platform}`), platform, info, key), /verification failed/);
  }
});

test("Preview cannot regress, treats numeric versions correctly and detects collisions", () => {
  const current = { channel: "preview", version: "0.1.550", commit: sha };
  assert.equal(previewDecision(null, current), "advance");
  assert.equal(previewDecision(current, { ...current, version: "0.1.99" }), "keep-newer");
  assert.equal(previewDecision(current, { ...current, version: "0.1.551" }), "advance");
  assert.equal(previewDecision(current, current), "unchanged");
  assert.throws(() => previewDecision(current, { ...current, commit: "b".repeat(40) }), /same Preview version/);
});

// A local gh stand-in exercises publication and retries without touching GitHub.
function publisher(t, channel) {
  const f = fixture(t, channel);
  manifest(f.directory, f.info, "MrFreeZZZ/oxbow", "notes", key);
  mkdirSync(join(f.root, "app/src-tauri"), { recursive: true });
  writeFileSync(join(f.root, "app/src-tauri/tauri.conf.json"), JSON.stringify({ plugins: { updater: { pubkey: key } } }));
  writeFileSync(join(f.root, "release-notes.txt"), "notes");
  const bin = join(f.root, "bin"); mkdirSync(bin);
  writeFileSync(join(bin, "gh"), `#!/usr/bin/env node
const fs=require('node:fs'),p=require('node:path'),root=process.env.FAKE_GH_DIR;
const args=process.argv.slice(2),stateFile=p.join(root,'state.json');
const state=fs.existsSync(stateFile)?JSON.parse(fs.readFileSync(stateFile)):{releases:{},calls:[]};
state.calls.push(args); const save=()=>fs.writeFileSync(stateFile,JSON.stringify(state));
const fail=()=>{save();console.error('(HTTP 404)');process.exit(1)};
const value=flag=>args[args.indexOf(flag)+1];
if(args[0]==='api'){
 const endpoint=args[1];let result;
 if(endpoint.includes('/releases/tags/'))result=state.releases[endpoint.split('/').pop()];
 else if(endpoint.includes('/commits/'))result={sha:process.env.SOURCE_SHA};
 if(!result)fail();save();console.log(JSON.stringify(result));
}else{
 const tag=args[2],dir=p.join(root,'remote-'+tag),r=state.releases[tag];
 if(args[1]==='create'){state.releases[tag]={draft:args.includes('--draft'),prerelease:args.includes('--prerelease'),name:value('--title'),assets:[]};fs.mkdirSync(dir,{recursive:true});}
 else if(args[1]==='edit'){if(!r)fail();if(args.includes('--draft=false'))r.draft=false;}
 else if(args[1]==='upload'){
  if(tag==='preview'&&process.env.FAIL_INDEX_ONCE&&!state.failed){state.failed=true;save();process.exit(1);}
  for(const file of args.slice(3,args.indexOf('--repo'))){const name=p.basename(file);fs.copyFileSync(file,p.join(dir,name));r.assets=r.assets.filter(e=>e.name!==name);r.assets.push({name});}
 }else if(args[1]==='download'){
  fs.mkdirSync(value('--dir'),{recursive:true});
  for(const file of fs.readdirSync(dir)){if(args.includes('--pattern')&&file!==value('--pattern'))continue;fs.copyFileSync(p.join(dir,file),p.join(value('--dir'),file));}
 }else fail();save();
}
`);
  chmodSync(join(bin, "gh"), 0o755);
  const env = { ...process.env, PATH: `${bin}:${process.env.PATH}`, FAKE_GH_DIR: f.root, CHANNEL: channel, RELEASE_VERSION: f.info.version, SOURCE_SHA: sha, RELEASE_TAG: f.info.tag, GITHUB_REPOSITORY: "MrFreeZZZ/oxbow" };
  const script = resolve(dirname(new URL(import.meta.url).pathname), "publish-release.mjs");
  return { ...f, run: (extra = {}) => spawnSync(process.execPath, [script], { cwd: f.root, env: { ...env, ...extra }, encoding: "utf8" }), state: () => JSON.parse(readFileSync(join(f.root, "state.json"))) };
}

test("Release stays a draft and uploads its manifest last", (t) => {
  const f = publisher(t, "release"), result = f.run();
  assert.equal(result.status, 0, result.stderr);
  const state = f.state();
  assert.equal(state.releases[f.info.tag].draft, true);
  assert.equal(state.releases[f.info.tag].prerelease, false);
  assert.equal(state.calls.filter((a) => a[1] === "upload").at(-1)[3], "release-assets/latest.json");
  assert.equal(state.releases.preview, undefined);
});

test("Preview recovers after snapshot publication without replacing signed packages", (t) => {
  const f = publisher(t, "preview");
  assert.equal(f.run({ FAIL_INDEX_ONCE: "1" }).status, 1);
  assert.equal(f.state().releases[f.info.tag].draft, false);
  const before = f.state().calls.filter((a) => a[1] === "upload" && a[2] === f.info.tag).length;
  const retry = f.run(); assert.equal(retry.status, 0, retry.stderr);
  const state = f.state();
  assert.equal(state.calls.filter((a) => a[1] === "upload" && a[2] === f.info.tag).length, before);
  const index = JSON.parse(readFileSync(join(f.root, "remote-preview/latest.json")));
  assert.equal(index.commit, sha);
  assert.equal(state.releases.preview.prerelease, true);
  assert.ok(state.calls.some((a) => a.includes("--latest=false")));
});

test("a published stable release cannot be replaced", (t) => {
  const f = publisher(t, "release");
  writeFileSync(join(f.root, "state.json"), JSON.stringify({ releases: { [f.info.tag]: { draft: false } }, calls: [] }));
  assert.equal(f.run().status, 1);
  assert.ok(f.state().calls.every((a) => a[0] === "api"));
});

test("a delayed Preview run leaves a newer channel index untouched", (t) => {
  const f = publisher(t, "preview");
  assert.equal(f.run().status, 0);
  const path = join(f.root, "remote-preview/latest.json");
  const newer = { ...JSON.parse(readFileSync(path)), version: "0.1.551", commit: "b".repeat(40) };
  writeFileSync(path, JSON.stringify(newer));
  const before = f.state().calls.filter((a) => a[1] === "upload" && a[2] === "preview").length;
  const delayed = f.run(); assert.equal(delayed.status, 0, delayed.stderr);
  assert.equal(f.state().calls.filter((a) => a[1] === "upload" && a[2] === "preview").length, before);
  assert.deepEqual(JSON.parse(readFileSync(path)), newer);
});
