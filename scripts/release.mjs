// Packaging and validation shared by Release and Preview. Uses Node's Ed25519 and BLAKE2b
// implementations to verify the Minisign format used by Tauri before publishing anything.
import { createHash, createPublicKey, verify } from "node:crypto";
import { execFileSync } from "node:child_process";
import { appendFileSync, copyFileSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const channels = JSON.parse(readFileSync(new URL("../app/src-tauri/update-channels.json", import.meta.url), "utf8"));
export const platforms = Object.fromEntries(Object.entries(channels).map(([name, definition]) => [name, definition.platforms]));
const matrix = [
  { name: "macOS Apple Silicon", os: "macos-latest", target: "aarch64-apple-darwin", platform: "darwin-aarch64", bundles: "app" },
  { name: "macOS Intel", os: "macos-latest", target: "x86_64-apple-darwin", platform: "darwin-x86_64", bundles: "app" },
  { name: "Linux x64", os: "ubuntu-22.04", target: "x86_64-unknown-linux-gnu", platform: "linux-x86_64", bundles: "deb,appimage" },
  { name: "Windows x64", os: "windows-latest", target: "x86_64-pc-windows-msvc", platform: "windows-x86_64", bundles: "nsis" },
];

function requireValue(ok, message) {
  if (!ok) throw new Error(message);
}

export function identity(channel, version, sha, tag) {
  requireValue(Object.hasOwn(platforms, channel), "Unknown distribution channel");
  requireValue(/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version), "Invalid distribution version");
  requireValue(/^[a-f0-9]{40}$/.test(sha), "Distribution must use a full commit SHA");
  const expected = channel === "release" ? `v${version}` : `preview-${sha}`;
  requireValue(tag === expected, `Expected distribution tag ${expected}`);
  return { channel, version, sha, tag };
}

function base64(text) {
  const value = text.trim();
  requireValue(/^[A-Za-z0-9+/]+={0,2}$/.test(value), "Invalid base64 encoding");
  const bytes = Buffer.from(value, "base64");
  requireValue(bytes.toString("base64") === value, "Noncanonical base64 encoding");
  return bytes;
}

export function verifyArtifact(data, encodedSignature, encodedPublicKey, version) {
  const keyLines = base64(encodedPublicKey).toString("utf8").trim().split(/\r?\n/);
  const keyPacket = base64(keyLines[1] ?? "");
  requireValue(keyPacket.length === 42 && keyPacket.subarray(0, 2).toString() === "Ed", "Invalid Minisign public key");
  const lines = base64(encodedSignature).toString("utf8").trim().split(/\r?\n/);
  requireValue(lines.length === 4 && lines[2].startsWith("trusted comment: "), "Invalid Minisign signature");
  const packet = base64(lines[1]);
  const globalSignature = base64(lines[3]);
  requireValue(packet.length === 74 && globalSignature.length === 64, "Invalid signature length");
  requireValue(packet.subarray(2, 10).equals(keyPacket.subarray(2, 10)), "Signature belongs to a different key");
  const algorithm = packet.subarray(0, 2).toString();
  requireValue(algorithm === "ED" || algorithm === "Ed", "Unsupported Minisign algorithm");
  const publicKey = createPublicKey({ key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), keyPacket.subarray(10)]), format: "der", type: "spki" });
  const signature = packet.subarray(10);
  const signedData = algorithm === "ED" ? createHash("blake2b512").update(data).digest() : data;
  requireValue(verify(null, signedData, publicKey, signature), "Artifact signature verification failed");
  const comment = lines[2].slice("trusted comment: ".length);
  requireValue(verify(null, Buffer.concat([signature, Buffer.from(comment)]), publicKey, globalSignature), "Signed comment verification failed");
  const versions = comment.split("\t").filter((field) => field.startsWith("version:"));
  requireValue(versions.length === 1 && versions[0].slice(8) === version, "Signature version does not match the release");
}

function publicKey() {
  return JSON.parse(readFileSync("app/src-tauri/tauri.conf.json", "utf8")).plugins.updater.pubkey;
}

function one(directory, suffix) {
  const files = readdirSync(directory).filter((name) => name.endsWith(suffix));
  requireValue(files.length === 1, `Expected one ${suffix} in ${directory}, got ${files.length}`);
  return join(directory, files[0]);
}

