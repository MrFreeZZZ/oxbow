// Published packages are never replaced. Only the small Preview channel index moves;
// its URLs always refer to an immutable per-commit release.
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { identity, platforms, verifyArtifact } from "./release.mjs";

export function previewDecision(current, next) {
  if (!current) return "advance";
  if (current.channel !== "preview") throw new Error("Existing Preview index has an unexpected channel");
  const parse = (v) => {
    if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(v)) throw new Error("Invalid Preview version");
    return v.split(".").map(BigInt);
  };
  const a = parse(current.version), b = parse(next.version);
  for (let i = 0; i < a.length; i++) {
    if (a[i] > b[i]) return "keep-newer";
    if (a[i] < b[i]) return "advance";
  }
  if (current.commit !== next.commit) throw new Error("Different commits have the same Preview version");
  return "unchanged";
}

function gh(...args) {
  return execFileSync("gh", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] });
}

function optionalApi(repository, path) {
  try { return JSON.parse(gh("api", `repos/${repository}/${path}`)); }
  catch (error) {
    if (/\(HTTP 404\)/.test(String(error.stderr))) return null;
    throw new Error(`GitHub API request failed: ${path}`);
  }
}

function downloadIndex(repository, tag, directory) {
  gh("release", "download", tag, "--repo", repository, "--pattern", "latest.json", "--dir", directory);
  return JSON.parse(readFileSync(join(directory, "latest.json"), "utf8"));
}

export function validatePublished(manifest, info, repository, directory, key) {
  if (manifest.channel !== info.channel || manifest.commit !== info.sha || manifest.version !== info.version) throw new Error("Published build identity does not match");
  if (JSON.stringify(Object.keys(manifest.platforms).sort()) !== JSON.stringify([...platforms[info.channel]].sort())) throw new Error("Published build matrix is incomplete");
  for (const entry of Object.values(manifest.platforms)) {
    const prefix = `https://github.com/${repository}/releases/download/${info.tag}/`;
    if (!entry.url.startsWith(prefix)) throw new Error("Published package URL points to another release");
    const name = decodeURIComponent(entry.url.slice(prefix.length));
    if (/[\\/]/.test(name) || !name || name === "." || name === "..") throw new Error("Invalid published asset name");
    verifyArtifact(readFileSync(join(directory, name)), entry.signature, key, info.version);
  }
}

function main() {
  const info = identity(process.env.CHANNEL, process.env.RELEASE_VERSION, process.env.SOURCE_SHA, process.env.RELEASE_TAG);
  const repository = process.env.GITHUB_REPOSITORY;
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository)) throw new Error("Invalid repository");
  const work = mkdtempSync(join(tmpdir(), "oxbow-publish-"));
  try {
    let manifest = JSON.parse(readFileSync("release-assets/latest.json", "utf8"));
    const key = JSON.parse(readFileSync("app/src-tauri/tauri.conf.json", "utf8")).plugins.updater.pubkey;
    validatePublished(manifest, info, repository, "release-assets", key);
    const release = optionalApi(repository, `releases/tags/${info.tag}`);
    // A missing ref returns 404 here; commits/<missing tag> returns 422 instead.
    const tag = optionalApi(repository, `git/ref/tags/${info.tag}`);
    if (tag) {
      // Resolve annotated tags through the commit endpoint too, rather than comparing tag objects.
      const tagCommit = optionalApi(repository, `commits/${info.tag}`);
      if (tagCommit?.sha !== info.sha) throw new Error("Remote tag no longer points to the built commit");
    }
    if (release && !release.draft) {
      if (info.channel !== "preview" || !release.prerelease) throw new Error("A published release cannot be overwritten");
      // Recover a retry that published the snapshot but failed before updating the index.
      gh("release", "download", info.tag, "--repo", repository, "--dir", work);
      manifest = JSON.parse(readFileSync(join(work, "latest.json"), "utf8"));
      validatePublished(manifest, info, repository, work, key);
      console.log("Reusing the previously published, verified Preview snapshot.");
    } else {
      const title = info.channel === "preview" ? `Oxbow ${info.version} — Preview (${info.sha.slice(0, 7)})` : `Oxbow ${info.version}`;
      const flags = ["--repo", repository, "--title", title, "--notes-file", "release-notes.txt"];
      if (info.channel === "preview") flags.push("--prerelease", "--latest=false");
      if (!release) {
        if (info.channel === "release") flags.push("--verify-tag");
        gh("release", "create", info.tag, "--draft", "--target", info.sha, ...flags);
      } else gh("release", "edit", info.tag, ...flags);
      const assets = readdirSync("release-assets", { withFileTypes: true }).filter((e) => e.isFile() && e.name !== "latest.json").map((e) => join("release-assets", e.name));
      gh("release", "upload", info.tag, ...assets, "--repo", repository, "--clobber");
      // Upload the manifest last, after every package and its signature have arrived.
      gh("release", "upload", info.tag, "release-assets/latest.json", "--repo", repository, "--clobber");
      if (info.channel === "release") {
        console.log("Release draft is ready. Publish it as Latest when reviewed.");
        return;
      }
      gh("release", "edit", info.tag, "--repo", repository, "--draft=false", "--prerelease", "--latest=false");
    }
    const index = optionalApi(repository, "releases/tags/preview");
    if (index && (index.draft || !index.prerelease || index.name !== "Oxbow Preview updates")) throw new Error("The preview tag is occupied by an unrelated release");
    let current = null;
    if (index?.assets.some((e) => e.name === "latest.json")) {
      const directory = mkdtempSync(join(work, "index-"));
      current = downloadIndex(repository, "preview", directory);
    }
    const decision = previewDecision(current, manifest);
    if (decision !== "advance") {
      console.log(`Preview index: ${decision}.`);
      return;
    }
    if (!index) {
      const occupied = optionalApi(repository, "git/ref/tags/preview");
      if (occupied) throw new Error("The preview tag already exists without a channel index release");
      const notes = "Rolling index for macOS Apple Silicon Preview updates. Signed packages are stored in per-commit Preview releases; latest.json identifies their exact commit.";
      const notesFile = join(work, "index-notes.txt");
      writeFileSync(notesFile, notes);
      gh("release", "create", "preview", "--repo", repository, "--target", info.sha, "--prerelease", "--latest=false", "--title", "Oxbow Preview updates", "--notes-file", notesFile);
    }
    const latest = join(work, "preview-index", "latest.json");
    // Keep the name latest.json for the updater's fixed endpoint.
    mkdirSync(join(work, "preview-index"));
    writeFileSync(latest, `${JSON.stringify(manifest, null, 2)}\n`);
    gh("release", "upload", "preview", latest, "--repo", repository, "--clobber");
    console.log(`Preview index now points to ${info.sha}.`);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) { console.error(error.message); process.exitCode = 1; }
}
