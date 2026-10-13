import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, before, describe, it } from "node:test";
import { deflateSync } from "node:zlib";
import { DESKTOP_ICONS, generate, iconProblems } from "../release/icons.mjs";

const SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
<circle cx="512" cy="512" r="400" fill="#cc3333"/></svg>`;

let work = "";
let icons = "";
const fresh = () => {
  const dir = mkdtempSync(join(work, "set-"));
  for (const f of readdirSync(icons)) writeFileSync(join(dir, f), readFileSync(join(icons, f)));
  return dir;
};

function png(width, height, colorType = 6) {
  const chunk = (type, data) => {
    const head = Buffer.alloc(8);
    head.writeUInt32BE(data.length, 0);
    head.write(type, 4, "latin1");
    return Buffer.concat([head, data, Buffer.alloc(4)]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = colorType;
  const sig = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  return Buffer.concat([
    sig,
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(Buffer.alloc(1))),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

function ico(sizes) {
  const buf = Buffer.alloc(6 + sizes.length * 16);
  buf.writeUInt16LE(1, 2);
  buf.writeUInt16LE(sizes.length, 4);
  sizes.forEach((s, i) => {
    buf[6 + i * 16] = s === 256 ? 0 : s;
  });
  return buf;
}

before(
  () => {
    work = mkdtempSync(join(tmpdir(), "ixtable-icons-test-"));
    const source = join(work, "app-icon.svg");
    writeFileSync(source, SVG);
    icons = join(work, "generated");
    // Runs the Tauri CLI through npx, which needs network on first use.
    generate({ source, outDir: icons });
  },
  { timeout: 180_000 },
);

after(() => rmSync(work, { recursive: true, force: true }));

describe("release icons", () => {
  it("generates only the desktop set and it validates", () => {
    assert.deepEqual(
      readdirSync(icons).sort(),
      DESKTOP_ICONS.map((p) => p.replace("icons/", "")).sort(),
    );
    assert.deepEqual(iconProblems(icons), []);
  });

  it("fails clearly when the source is missing", () => {
    assert.throws(
      () => generate({ source: join(work, "nope.svg"), outDir: join(work, "x") }),
      /not found/,
    );
  });

  it("flags a missing file", () => {
    const dir = fresh();
    rmSync(join(dir, "icon.icns"));
    assert.deepEqual(iconProblems(dir), ["icon.icns: missing"]);
  });

  it("flags a PNG of the wrong size", () => {
    const dir = fresh();
    writeFileSync(join(dir, "64x64.png"), png(32, 32));
    assert.deepEqual(iconProblems(dir), ["64x64.png: expected 64x64, got 32x32"]);
  });

  it("flags non-square, non-RGBA and too-small icon.png", () => {
    const dir = fresh();
    writeFileSync(join(dir, "32x32.png"), png(32, 16));
    writeFileSync(join(dir, "128x128.png"), png(128, 128, 2));
    writeFileSync(join(dir, "icon.png"), png(256, 256));
    const problems = iconProblems(dir);
    assert.ok(problems.includes("32x32.png: not square (32x16)"));
    assert.ok(problems.includes("128x128.png: needs RGBA (color type 6), got 2"));
    assert.ok(problems.includes("icon.png: expected at least 512x512, got 256x256"));
  });

  it("flags an ICO without the 256 entry and a bad ICNS", () => {
    const dir = fresh();
    writeFileSync(join(dir, "icon.ico"), ico([16, 24, 32, 48, 64]));
    writeFileSync(join(dir, "icon.icns"), Buffer.from("nope"));
    assert.deepEqual(iconProblems(dir), [
      "icon.icns: not an ICNS file",
      "icon.ico: missing 256px entry",
    ]);
  });

  it("creates its output directory", () => {
    mkdirSync(join(work, "empty"));
    assert.equal(iconProblems(join(work, "empty")).length, DESKTOP_ICONS.length);
  });
});