export function collect(directory, destination, platform, info, key = publicKey()) {
  requireValue(platforms[info.channel]?.includes(platform), "Platform does not belong to this channel");
  identity(info.channel, info.version, info.sha, info.tag);
  mkdirSync(join(destination, "metadata"), { recursive: true });
  const prefix = `Oxbow_${info.version}_${platform}`;
  let source, filename;
  const installers = [];
  if (platform.startsWith("darwin-")) {
    source = one(join(directory, "macos"), ".app.tar.gz");
    filename = `${prefix}.app.tar.gz`;
    const zip = join(destination, `${prefix}.zip`);
    execFileSync("ditto", ["-c", "-k", "--sequesterRsrc", "--keepParent", join(directory, "macos", "Oxbow.app"), zip]);
    installers.push(basename(zip));
  } else if (platform.startsWith("linux-")) {
    source = one(join(directory, "appimage"), ".AppImage");
    filename = `${prefix}.AppImage`;
    copyFileSync(one(join(directory, "deb"), ".deb"), join(destination, `${prefix}.deb`));
    installers.push(`${prefix}.deb`);
  } else {
    source = one(join(directory, "nsis"), "-setup.exe");
    filename = `${prefix}-setup.exe`;
  }
  const signature = readFileSync(`${source}.sig`, "utf8").trim();
  const data = readFileSync(source);
  verifyArtifact(data, signature, key, info.version);
  copyFileSync(source, join(destination, filename));
  copyFileSync(`${source}.sig`, join(destination, `${filename}.sig`));
  const entry = { ...info, platform, filename, installers, sha256: createHash("sha256").update(data).digest("hex") };
  writeFileSync(join(destination, "metadata", `${platform}.json`), `${JSON.stringify(entry, null, 2)}\n`);
  return entry;
}

export function manifest(directory, info, repository, notes, key = publicKey()) {
  identity(info.channel, info.version, info.sha, info.tag);
  requireValue(/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository), "Invalid GitHub repository");
  const entries = readdirSync(join(directory, "metadata")).map((file) => JSON.parse(readFileSync(join(directory, "metadata", file), "utf8")));
  const expected = [...platforms[info.channel]].sort();
  requireValue(JSON.stringify(entries.map((e) => e.platform).sort()) === JSON.stringify(expected), "Incomplete or duplicate platform matrix");
  const result = { version: info.version, notes, pub_date: new Date().toISOString(), platforms: {}, channel: info.channel, commit: info.sha };
  for (const entry of entries) {
    requireValue(["channel", "version", "sha", "tag"].every((k) => entry[k] === info[k]), "Mixed builds in release artifacts");
    requireValue(entry.filename === basename(entry.filename) && !entry.filename.includes("\\"), "Invalid asset filename");
    for (const installer of entry.installers) {
      requireValue(installer === basename(installer) && !installer.includes("\\"), "Invalid installer filename");
      requireValue(readFileSync(join(directory, installer)).length > 0, "Empty installer");
    }
    const file = join(directory, entry.filename);
    const signature = readFileSync(`${file}.sig`, "utf8").trim();
    const data = readFileSync(file);
    requireValue(createHash("sha256").update(data).digest("hex") === entry.sha256, "Artifact changed after collection");
    verifyArtifact(data, signature, key, info.version);
    result.platforms[entry.platform] = { url: `https://github.com/${repository}/releases/download/${info.tag}/${encodeURIComponent(entry.filename)}`, signature };
  }
  writeFileSync(join(directory, "latest.json"), `${JSON.stringify(result, null, 2)}\n`);
  return result;
}

function environmentInfo() {
  return identity(process.env.CHANNEL, process.env.RELEASE_VERSION, process.env.SOURCE_SHA, process.env.RELEASE_TAG);
}

export function buildConfig(info) {
  identity(info.channel, info.version, info.sha, info.tag);
  return {
    version: info.version,
    bundle: { createUpdaterArtifacts: true },
    plugins: { updater: { endpoints: [channels[info.channel].endpoint] } },
  };
}

export function verifyBuildMetadata(metadata, info, platform, allowModified = false) {
  requireValue(metadata.version === info.version, "Executable version differs from distribution version");
  requireValue(metadata.buildChannel === info.channel && metadata.channel === info.channel, "Executable belongs to a different update channel");
  requireValue(metadata.platform === platform, "Executable architecture differs from the package target");
  const commit = metadata.commit.split(",")[0];
  requireValue(/^[a-f0-9]{7,40}$/.test(commit) && info.sha.startsWith(commit), "Executable was built from another commit");
  requireValue(allowModified || !metadata.commit.includes(","), "Distribution executable contains uncommitted changes");
}

