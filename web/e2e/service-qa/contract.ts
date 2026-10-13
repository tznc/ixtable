/**
 * Recorded Edge Function responses ("contract fixtures") for the desktop and
 * website clients. specs/contract.spec.ts calls every function the clients
 * use against the local stack and either records the sanitized exchange to
 * fixtures/contract/<name>.json (CONTRACT_RECORD=1) or checks that the live
 * response still has the recorded shape (keys and JSON value types), so a
 * server change that would break a client fails the contracts run.
 *
 * Consumers: web/e2e/service-qa/contract/website-types.ts (compile-time check
 * of web/src/lib/cloud FunctionMap) and the desktop decoders and parsers
 * tested in src-tauri/src/cloud/tests.rs.
 */
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { type FunctionResult, callFunction } from "./clients";
import type { TestUser } from "./seed";

export const CONTRACT_DIR = join(__dirname, "fixtures", "contract");
export const RECORDING = process.env.CONTRACT_RECORD === "1";

export interface ContractFixture {
  function: string;
  status: number;
  request: unknown;
  response: unknown;
}

const SECRET_KEYS =
  /^(dek|token|access_token|refresh_token|provider_token|signature|ciphertext|nonce|codeVerifier|codeChallenge|state|fingerprint)$/;
const URL_KEYS = /^(signedUrl|archiveUrl|acceptUrl|url)$/;
const JWT_LIKE = /eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}/g;

/** Replaces secret values with placeholders of the same JSON type. */
export function sanitize(value: unknown, key = ""): unknown {
  if (typeof value === "string") {
    if (SECRET_KEYS.test(key)) return "[redacted]";
    if (URL_KEYS.test(key)) {
      try {
        const url = new URL(value);
        for (const name of Array.from(url.searchParams.keys()))
          if (/token|session|sig/i.test(name)) url.searchParams.set(name, "redacted");
        return url.toString();
      } catch {
        return value;
      }
    }
    return value.replace(JWT_LIKE, "[redacted-jwt]");
  }
  if (Array.isArray(value)) return value.map((item) => sanitize(item, key));
  if (value === null || typeof value !== "object") return value;
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>).map(([k, v]) => [k, sanitize(v, k)]),
  );
}

/** JSON shape: keys and value types; arrays are described by their first item. */
export function shape(value: unknown): unknown {
  if (value === null) return "null";
  if (Array.isArray(value)) return value.length === 0 ? [] : [shape(value[0])];
  if (typeof value === "object")
    return Object.fromEntries(
      Object.keys(value as object)
        .sort()
        .map((key) => [key, shape((value as Record<string, unknown>)[key])]),
    );
  return typeof value;
}

/** Paths where `live` differs in shape from `recorded` (empty when they agree). */
export function shapeDiff(recorded: unknown, live: unknown, path = "$"): string[] {
  const a = shape(recorded);
  const b = shape(live);
  if (JSON.stringify(a) === JSON.stringify(b)) return [];
  if (
    a &&
    b &&
    typeof a === "object" &&
    typeof b === "object" &&
    !Array.isArray(a) &&
    !Array.isArray(b)
  ) {
    const out: string[] = [];
    const ra = recorded as Record<string, unknown>;
    const lb = live as Record<string, unknown>;
    for (const key of new Set([...Object.keys(ra), ...Object.keys(lb)])) {
      if (!(key in lb)) out.push(`${path}.${key} missing in live response`);
      else if (!(key in ra)) out.push(`${path}.${key} is new in live response`);
      else out.push(...shapeDiff(ra[key], lb[key], `${path}.${key}`));
    }
    return out;
  }
  if (Array.isArray(recorded) && Array.isArray(live) && recorded.length && live.length)
    return shapeDiff(recorded[0], live[0], `${path}[0]`);
  return [`${path}: recorded ${JSON.stringify(a)}, live ${JSON.stringify(b)}`];
}

export function readContract(name: string): ContractFixture | null {
  const file = join(CONTRACT_DIR, `${name}.json`);
  return existsSync(file) ? (JSON.parse(readFileSync(file, "utf8")) as ContractFixture) : null;
}

/**
 * Records (CONTRACT_RECORD=1) or verifies one exchange. Returns the shape
 * differences against the committed fixture (always empty when recording).
 */
export function checkContract(
  name: string,
  fn: string,
  request: unknown,
  result: FunctionResult<unknown>,
): string[] {
  const fixture: ContractFixture = {
    function: fn,
    status: result.status,
    request: sanitize(request),
    response: sanitize(result.body),
  };
  if (RECORDING) {
    mkdirSync(CONTRACT_DIR, { recursive: true });
    writeFileSync(join(CONTRACT_DIR, `${name}.json`), `${JSON.stringify(fixture, null, 2)}\n`);
    return [];
  }
  const recorded = readContract(name);
  if (!recorded) return [`${name}: no fixture (run with CONTRACT_RECORD=1)`];
  const diff = shapeDiff(recorded.response, fixture.response).map((d) => `${name}: ${d}`);
  if (recorded.status !== fixture.status)
    diff.push(`${name}: status recorded ${recorded.status}, live ${fixture.status}`);
  return diff;
}

/** Calls `fn` as `user` and records or verifies the exchange under `name`. */
export class ContractRecorder {
  readonly drift: string[] = [];
  readonly names: string[] = [];

  async call<T = Record<string, unknown>>(
    name: string,
    fn: string,
    user: TestUser | null,
    body: Record<string, unknown>,
  ): Promise<FunctionResult<T>> {
    const result = await callFunction<T>(fn, { jwt: user?.jwt, body });
    this.names.push(name);
    this.drift.push(...checkContract(name, fn, body, result as FunctionResult<unknown>));
    return result;
  }
}
