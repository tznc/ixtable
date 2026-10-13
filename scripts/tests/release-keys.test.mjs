import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createPrivateKey, createPublicKey, generateKeyPairSync, randomBytes } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, describe, it } from "node:test";
import { fileURLToPath } from "node:url";
import {
  DEV_CLOUD_KEYS,
  devCloudKeys,
  embeddedKeyProblems,
  rawCloudKey,
  releaseCloudKey,
  releaseKeyProblems,
} from "../release/keys.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const scratch = mkdtempSync(join(tmpdir(), "ixtable-keys-test-"));
after(() => rmSync(scratch, { recursive: true, force: true }));

function cloudKey() {
  const spki = generateKeyPairSync("ed25519").publicKey.export({ format: "der", type: "spki" });
  return { spki: spki.toString("base64"), raw: spki.subarray(12).toString("base64") };
}

const production = () => ({
  IXTABLE_CLOUD_PUBLIC_KEY_RAW: cloudKey().raw,
});

describe("release key gate", () => {
  it("accepts production keys from variables", () => {
    assert.deepEqual(releaseKeyProblems(production(), root), []);
    assert.match(
      releaseKeyProblems({ ...production(), IXTABLE_CLOUD_PUBLIC_KEY_RAW: "" }, root)[0],
      /IXTABLE_CLOUD_PUBLIC_KEY_RAW is not set/,
    );
    const spkiOnly = { IXTABLE_CLOUD_PUBLIC_KEY: cloudKey().spki };
    assert.deepEqual(releaseKeyProblems(spkiOnly, root), []);
    const blankRaw = { ...spkiOnly, IXTABLE_CLOUD_PUBLIC_KEY_RAW: "" };
    assert.equal(releaseCloudKey(blankRaw), spkiOnly.IXTABLE_CLOUD_PUBLIC_KEY);
    assert.deepEqual(releaseKeyProblems(blankRaw, root), []);
    assert.equal(
      releaseCloudKey({ ...blankRaw, IXTABLE_CLOUD_PUBLIC_KEY_RAW: " \n" }),
      spkiOnly.IXTABLE_CLOUD_PUBLIC_KEY,
    );
  });

  it("refuses a missing key", () => {
    const problems = releaseKeyProblems({}, root);
    assert.equal(problems.length, 1);
    assert.match(problems[0], /IXTABLE_CLOUD_PUBLIC_KEY_RAW is not set/);
  });

  it("refuses dev and test cloud keys in every accepted encoding", () => {
    const rfc8032 = Buffer.from(DEV_CLOUD_KEYS[0], "hex");
    const spki = Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), rfc8032]);
    for (const value of [DEV_CLOUD_KEYS[0], rfc8032.toString("base64"), spki.toString("base64")])
      assert.deepEqual(
        releaseKeyProblems({ ...production(), IXTABLE_CLOUD_PUBLIC_KEY_RAW: value }, root),
        ["IXTABLE_CLOUD_PUBLIC_KEY_RAW is a development or test key"],
      );
  });

  it("treats this checkout's local Supabase key as a dev key", () => {
    const local = cloudKey();
    mkdirSync(join(scratch, "supabase/functions"), { recursive: true });
    writeFileSync(
      join(scratch, "supabase/functions/.env.local"),
      `IXTABLE_CLOUD_PUBLIC_KEY=${local.spki}\nIXTABLE_CLOUD_PUBLIC_KEY_RAW=${local.raw}\n`,
    );
    assert.equal(devCloudKeys(scratch).has(rawCloudKey(local.raw).toString("hex")), true);
    assert.equal(devCloudKeys(root).has("0".repeat(64)), true);
  });

  it("lists every Ed25519 key committed to the repository as a dev key, without .env.local", () => {
    const git = (args) => spawnSync("git", args, { cwd: root, encoding: "utf8" }).stdout;
    const prefixes = [
      ["302e0201003005", "06032b657004220420"].join(""),
      "302a300506032b6570032100",
    ].map((h) => Buffer.from(h, "hex").toString("base64").slice(0, 16));
    const files = git(["grep", "-l", "-I", ...prefixes.flatMap((p) => ["-e", p])])
      .split("\n")
      .filter(Boolean);
    const found = new Set();
    for (const file of files) {
      const text = readFileSync(join(root, file), "utf8");
      for (const [b64] of text.matchAll(/MC[4o][A-Za-z0-9+/]{40,}={0,2}/g)) {
        const der = Buffer.from(b64, "base64");
        const pub =
          der.length === 48
            ? createPublicKey(createPrivateKey({ key: der, format: "der", type: "pkcs8" }))
            : createPublicKey({ key: der, format: "der", type: "spki" });
        found.add(pub.export({ format: "der", type: "spki" }).subarray(12).toString("hex"));
      }
    }
    assert.ok(found.size > 0);
    const ciKeys = devCloudKeys(scratch);
    for (const key of found) assert.ok(ciKeys.has(key), key);
    assert.ok(
      DEV_CLOUD_KEYS.includes("57b73bbef374efb52c4197e46cc61c783cf1cd01266dbb8abc0a0b1e8bd1728d"),
    );
  });

  it("refuses malformed keys", () => {
    const problems = releaseKeyProblems({ IXTABLE_CLOUD_PUBLIC_KEY_RAW: "abc" }, root);
    assert.deepEqual(problems, ["IXTABLE_CLOUD_PUBLIC_KEY_RAW: not a valid Ed25519 public key"]);
  });

  it("prefers the raw cloud key variable", () => {
    assert.equal(
      releaseCloudKey({ IXTABLE_CLOUD_PUBLIC_KEY: "b", IXTABLE_CLOUD_PUBLIC_KEY_RAW: "a" }),
      "a",
    );
  });

  it("checks what a built binary pins", () => {
    const env = production();
    const good = Buffer.concat([randomBytes(64), Buffer.from(env.IXTABLE_CLOUD_PUBLIC_KEY_RAW)]);
    assert.deepEqual(embeddedKeyProblems(good, env), []);
    assert.deepEqual(embeddedKeyProblems(randomBytes(64), env), [
      "the binary does not pin the release ixtable Cloud key",
    ]);
  });
});

describe("tauri-action build wrapper", () => {
  const wrapper = (env) =>
    spawnSync(process.execPath, [join(root, "scripts/release/keys.mjs"), "build", "--help"], {
      cwd: root,
      encoding: "utf8",
      env: { ...process.env, ...env },
    });

  it(
    "fails a release build whose binary does not pin the release keys",
    { timeout: 180_000 },
    () => {
      const binary = join(scratch, "ixtable-bin");
      writeFileSync(binary, randomBytes(64));
      const env = { ...production(), IXTABLE_RELEASE_BINARY: binary };
      // Runs the Tauri CLI through npx (needs network on first use).
      assert.equal(wrapper({ ...env, IXTABLE_RELEASE: "" }).status, 0);
      const release = wrapper({ ...env, IXTABLE_RELEASE: "1" });
      assert.equal(release.status, 1);
      assert.match(release.stderr, /does not pin the release ixtable Cloud key/);
      writeFileSync(binary, Buffer.from(env.IXTABLE_CLOUD_PUBLIC_KEY_RAW));
      assert.equal(wrapper({ ...env, IXTABLE_RELEASE: "1" }).status, 0);
    },
  );
});
