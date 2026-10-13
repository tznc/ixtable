import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";
import { compareVersions, readVersions, resolvePlan } from "../release/plan.mjs";
import {
  exportedNames,
  missingSecrets,
  notarizationMethod,
  releaseConnectSrc,
  requiredSecrets,
  tauriConfigOverride,
  writeApiKey,
} from "../release/signing.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "../..");
const conf = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"));
const versions = { tauri: "1.2.0", cargo: "1.2.0" };

describe("release plan", () => {
  it("keeps tauri.conf.json and Cargo.toml on one version", () => {
    const v = readVersions(root);
    assert.deepEqual(Object.keys(v).sort(), ["cargo", "tauri"]);
    assert.equal(v.cargo, v.tauri);
  });

  it("maps tags to channels and refuses mismatched or untagged releases", () => {
    assert.deepEqual(resolvePlan({ event: "push", ref: "refs/tags/v1.2.0", versions }), {
      channel: "stable",
      version: "1.2.0",
      tag: "v1.2.0",
      prerelease: false,
    });
    const beta = { tauri: "1.3.0-beta.1", cargo: "1.3.0-beta.1" };
    const betaPlan = resolvePlan({ event: "push", ref: "refs/tags/v1.3.0-beta.1", versions: beta });
    assert.equal(betaPlan.channel, "beta");
    assert.equal(betaPlan.prerelease, true);
    assert.throws(
      () => resolvePlan({ event: "push", ref: "refs/tags/v1.2.1", versions }),
      /tag v1.2.0/,
    );
    assert.throws(
      () => resolvePlan({ event: "push", ref: "refs/heads/main", versions }),
      /must be tags/,
    );
    assert.throws(
      () =>
        resolvePlan({
          event: "push",
          ref: "refs/tags/v1.2.0",
          versions: { ...versions, cargo: "1.1.0" },
        }),
      /Versions disagree/,
    );
  });

  it("lets manual runs build drafts anywhere but beta/stable only from the tag", () => {
    assert.deepEqual(
      resolvePlan({
        event: "workflow_dispatch",
        ref: "refs/heads/main",
        channelInput: "draft",
        versions,
      }),
      { channel: "draft", version: "1.2.0", tag: "", prerelease: true },
    );
    assert.throws(
      () =>
        resolvePlan({
          event: "workflow_dispatch",
          ref: "refs/heads/main",
          channelInput: "beta",
          versions,
        }),
      /from tag v1.2.0/,
    );
    const fromTag = resolvePlan({
      event: "workflow_dispatch",
      ref: "refs/tags/v1.2.0",
      channelInput: "beta",
      versions,
    });
    assert.equal(fromTag.channel, "beta");
    assert.equal(fromTag.tag, "v1.2.0");
    const pre = { tauri: "2.0.0-rc.1", cargo: "2.0.0-rc.1" };
    assert.throws(
      () =>
        resolvePlan({
          event: "workflow_dispatch",
          ref: "refs/tags/v2.0.0-rc.1",
          channelInput: "stable",
          versions: pre,
        }),
      /Stable releases need a release version/,
    );
    assert.throws(
      () =>
        resolvePlan({
          event: "workflow_dispatch",
          ref: "refs/tags/v1.2.0",
          channelInput: "nightly",
          versions,
        }),
      /Unknown channel/,
    );
  });

  it("orders versions by SemVer precedence", () => {
    const sorted = [
      "1.0.0",
      "1.0.0-alpha",
      "1.0.0-beta.11",
      "1.0.0-beta.2",
      "0.9.9",
      "1.0.0-alpha.1",
      "1.0.1",
    ].sort(compareVersions);
    assert.deepEqual(sorted, [
      "0.9.9",
      "1.0.0-alpha",
      "1.0.0-alpha.1",
      "1.0.0-beta.2",
      "1.0.0-beta.11",
      "1.0.0",
      "1.0.1",
    ]);
    assert.throws(() => compareVersions("1.0", "1.0.0"), /semantic/);
  });
});

