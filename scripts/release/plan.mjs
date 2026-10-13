#!/usr/bin/env node
// Resolves what a release run builds: channel, version, and tag (release.yml `plan` job).
// Fails when the tag and the two version files disagree.
import { appendFileSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

export const CHANNELS = ["draft", "beta", "stable"];
const SEMVER = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/;

/** Versions declared by tauri.conf.json and Cargo.toml. */
export function readVersions(root) {
  const json = (file) => JSON.parse(readFileSync(join(root, file), "utf8"));
  const cargo = readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8");
  return {
    tauri: json("src-tauri/tauri.conf.json").version,
    cargo: cargo.match(/^\[package\][^[]*?^version\s*=\s*"([^"]+)"/m)?.[1],
  };
}

export function parseVersion(version) {
  const m = SEMVER.exec(version ?? "");
  if (!m) return null;
  return { core: [Number(m[1]), Number(m[2]), Number(m[3])], pre: m[4] ? m[4].split(".") : [] };
}

/** SemVer 2 precedence: negative, zero, or positive. */
export function compareVersions(a, b) {
  const x = parseVersion(a);
  const y = parseVersion(b);
  if (!x || !y) throw new Error(`Not a semantic version: ${!x ? a : b}`);
  for (let i = 0; i < 3; i++) if (x.core[i] !== y.core[i]) return x.core[i] - y.core[i];
  if (!x.pre.length || !y.pre.length) return y.pre.length - x.pre.length;
  for (let i = 0; i < Math.max(x.pre.length, y.pre.length); i++) {
    const [p, q] = [x.pre[i], y.pre[i]];
    if (p === undefined || q === undefined) return p === undefined ? -1 : 1;
    if (p === q) continue;
    const [np, nq] = [/^\d+$/.test(p), /^\d+$/.test(q)];
    if (np && nq) return Number(p) - Number(q);
    if (np !== nq) return np ? -1 : 1;
    return p < q ? -1 : 1;
  }
  return 0;
}

/**
 * Tag pushes: `vX.Y.Z` releases to stable, `vX.Y.Z-pre` to beta. Manual runs pick
 * a channel; beta and stable must still run from the matching tag.
 */
export function resolvePlan({ event, ref, channelInput, versions }) {
  const { tauri, cargo } = versions;
  if (!parseVersion(tauri)) throw new Error(`tauri.conf.json version is not semver: ${tauri}`);
  if (tauri !== cargo)
    throw new Error(`Versions disagree: tauri.conf.json ${tauri}, Cargo.toml ${cargo}`);
  const version = tauri;
  const prerelease = parseVersion(version).pre.length > 0;
  const tagged = ref?.startsWith("refs/tags/") ? ref.slice("refs/tags/".length) : "";
  let channel;
  if (event === "push") {
    if (!tagged) throw new Error(`Release pushes must be tags, got ${ref}`);
    channel = prerelease ? "beta" : "stable";
  } else {
    channel = channelInput || "draft";
    if (!CHANNELS.includes(channel)) throw new Error(`Unknown channel ${channel}`);
  }
  if (channel === "draft") return { channel, version, tag: "", prerelease: true };
  if (tagged !== `v${version}`)
    throw new Error(`The ${channel} channel releases from tag v${version}; this run is on ${ref}`);
  if (channel === "stable" && prerelease)
    throw new Error(`Stable releases need a release version, not ${version}`);
  return { channel, version, tag: tagged, prerelease: channel !== "stable" };
}

function main() {
  const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
  const plan = resolvePlan({
    event: process.env.GITHUB_EVENT_NAME,
    ref: process.env.GITHUB_REF,
    channelInput: process.env.RELEASE_CHANNEL,
    versions: readVersions(root),
  });
  console.log(JSON.stringify(plan));
  if (process.env.GITHUB_OUTPUT)
    appendFileSync(
      process.env.GITHUB_OUTPUT,
      Object.entries(plan)
        .map(([k, v]) => `${k}=${v}\n`)
        .join(""),
    );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    main();
  } catch (error) {
    console.error(`::error::${error.message}`);
    process.exit(1);
  }
}
