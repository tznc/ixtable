#!/usr/bin/env node
// Desktop icon set for Tauri bundles: `tauri icon` renders every platform's icons from one
// source image; only the desktop files listed in DESKTOP_ICONS are kept (no Square*Logo,
// StoreLogo, android or ios output). iconProblems() validates headers with no dependencies.
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");

/** Paths relative to src-tauri, in bundle order. */
export const DESKTOP_ICONS = [
  "icons/32x32.png",
  "icons/64x64.png",
  "icons/128x128.png",
  "icons/128x128@2x.png",
  "icons/icon.icns",
  "icons/icon.ico",
  "icons/icon.png",
];

/** Exact PNG sizes; icon.png only needs to be at least `min` and square. */
const PNG_SIZES = {
  "icons/32x32.png": 32,
  "icons/64x64.png": 64,
  "icons/128x128.png": 128,
  "icons/128x128@2x.png": 256,
};
const ICON_PNG = { name: "icons/icon.png", min: 512 };
export const ICO_SIZES = [16, 24, 32, 48, 64, 256];
// `tauri icon` writes is32/s8mk/il32/l8mk/ic07-ic14. ic10 (1024) and ic09 (512) are the
// large entries every run emits and the ones macOS needs for Retina Dock and Finder icons.
export const ICNS_TYPES = { ic10: 1024, ic09: 512 };

const PNG_SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

/** Reads width, height and color type from a PNG buffer, or returns an error string. */
export function readPngHeader(buf) {
  if (buf.length < 26 || !buf.subarray(0, 8).equals(PNG_SIGNATURE)) return "not a PNG";
  if (buf.toString("latin1", 12, 16) !== "IHDR") return "PNG has no IHDR chunk";
  return { width: buf.readUInt32BE(16), height: buf.readUInt32BE(20), colorType: buf[25] };
}

function pngProblems(name, buf, expected, min) {
  const header = readPngHeader(buf);
  if (typeof header === "string") return [`${name}: ${header}`];
  const { width, height, colorType } = header;
  const problems = [];
  if (width !== height) problems.push(`${name}: not square (${width}x${height})`);
  else if (expected !== undefined && width !== expected)
    problems.push(`${name}: expected ${expected}x${expected}, got ${width}x${height}`);
  else if (min !== undefined && width < min)
    problems.push(`${name}: expected at least ${min}x${min}, got ${width}x${height}`);
  if (colorType !== 6) problems.push(`${name}: needs RGBA (color type 6), got ${colorType}`);
  return problems;
}

function icoProblems(name, buf) {
  if (buf.length < 6 || buf.readUInt16LE(0) !== 0 || buf.readUInt16LE(2) !== 1)
    return [`${name}: not an ICO file`];
  const count = buf.readUInt16LE(4);
  if (buf.length < 6 + count * 16) return [`${name}: truncated ICO directory`];
  const sizes = new Set();
  for (let i = 0; i < count; i++) sizes.add(buf[6 + i * 16] || 256);
  return ICO_SIZES.filter((s) => !sizes.has(s)).map((s) => `${name}: missing ${s}px entry`);
}

function icnsProblems(name, buf) {
  if (buf.length < 8 || buf.toString("latin1", 0, 4) !== "icns")
    return [`${name}: not an ICNS file`];
  const types = new Set();
  for (let pos = 8; pos + 8 <= buf.length; ) {
    const len = buf.readUInt32BE(pos + 4);
    if (len < 8) return [`${name}: corrupt ICNS entry`];
    types.add(buf.toString("latin1", pos, pos + 4));
    pos += len;
  }
  return Object.entries(ICNS_TYPES)
    .filter(([type]) => !types.has(type))
    .map(([type, size]) => `${name}: missing ${size}px entry (${type})`);
}

/** Problems with the icon set in `dir` (the icons directory itself); empty means OK. */
export function iconProblems(dir) {
  const problems = [];
  for (const rel of DESKTOP_ICONS) {
    const name = rel.replace(/^icons\//, "");
    const file = join(dir, name);
    if (!existsSync(file)) {
      problems.push(`${name}: missing`);
      continue;
    }
    const buf = readFileSync(file);
    if (name.endsWith(".ico")) problems.push(...icoProblems(name, buf));
    else if (name.endsWith(".icns")) problems.push(...icnsProblems(name, buf));
    else if (rel === ICON_PNG.name)
      problems.push(...pngProblems(name, buf, undefined, ICON_PNG.min));
    else problems.push(...pngProblems(name, buf, PNG_SIZES[rel]));
  }
  return problems;
}

/** Runs the local Tauri CLI through node, so no shell or .cmd shim is involved on Windows. */
function runTauriIcon(source, out) {
  const cli = createRequire(join(ROOT, "package.json")).resolve("@tauri-apps/cli/tauri.js");
  execFileSync(process.execPath, [cli, "icon", source, "-o", out], {
    cwd: ROOT,
    stdio: ["ignore", "ignore", "inherit"],
  });
}

/** Renders icons from `source` (.svg or .png) and copies only DESKTOP_ICONS into `outDir`. */
export function generate({ source, outDir }) {
  const src = resolve(source);
  if (!existsSync(src)) throw new Error(`Icon source not found: ${src}`);
  const tmp = mkdtempSync(join(tmpdir(), "ixtable-icons-"));
  try {
    runTauriIcon(src, tmp);
    mkdirSync(resolve(outDir), { recursive: true });
    for (const rel of DESKTOP_ICONS) {
      const name = rel.replace(/^icons\//, "");
      const from = join(tmp, name);
      if (!existsSync(from)) throw new Error(`tauri icon did not produce ${name}`);
      copyFileSync(from, join(resolve(outDir), name));
    }
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}

function defaultSource() {
  for (const rel of ["branding/app-icon.svg", "branding/app-icon.png"]) {
    if (existsSync(join(ROOT, rel))) return join(ROOT, rel);
  }
  return join(ROOT, "branding/app-icon.svg");
}

function report(dir) {
  const problems = iconProblems(dir);
  for (const p of problems) console.error(p);
  if (problems.length > 0) return 1;
  console.log(`Icons OK in ${dir}`);
  return 0;
}

function main(argv) {
  const [command, arg] = argv;
  if (command === "generate") {
    const outDir = join(ROOT, "src-tauri", "icons");
    try {
      generate({ source: arg ?? defaultSource(), outDir });
    } catch (error) {
      console.error(error.message);
      return 1;
    }
    return report(outDir);
  }
  if (command === "check") return report(arg ?? join(ROOT, "src-tauri", "icons"));
  console.error("Usage: icons.mjs generate [source] | check [dir]");
  return 2;
}

if (import.meta.url === pathToFileURL(process.argv[1]).href)
  process.exit(main(process.argv.slice(2)));
