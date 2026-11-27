// Published Release identity is read once; every job uses the resolved outputs.
import { execFileSync } from "node:child_process";

export const migration = Object.freeze({
  tag: "v0.1.750", version: "2026.8.0",
  commit: "94960fcf284af8c4795390a04b97ca66e0ceb1e4",
  source: "9cae2f0228c4ebeb16b6e6fc1a7d72fbc275f20f",
});
const numeric = "(0|[1-9]\\d*)";
export const stablePattern = new RegExp(`^${numeric}\\.${numeric}\\.${numeric}$`);
export const previewPattern = new RegExp(`^${numeric}\\.${numeric}\\.${numeric}-preview\\.${numeric}$`);

export function nextVersion(base, kind, year) {
  if (!/^[1-9]\d{3}\.[1-9]\d*\.(0|[1-9]\d*)$/.test(base)) throw new Error("Invalid year-based Release version");
  const [yearText, series, patch] = base.split(".");
  const previousYear = Number(yearText);
  if (!Number.isInteger(year) || year < previousYear || year > 9999) throw new Error("Invalid release year");
  if (kind === "hotfix") return `${previousYear}.${series}.${BigInt(patch) + 1n}`;
  if (kind === "planned") return year > previousYear ? `${year}.1.0` : `${year}.${BigInt(series) + 1n}.0`;
  throw new Error("Choose planned or hotfix explicitly");
}

export function publishedBase(release, git) {
  if (!release || release.draft || release.prerelease || !release.published_at) throw new Error("Latest must be a published stable Release");
  const tag = release.tag_name;
  if (tag === migration.tag) {
    if (git("rev-parse", `${tag}^{commit}`) !== migration.commit) throw new Error("Migration tag moved");
    return { ...migration };
  }
  const version = tag?.replace(/^v/, "");
  nextVersion(version, "hotfix", Number(version?.split(".")[0]));
  if (tag !== `v${version}`) throw new Error("Invalid Release tag");
  const commit = git("rev-parse", `${tag}^{commit}`);
  // Promotion merges preserve the source as their second parent, even off preview's history.
  const parents = git("show", "-s", "--format=%P", commit).split(" ").filter(Boolean);
  if (parents.length > 2) throw new Error("Ambiguous Release promotion merge");
  return { tag, version, commit, source: parents[1] ?? commit };
}

export function resolveVersions({ channel, kind, tag, year, release, git }) {
  const base = publishedBase(release, git);
  const planned = nextVersion(base.version, "planned", year);
  const hotfix = nextVersion(base.version, "hotfix", year);
  const sha = git("rev-parse", "HEAD");
  const count = git("rev-list", "--count", "HEAD");
  if (!/^(0|[1-9]\d*)$/.test(count)) throw new Error("Invalid Preview commit count");
  let version;
  if (channel === "preview") version = `${planned}-preview.${count}`;
  else if (channel === "release") {
    if (!kind) kind = tag === `v${planned}` ? "planned" : tag === `v${hotfix}` ? "hotfix" : undefined;
    version = nextVersion(base.version, kind, year);
    if (tag && tag !== `v${version}`) throw new Error("Release tag differs from the calculated version");
  } else throw new Error("Unknown distribution channel");
  git("merge-base", "--is-ancestor", base.source, "HEAD");
  return { sha, version, tag: channel === "preview" ? `preview-${sha}` : `v${version}`,
    release_kind: kind ?? "planned", base_release: base.version, base_tag: base.tag,
    base_commit: base.commit, base_source: base.source,
    commits_since_release: git("rev-list", "--count", `${base.source}..HEAD`) };
}

export function resolvePublished(channel, kind, tag) {
  const git = (...args) => execFileSync("git", args, { encoding: "utf8" }).trim();
  const repository = process.env.GITHUB_REPOSITORY || "MrFreeZZZ/oxbow";
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository)) throw new Error("Invalid GitHub repository");
  const release = JSON.parse(execFileSync("gh", ["api", `repos/${repository}/releases/latest`], { encoding: "utf8" }));
  return resolveVersions({ channel, kind, tag, year: new Date().getUTCFullYear(), release, git });
}
