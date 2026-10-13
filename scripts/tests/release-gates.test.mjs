import assert from "node:assert/strict";
import { generateKeyPairSync } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";
import {
  cargoDependencies,
  committedPrivateKeys,
  GATES,
  onlyDevTestKeys,
  PRIVATE_KEY_FIXTURES,
  runGates,
  unreviewedCrypto,
} from "../ci/release-gates.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");

describe("release gates", () => {
  it("passes every automated gate on this tree and leaves human sign-offs pending", () => {
    const results = runGates(root);
    assert.deepEqual(
      results.filter((r) => r.status === "fail"),
      [],
    );
    const pending = results.filter((r) => r.status === "pending").map((r) => r.id);
    for (const id of ["external-security-review", "key-ceremony", "green-ci-all-os"])
      assert.ok(pending.includes(id), `${id} should be pending`);
  });

  it("maps every gate in docs/release-checklist.md, and nothing else", () => {
    const doc = readFileSync(join(root, "docs/release-checklist.md"), "utf8");
    const documented = [...doc.matchAll(/^\| `([a-z0-9-]+)` \|/gm)].map((m) => m[1]);
    assert.deepEqual(
      documented.sort(),
      GATES.map((g) => g.id).sort(),
    );
  });

  it("reports a failing check and a check that throws as failures", () => {
    const results = runGates(root, [
      { id: "a", prd: "x", title: "a", check: () => ["broken"] },
      {
        id: "b",
        prd: "x",
        title: "b",
        check: () => {
          throw new Error("boom");
        },
      },
    ]);
    assert.deepEqual(
      results.map((r) => [r.status, r.detail]),
      [
        ["fail", "broken"],
        ["fail", "boom"],
      ],
    );
  });

  it("flags crypto dependencies that are not on the allowlist", () => {
    const toml = `[package]\nname = "x"\n[dependencies]\nsha2 = "0.10"\naes-siv = "1"\nserde = "1"\n[dev-dependencies]\nring = "0.17"\n[profile.dev]\ndebug = 1\n`;
    assert.deepEqual(cargoDependencies(toml), ["sha2", "aes-siv", "serde", "ring"]);
    assert.deepEqual(unreviewedCrypto(toml), ["aes-siv", "ring"]);
  });

  it("finds committed private keys by name or content", () => {
    const files = {
      "a.txt": "hello",
      "b.pem": `-----BEGIN ${"PRIVATE"} KEY-----\nMC4CAQ==\n-----END PRIVATE KEY-----`,
      "release/updater.key": "x",
      "c.txt": Buffer.from(
        ["untrusted comment:", "rsign encrypted secret key\nRWR"].join(" "),
      ).toString("base64"),
      "cert.p12": "",
    };
    assert.deepEqual(
      committedPrivateKeys(Object.keys(files), (f) => files[f]),
      ["b.pem", "release/updater.key", "c.txt", "cert.p12"],
    );
  });

  it("finds Ed25519 PKCS8 keys and allows only the dev keys in known fixtures", () => {
    const pkcs8 = (
      der = generateKeyPairSync("ed25519").privateKey.export({ format: "der", type: "pkcs8" }),
    ) => der.toString("base64");
    const fixture = PRIVATE_KEY_FIXTURES[0];
    const committed = readFileSync(join(root, fixture), "utf8");
    assert.equal(onlyDevTestKeys(committed), true);
    const fresh = pkcs8();
    const pem = `-----BEGIN ${"PRIVATE"} KEY-----`;
    const files = {
      [fixture]: committed,
      "new.ts": `const key = "${fresh}";`,
      "hex.ts": `const key = "${Buffer.from(fresh, "base64").toString("hex").toUpperCase()}";`,
      "copy/crypto_test.ts": committed,
    };
    const read = (f) => files[f];
    assert.deepEqual(committedPrivateKeys(Object.keys(files), read), [
      "new.ts",
      "hex.ts",
      "copy/crypto_test.ts",
    ]);
    for (const planted of [`${committed}\n"${fresh}"`, `${committed}\n${pem}`])
      assert.deepEqual(
        committedPrivateKeys([fixture], () => planted),
        [fixture],
      );
  });
});
