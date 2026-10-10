import { mkdtempSync, readFileSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { compareVersions, readVersions, resolvePlan } from "../../scripts/release/plan.mjs";
import {
  exportedNames,
  missingSecrets,
  notarizationMethod,
  releaseConnectSrc,
  requiredSecrets,
  tauriConfigOverride,
  writeApiKey,
} from "../../scripts/release/signing.mjs";

const root = join(__dirname, "../..");
const conf = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"));
const versions = { tauri: "1.2.0", npm: "1.2.0", cargo: "1.2.0" };

describe("release plan", () => {
  it("keeps tauri.conf.json, package.json, and Cargo.toml on one version", () => {
    const v = readVersions(root);
    expect(v.npm).toBe(v.tauri);
    expect(v.cargo).toBe(v.tauri);
  });

  it("maps tags to channels and refuses mismatched or untagged releases", () => {
    expect(resolvePlan({ event: "push", ref: "refs/tags/v1.2.0", versions })).toEqual({
      channel: "stable",
      version: "1.2.0",
      tag: "v1.2.0",
      prerelease: false,
    });
    const beta = { tauri: "1.3.0-beta.1", npm: "1.3.0-beta.1", cargo: "1.3.0-beta.1" };
    expect(
      resolvePlan({ event: "push", ref: "refs/tags/v1.3.0-beta.1", versions: beta }),
    ).toMatchObject({ channel: "beta", prerelease: true });
    expect(() => resolvePlan({ event: "push", ref: "refs/tags/v1.2.1", versions })).toThrow(
      /tag v1.2.0/,
    );
    expect(() => resolvePlan({ event: "push", ref: "refs/heads/main", versions })).toThrow(
      /must be tags/,
    );
    expect(() =>
      resolvePlan({
        event: "push",
        ref: "refs/tags/v1.2.0",
        versions: { ...versions, npm: "1.1.0" },
      }),
    ).toThrow(/Versions disagree/);
  });

  it("lets manual runs build drafts anywhere but beta/stable only from the tag", () => {
    expect(
      resolvePlan({
        event: "workflow_dispatch",
        ref: "refs/heads/main",
        channelInput: "draft",
        versions,
      }),
    ).toEqual({ channel: "draft", version: "1.2.0", tag: "", prerelease: true });
    expect(() =>
      resolvePlan({
        event: "workflow_dispatch",
        ref: "refs/heads/main",
        channelInput: "beta",
        versions,
      }),
    ).toThrow(/from tag v1.2.0/);
    expect(
      resolvePlan({
        event: "workflow_dispatch",
        ref: "refs/tags/v1.2.0",
        channelInput: "beta",
        versions,
      }),
    ).toMatchObject({ channel: "beta", tag: "v1.2.0" });
    const pre = { tauri: "2.0.0-rc.1", npm: "2.0.0-rc.1", cargo: "2.0.0-rc.1" };
    expect(() =>
      resolvePlan({
        event: "workflow_dispatch",
        ref: "refs/tags/v2.0.0-rc.1",
        channelInput: "stable",
        versions: pre,
      }),
    ).toThrow(/Stable releases need a release version/);
    expect(() =>
      resolvePlan({
        event: "workflow_dispatch",
        ref: "refs/tags/v1.2.0",
        channelInput: "nightly",
        versions,
      }),
    ).toThrow(/Unknown channel/);
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
    expect(sorted).toEqual([
      "0.9.9",
      "1.0.0-alpha",
      "1.0.0-alpha.1",
      "1.0.0-beta.2",
      "1.0.0-beta.11",
      "1.0.0",
      "1.0.1",
    ]);
    expect(() => compareVersions("1.0", "1.0.0")).toThrow(/semantic/);
  });
});

describe("signing secrets", () => {
  const all = (platform: string) =>
    Object.fromEntries(requiredSecrets(platform).map((name: string) => [name, "x"]));

  it("names every missing secret per platform and treats blanks as missing", () => {
    expect(missingSecrets("linux", {})).toEqual([]);
    expect(missingSecrets("macos", { ...all("macos"), APPLE_ID: "  " })).toEqual(["APPLE_ID"]);
    expect(missingSecrets("windows", all("windows"))).toEqual([]);
    expect(requiredSecrets("macos")).toEqual(
      expect.arrayContaining([
        "APPLE_CERTIFICATE",
        "APPLE_SIGNING_IDENTITY",
        "APPLE_TEAM_ID",
        "APPLE_PASSWORD",
      ]),
    );
    expect(() => requiredSecrets("beos")).toThrow(/Unknown platform/);
  });

  it("exports only secrets that are set", () => {
    expect(exportedNames("macos", { APPLE_CERTIFICATE: "" })).toEqual([]);
    expect(exportedNames("macos", { APPLE_ID: "a" })).toEqual(["APPLE_ID"]);
  });

  it("notarizes with an App Store Connect API key once one is set, and never exports its text", () => {
    expect(notarizationMethod({})).toBe("appleId");
    const apiKey = { APPLE_API_ISSUER: "i", APPLE_API_KEY: "ABC123", APPLE_API_PRIVATE_KEY: "k" };
    expect(notarizationMethod({ APPLE_API_KEY: "ABC123" })).toBe("apiKey");
    const cert = {
      APPLE_CERTIFICATE: "c",
      APPLE_CERTIFICATE_PASSWORD: "p",
      APPLE_SIGNING_IDENTITY: "s",
    };
    expect(missingSecrets("macos", { ...cert, ...apiKey })).toEqual([]);
    expect(missingSecrets("macos", { ...cert, APPLE_API_KEY: "ABC123" })).toEqual([
      "APPLE_API_ISSUER",
      "APPLE_API_PRIVATE_KEY",
    ]);
    expect(exportedNames("macos", { ...cert, ...apiKey })).not.toContain("APPLE_API_PRIVATE_KEY");
  });

  it("writes the API key where the Tauri CLI looks for it, readable only by the runner", () => {
    const dir = mkdtempSync(join(tmpdir(), "ixtable-signing-"));
    const path = writeApiKey({ APPLE_API_KEY: "ABC123", APPLE_API_PRIVATE_KEY: "pem" }, dir);
    expect(path).toBe(join(dir, "AuthKey_ABC123.p8"));
    expect(readFileSync(path, "utf8")).toBe("pem\n");
    if (process.platform !== "win32") expect(statSync(path).mode & 0o077).toBe(0);
    expect(writeApiKey({ APPLE_API_KEY: "ABC123" }, dir)).toBeUndefined();
    expect(() => writeApiKey({ APPLE_API_KEY: "../x", APPLE_API_PRIVATE_KEY: "pem" }, dir)).toThrow(
      /key ID/,
    );
  });

  it("signs Windows through Azure Trusted Signing only with all credentials", () => {
    const signed = tauriConfigOverride("windows", all("windows"));
    expect(signed.bundle.windows.signCommand.cmd).toBe("trusted-signing-cli");
    expect(signed.bundle.windows.signCommand.args.at(-1)).toBe("%1");
    const draft = tauriConfigOverride("windows", { AZURE_CLIENT_ID: "x" });
    expect(draft.bundle).toEqual({});
  });

  it("drops the local cloud origin from release CSPs and adds only an https cloud origin", () => {
    expect(tauriConfigOverride("linux", {}).app.security.csp["connect-src"]).toBe(
      "'self' ipc: http://ipc.localhost",
    );
    expect(releaseConnectSrc("https://abc.supabase.co/some/path")).toBe(
      "'self' ipc: http://ipc.localhost https://abc.supabase.co",
    );
    expect(() => releaseConnectSrc("http://abc.supabase.co")).toThrow(/https/);
  });
});

describe("content security policy", () => {
  const csp = conf.app.security.csp as Record<string, string>;
  it("allows scripts only from the app and never eval", () => {
    expect(csp["default-src"]).toBe("'self'");
    expect(csp["script-src"]).toBe("'self'");
    expect(csp["object-src"]).toBe("'none'");
    for (const [directive, value] of Object.entries({ ...csp, ...conf.app.security.devCsp })) {
      expect(value, directive).not.toMatch(
        /unsafe-eval|\*|https?:\/\/(?!127\.0\.0\.1|ipc\.localhost)/,
      );
    }
    expect(csp["connect-src"].split(" ")).toEqual(
      expect.arrayContaining(["'self'", "ipc:", "http://ipc.localhost"]),
    );
  });
});