function main() {
  const command = process.argv[2];
  if (command === "resolve") {
    const git = (...args) => execFileSync("git", args, { encoding: "utf8" }).trim();
    const sha = git("rev-parse", "HEAD");
    const base = JSON.parse(readFileSync("app/src-tauri/tauri.conf.json", "utf8")).version.split(".").slice(0, 2).join(".");
    const version = `${base}.${git("rev-list", "--count", "HEAD")}`;
    const channel = process.env.CHANNEL;
    const tag = channel === "preview" ? `preview-${sha}` : process.env.RELEASE_TAG;
    identity(channel, version, sha, tag);
    const outputs = { sha, version, tag, epoch: git("show", "-s", "--format=%ct", "HEAD"), matrix: JSON.stringify({ include: matrix.filter((item) => platforms[channel].includes(item.platform)) }) };
    appendFileSync(process.env.GITHUB_OUTPUT, Object.entries(outputs).map(([k, v]) => `${k}=${v}\n`).join(""));
  } else if (command === "config") {
    // Only distribution jobs require a private key. Normal PR/local builds remain unsigned.
    const config = buildConfig(environmentInfo());
    writeFileSync("app/release-config.json", `${JSON.stringify(config, null, 2)}\n`);
  } else if (command === "collect") {
    collect(process.env.BUNDLE_DIR, "release-assets", process.env.PLATFORM, environmentInfo());
  } else if (command === "verify-build") {
    const info = environmentInfo();
    const allowModified = process.env.OXBOW_VERIFY_ALLOW_MODIFIED === "1";
    requireValue(allowModified || execFileSync("git", ["status", "--porcelain", "--untracked-files=no"], { encoding: "utf8" }).trim() === "", "Distribution checkout contains uncommitted changes");
    const platform = process.env.PLATFORM;
    requireValue(platforms[info.channel].includes(platform), "Unsupported distribution platform");
    let binary = join(process.env.BUNDLE_DIR, "..", platform.startsWith("windows-") ? "oxbow.exe" : "oxbow");
    if (platform.startsWith("darwin-")) {
      const app = join(process.env.BUNDLE_DIR, "macos", "Oxbow.app", "Contents");
      const version = execFileSync("plutil", ["-extract", "CFBundleShortVersionString", "raw", "-o", "-", join(app, "Info.plist")], { encoding: "utf8" }).trim();
      requireValue(version === info.version, "macOS bundle version differs from distribution version");
      binary = join(app, "MacOS", "oxbow");
      const arch = execFileSync("lipo", ["-archs", binary], { encoding: "utf8" }).trim();
      requireValue(arch === (platform === "darwin-aarch64" ? "arm64" : "x86_64"), "macOS executable architecture mismatch");
    }
    const os = process.platform === "win32" ? "windows" : process.platform;
    const arch = { arm64: "aarch64", x64: "x86_64" }[process.arch];
    if (platform === `${os}-${arch}`) {
      const metadata = JSON.parse(execFileSync(resolve(binary), ["--build-info"], { encoding: "utf8" }));
      verifyBuildMetadata(metadata, info, platform, allowModified);
    }
    console.log(`Verified ${platform} package identity.`);
  } else if (command === "notes") {
    const info = environmentInfo();
    const description = info.channel === "preview"
      ? "Preview build for macOS Apple Silicon, automatically published after checks."
      : "Downloads for macOS Apple Silicon and Intel, Linux x64, and Windows x64.";
    writeFileSync("release-notes.txt", `## Oxbow ${info.version}\n\n${description}\n\nBuilt from commit ${info.sha}.\n\n### macOS installation\n\nThe macOS build is ad-hoc signed and is not notarized. After extracting Oxbow.app, run \`xattr -cr Oxbow.app\` in its directory, then move it to Applications.\n\nOxbow is in active development. Please report issues with your operating system, build name, and steps to reproduce.\n`);
  } else if (command === "manifest") {
    manifest("release-assets", environmentInfo(), process.env.GITHUB_REPOSITORY, readFileSync("release-notes.txt", "utf8"));
  } else throw new Error("Unknown release command");
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
