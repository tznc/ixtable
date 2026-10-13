// Lists top-level `pub fn`s under src-tauri/src that nothing else in the crate names, so
// dead public functions cannot hide behind `pub` (rustc only warns about private ones).
// Exit 1 when any is found. Usage: node scripts/ci/unused-pub-rust.mjs
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "../..");
const SRC = join(ROOT, "src-tauri", "src");
const DECL = /^pub(?:\(crate\))?\s+(?:async\s+)?fn\s+([A-Za-z_]\w*)/gm;

const files = [];
const walk = (dir) => {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) walk(path);
    else if (entry.name.endsWith(".rs")) files.push(path);
  }
};
walk(SRC);

const texts = new Map(files.map((f) => [f, readFileSync(f, "utf8")]));
const all = [...texts.values()].join("\n");
const declared = [];
for (const [file, text] of texts) {
  for (const m of text.matchAll(DECL)) {
    const line = text.slice(0, m.index).split("\n").length;
    declared.push({ file: relative(ROOT, file), line, name: m[1] });
  }
}
const declarations = new Map();
for (const d of declared) declarations.set(d.name, (declarations.get(d.name) ?? 0) + 1);
const uses = (name) => all.match(new RegExp(`\\b${name}\\b`, "g"))?.length ?? 0;
const unused = declared.filter((d) => uses(d.name) <= declarations.get(d.name));
for (const d of unused) console.log(`${d.file}:${d.line}: unused public function \`${d.name}\``);
console.log(`${unused.length} unused public Rust function(s)`);
process.exit(unused.length ? 1 : 0);