describe("signing secrets", () => {
  const all = (platform) => Object.fromEntries(requiredSecrets(platform).map((name) => [name, "x"]));

  it("names every missing secret per platform and treats blanks as missing", () => {
    assert.deepEqual(missingSecrets("linux", {}), []);
    assert.deepEqual(missingSecrets("macos", { ...all("macos"), APPLE_ID: "  " }), ["APPLE_ID"]);
    assert.deepEqual(missingSecrets("windows", all("windows")), []);
    const mac = requiredSecrets("macos");
    for (const name of [
      "APPLE_CERTIFICATE",
      "APPLE_SIGNING_IDENTITY",
      "APPLE_TEAM_ID",
      "APPLE_PASSWORD",
    ])
      assert.ok(mac.includes(name), name);
    assert.throws(() => requiredSecrets("beos"), /Unknown platform/);
  });

  it("exports only secrets that are set", () => {
    assert.deepEqual(exportedNames("macos", { APPLE_CERTIFICATE: "" }), []);
    assert.deepEqual(exportedNames("macos", { APPLE_ID: "a" }), ["APPLE_ID"]);
  });

  it("notarizes with an App Store Connect API key once one is set, and never exports its text", () => {
    assert.equal(notarizationMethod({}), "appleId");
    const apiKey = { APPLE_API_ISSUER: "i", APPLE_API_KEY: "ABC123", APPLE_API_PRIVATE_KEY: "k" };
    assert.equal(notarizationMethod({ APPLE_API_KEY: "ABC123" }), "apiKey");
    const cert = {
      APPLE_CERTIFICATE: "c",
      APPLE_CERTIFICATE_PASSWORD: "p",
      APPLE_SIGNING_IDENTITY: "s",
    };
    assert.deepEqual(missingSecrets("macos", { ...cert, ...apiKey }), []);
    assert.deepEqual(missingSecrets("macos", { ...cert, APPLE_API_KEY: "ABC123" }), [
      "APPLE_API_ISSUER",
      "APPLE_API_PRIVATE_KEY",
    ]);
    assert.ok(!exportedNames("macos", { ...cert, ...apiKey }).includes("APPLE_API_PRIVATE_KEY"));
  });

  it("writes the API key where the Tauri CLI looks for it, readable only by the runner", () => {
    const dir = mkdtempSync(join(tmpdir(), "ixtable-signing-"));
    const path = writeApiKey({ APPLE_API_KEY: "ABC123", APPLE_API_PRIVATE_KEY: "pem" }, dir);
    assert.equal(path, join(dir, "AuthKey_ABC123.p8"));
    assert.equal(readFileSync(path, "utf8"), "pem\n");
    if (process.platform !== "win32") assert.equal(statSync(path).mode & 0o077, 0);
    assert.equal(writeApiKey({ APPLE_API_KEY: "ABC123" }, dir), undefined);
    assert.throws(
      () => writeApiKey({ APPLE_API_KEY: "../x", APPLE_API_PRIVATE_KEY: "pem" }, dir),
      /key ID/,
    );
  });

  it("signs Windows through Azure Trusted Signing only with all credentials", () => {
    const signed = tauriConfigOverride("windows", all("windows"));
    assert.equal(signed.bundle.windows.signCommand.cmd, "trusted-signing-cli");
    assert.equal(signed.bundle.windows.signCommand.args.at(-1), "%1");
    const draft = tauriConfigOverride("windows", { AZURE_CLIENT_ID: "x" });
    assert.deepEqual(draft.bundle, {});
  });

  it("drops the local cloud origin from release CSPs and adds only an https cloud origin", () => {
    assert.equal(
      tauriConfigOverride("linux", {}).app.security.csp["connect-src"],
      "'self' ipc: http://ipc.localhost",
    );
    assert.equal(
      releaseConnectSrc("https://abc.supabase.co/some/path"),
      "'self' ipc: http://ipc.localhost https://abc.supabase.co",
    );
    assert.throws(() => releaseConnectSrc("http://abc.supabase.co"), /https/);
  });
});

describe("content security policy", () => {
  it("has no webview CSP in the base config and never allows eval or remote origins in releases", () => {
    // The frontend is gone: tauri.conf.json declares no windows and no CSP.
    assert.deepEqual(conf.app.windows, []);
    assert.equal(conf.app.security.csp, null);
    assert.equal(conf.app.security.devCsp, undefined);
    const csp = tauriConfigOverride("linux", {}).app.security.csp;
    for (const [directive, value] of Object.entries(csp)) {
      assert.doesNotMatch(
        value,
        /unsafe-eval|\*|https?:\/\/(?!127\.0\.0\.1|ipc\.localhost)/,
        directive,
      );
    }
    const connect = csp["connect-src"].split(" ");
    for (const v of ["'self'", "ipc:", "http://ipc.localhost"]) assert.ok(connect.includes(v), v);
  });
});
