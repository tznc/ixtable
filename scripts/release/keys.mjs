#!/usr/bin/env node
// Release key gates for release.yml (PRD §27.2 "signed bundles must fail closed").
// The production public key comes from repository variables, never from the repository:
//   IXTABLE_CLOUD_PUBLIC_KEY_RAW  ixtable Cloud bundle-signing key (raw 32-byte Ed25519, base64),
//                                 or IXTABLE_CLOUD_PUBLIC_KEY (SPKI DER base64)
// A beta/stable build refuses to run when a key is missing, malformed, or a known dev/test key.
//   embedded --binary f   check a built binary pins the release cloud key
//   build <tauri args>    tauri-action's tauriScript: `tauri build`, then, when IXTABLE_RELEASE=1, the
//                         embedded check on IXTABLE_RELEASE_BINARY, so a bad binary fails before upload
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { runTauri } from "../tauri.mjs";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "../..");

/**
 * Ed25519 public keys that must never be pinned into a release (hex, raw 32 bytes), from
 * dev-cloud-keys.json, which src-tauri/build.rs also reads. Add any key that was ever used for
 * development or tests and shared outside a local machine; a unit test fails when a committed
 * Ed25519 key is missing from the list.
 */
export const DEV_CLOUD_KEYS = JSON.parse(
  readFileSync(join(ROOT, "scripts/release/dev-cloud-keys.json"), "utf8"),
).keys;

const SPKI_PREFIX = Buffer.from("302a300506032b6570032100", "hex");
const present = (value) => typeof value === "string" && value.trim() !== "";

/** Raw 32-byte Ed25519 key from base64 (raw or SPKI DER), base64url, hex, or PEM, as cloud/config.rs accepts. */
export function rawCloudKey(text) {
  const t = text.trim().replace(/^"|"$/g, "");
  let bytes;
  if (/^[0-9a-fA-F]{64}$/.test(t)) bytes = Buffer.from(t, "hex");
  else if (t.startsWith("-----BEGIN"))
    bytes = Buffer.from(
      t
        .split(/\r?\n/)
        .filter((line) => !line.startsWith("-----"))
        .join(""),
      "base64",
    );
  else bytes = Buffer.from(t.replace(/-/g, "+").replace(/_/g, "/"), "base64");
  if (bytes.length === 44 && bytes.subarray(0, 12).equals(SPKI_PREFIX)) bytes = bytes.subarray(12);
  if (bytes.length !== 32) throw new Error("not a valid Ed25519 public key");
  return bytes;
}

/**
 * Cloud keys that are dev/test only: the committed list, plus this checkout's local stack key when
 * one exists. dev-secrets.mjs generates that key per checkout and never shares it, so CI (which has
 * no .env.local) relies on the committed list alone.
 */
export function devCloudKeys(root = ROOT) {
  const keys = new Set(DEV_CLOUD_KEYS);
  const local = join(root, "supabase/functions/.env.local");
  if (existsSync(local)) {
    const match = /^IXTABLE_CLOUD_PUBLIC_KEY(?:_RAW)?=(.+)$/gm;
    for (const [, value] of readFileSync(local, "utf8").matchAll(match)) {
      try {
        keys.add(rawCloudKey(value).toString("hex"));
      } catch {
        // A malformed local key cannot be pinned anyway.
      }
    }
  }
  return keys;
}

/**
 * The cloud key text the build pins. cloud/config.rs and build.rs read the RAW variable first and
 * treat a blank one as unset, so an unset repository variable (`""` in release.yml) falls back.
 */
export function releaseCloudKey(env) {
  if (present(env.IXTABLE_CLOUD_PUBLIC_KEY_RAW)) return env.IXTABLE_CLOUD_PUBLIC_KEY_RAW.trim();
  if (present(env.IXTABLE_CLOUD_PUBLIC_KEY)) return env.IXTABLE_CLOUD_PUBLIC_KEY.trim();
  return null;
}

/** Problems that make a build unfit for beta/stable. Names variables, never secret values. */
export function releaseKeyProblems(env, root = ROOT) {
  const problems = [];
  const cloud = releaseCloudKey(env);
  if (!cloud) {
    problems.push(
      "IXTABLE_CLOUD_PUBLIC_KEY_RAW is not set (a build without it refuses every cloud install)",
    );
  } else {
    try {
      if (devCloudKeys(root).has(rawCloudKey(cloud).toString("hex")))
        problems.push("IXTABLE_CLOUD_PUBLIC_KEY_RAW is a development or test key");
    } catch (error) {
      problems.push(`IXTABLE_CLOUD_PUBLIC_KEY_RAW: ${error.message}`);
    }
  }
  return problems;
}

/** Problems with a built binary: it must hold the release cloud key text. */
export function embeddedKeyProblems(binary, env) {
  const problems = [];
  const cloud = releaseCloudKey(env);
  if (cloud && !binary.includes(Buffer.from(cloud)))
    problems.push("the binary does not pin the release ixtable Cloud key");
  return problems;
}

function arg(name) {
  const index = process.argv.indexOf(`--${name}`);
  return index > 0 ? process.argv[index + 1] : undefined;
}

async function main() {
  const [command] = process.argv.slice(2);
  const env = process.env;
  if (command === "embedded") {
    const problems = embeddedKeyProblems(readFileSync(arg("binary")), env);
    if (problems.length) throw new Error(problems.join("; "));
    console.log("binary pins the release keys");
    return;
  }
  if (command === "build") {
    // Runs inside tauri-action, which uploads to the draft release only when this exits 0.
    const status = runTauri(process.argv.slice(2));
    if (status !== 0) process.exit(status);
    if (env.IXTABLE_RELEASE !== "1") return;
    if (!present(env.IXTABLE_RELEASE_BINARY)) throw new Error("IXTABLE_RELEASE_BINARY is not set");
    const problems = embeddedKeyProblems(readFileSync(env.IXTABLE_RELEASE_BINARY), env);
    if (problems.length) throw new Error(problems.join("; "));
    console.log("binary pins the release keys");
    return;
  }
  throw new Error("usage: keys.mjs embedded --binary <file> | build <tauri args>");
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(`::error::${error.message}`);
    process.exit(1);
  });
}
