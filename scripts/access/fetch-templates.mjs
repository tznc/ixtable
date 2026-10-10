#!/usr/bin/env node
// Downloads the featured Microsoft Access templates for the optional corpus
// tests in src-tauri/src/access/tests (docs/decisions/access-import.md).
// Usage: node scripts/access/fetch-templates.mjs <directory>
// Then: IXTABLE_ACCESS_TEMPLATES_DIR=<directory> cargo test --lib access::
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

// Template ids from https://support.microsoft.com/en-us/access/featured-access-templates
const TEMPLATES = [
  "tf01225342",
  "tf01225343",
  "tf01225345",
  "tf01225346",
  "tf01225348",
  "tf01225349",
  "tf01225351",
  "tf01225353",
  "tf01225355",
  "tf01225356",
  "tf01228997",
  "tf10094830",
  "tf10206878",
  "tf10206879",
  "tf10206880",
  "tf10206881",
  "tf10206882",
  "tf10206883",
  "tf10206884",
  "tf10222094",
  "tf10222095",
  "tf10222096",
  "tf10238207",
  "tf10251225",
  "tf10288085",
  "tf10288086",
  "tf10333680",
  "tf10350815",
  "tf11138777_win32",
  "tf22238896_win32",
];
const BASE = "https://omextemplates.content.office.net/support/templates/en-us/";

const dir = process.argv[2];
if (!dir) {
  console.error("usage: node scripts/access/fetch-templates.mjs <directory>");
  process.exit(2);
}
mkdirSync(dir, { recursive: true });
let failed = 0;
for (const id of TEMPLATES) {
  const target = join(dir, `${id}.accdt`);
  if (existsSync(target)) continue;
  const response = await fetch(`${BASE}${id}.accdt`);
  if (!response.ok) {
    console.error(`${id}: HTTP ${response.status}`);
    failed += 1;
    continue;
  }
  writeFileSync(target, Buffer.from(await response.arrayBuffer()));
  console.log(`${id}.accdt`);
}
process.exit(failed ? 1 : 0);
