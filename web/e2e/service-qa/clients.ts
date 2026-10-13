/**
 * Supabase clients for service-qa. Keys come from env vars, then
 * `supabase status -o env`, then the standard local demo keys. Every URL is
 * checked to be local: service-qa never talks to a hosted project.
 */
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { createClient, type SupabaseClient } from "@supabase/supabase-js";

export const REPO_ROOT = join(__dirname, "..", "..", "..");
export const LOCAL_SUPABASE_URL = "http://127.0.0.1:54321";
/** Standard local demo anon JWT (`supabase start` default). */
export const LOCAL_ANON_KEY =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZS1kZW1vIiwicm9sZSI6ImFub24iLCJleHAiOjE5ODM4MTI5OTZ9.CRXP1A7WOeoJeXxjNni43kdQwgnWNReilDMblYTn_I0";
/** Standard local demo service_role JWT (`supabase start` default). */
export const LOCAL_SERVICE_ROLE_KEY =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZS1kZW1vIiwicm9sZSI6InNlcnZpY2Vfcm9sZSIsImV4cCI6MTk4MzgxMjk5Nn0.EGIM96RAZx35lJzdJsyH-qQwv8Hdp7fsn3W0YpN81IU";

export interface LocalStack {
  url: string;
  anonKey: string;
  serviceRoleKey: string;
}

const LOCAL_HOSTS = new Set(["127.0.0.1", "localhost", "[::1]", "::1", "host.docker.internal"]);

/** Throws unless `url` points at a local stack. */
export function assertLocalUrl(url: string): string {
  const { hostname } = new URL(url);
  if (!LOCAL_HOSTS.has(hostname)) {
    throw new Error(
      `service-qa refuses non-local Supabase URL ${url}. Use the local CLI stack only.`,
    );
  }
  return url.replace(/\/$/, "");
}

function fromStatus(): Partial<LocalStack> {
  try {
    const stdout = execFileSync("supabase", ["status", "-o", "env"], {
      cwd: REPO_ROOT,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
      timeout: 15_000,
    });
    const pick = (name: string) => new RegExp(`^${name}="?([^"\\n]+)"?$`, "m").exec(stdout)?.[1];
    return {
      url: pick("API_URL"),
      anonKey: pick("ANON_KEY"),
      serviceRoleKey: pick("SERVICE_ROLE_KEY"),
    };
  } catch {
    return {};
  }
}

let cached: LocalStack | null = null;

export function localStack(): LocalStack {
  if (cached) return cached;
  const status =
    process.env.SUPABASE_URL &&
    process.env.SUPABASE_ANON_KEY &&
    process.env.SUPABASE_SERVICE_ROLE_KEY
      ? {}
      : fromStatus();
  cached = {
    url: assertLocalUrl(process.env.SUPABASE_URL ?? status.url ?? LOCAL_SUPABASE_URL),
    anonKey: process.env.SUPABASE_ANON_KEY ?? status.anonKey ?? LOCAL_ANON_KEY,
    serviceRoleKey:
      process.env.SUPABASE_SERVICE_ROLE_KEY ?? status.serviceRoleKey ?? LOCAL_SERVICE_ROLE_KEY,
  };
  return cached;
}

const options = {
  auth: { autoRefreshToken: false, persistSession: false, detectSessionInUrl: false },
};

/** Anon client (no session). Sign in on it to act as a user. */
export function getAnonClient(): SupabaseClient {
  const { url, anonKey } = localStack();
  return createClient(url, anonKey, options);
}

/** Service-role client: seeding, teardown and verifying DB state only. */
export function getServiceClient(): SupabaseClient {
  const { url, serviceRoleKey } = localStack();
  return createClient(url, serviceRoleKey, options);
}

export function functionsUrl(name: string): string {
  return `${localStack().url}/functions/v1/${name}`;
}

export interface FunctionResult<T = unknown> {
  status: number;
  body: T;
  headers: Headers;
}

/**
 * Calls an Edge Function over HTTP like the desktop and website do. `jwt` is
 * the caller's access token (omit for unauthenticated calls; the anon key is
 * still sent as `apikey`).
 */
export async function callFunction<T = Record<string, unknown>>(
  name: string,
  init: { jwt?: string; body?: unknown; method?: string; headers?: Record<string, string> } = {},
): Promise<FunctionResult<T>> {
  const { anonKey } = localStack();
  const response = await fetch(functionsUrl(name), {
    method: init.method ?? "POST",
    headers: {
      apikey: anonKey,
      ...(init.jwt ? { Authorization: `Bearer ${init.jwt}` } : {}),
      ...(init.body === undefined ? {} : { "Content-Type": "application/json" }),
      ...init.headers,
    },
    body:
      init.body === undefined
        ? undefined
        : typeof init.body === "string"
          ? init.body
          : JSON.stringify(init.body),
  });
  const text = await response.text();
  let body: unknown = text;
  try {
    body = text ? JSON.parse(text) : null;
  } catch {
    // Non-JSON body stays a string.
  }
  return { status: response.status, body: body as T, headers: response.headers };
}

/**
 * Local Edge secrets from supabase/functions/.env.local (written by
 * scripts/cloud/dev-secrets.mjs), e.g. IXTABLE_CLOUD_PUBLIC_KEY or
 * STRIPE_WEBHOOK_SECRET for signing fake webhook events.
 */
export function devSecret(name: string): string {
  const file = join(REPO_ROOT, "supabase", "functions", ".env.local");
  if (!existsSync(file))
    throw new Error("Missing supabase/functions/.env.local. Run `npm run service-qa:up --prefix web`.");
  const match = new RegExp(`^${name}=(.*)$`, "m").exec(readFileSync(file, "utf8"));
  if (!match) throw new Error(`${name} is not set in supabase/functions/.env.local`);
  return match[1].trim();
}
