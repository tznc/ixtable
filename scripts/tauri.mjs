#!/usr/bin/env node
// Runs the pinned Tauri CLI (`node scripts/tauri.mjs dev`, `... build`, `... icon`) without a
// root package.json: npx fetches and caches the exact version on first use.
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

export const TAURI_CLI = "@tauri-apps/cli@2.11.4";

/** Runs the CLI with `args` from the repository root and returns its exit status. */
export function runTauri(args, stdio = "inherit") {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const npx = process.platform === "win32" ? "npx.cmd" : "npx";
  const result = spawnSync(npx, ["--yes", TAURI_CLI, ...args], {
    cwd: root,
    stdio,
    shell: process.platform === "win32",
  });
  if (result.error) throw result.error;
  return result.status ?? 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exit(runTauri(process.argv.slice(2)));
}
