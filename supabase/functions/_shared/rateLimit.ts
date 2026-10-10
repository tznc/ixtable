// Abuse controls and service counters (PRD Phase 5).
import { serviceClient } from "./db.ts";
import { HttpError } from "./http.ts";

/** True while `bucket` has had at most `max` hits in the current window. */
export async function rateLimit(
  bucket: string,
  max: number,
  windowSeconds: number,
): Promise<boolean> {
  const { data, error } = await serviceClient().rpc("rate_limit", {
    p_bucket: bucket,
    p_max: max,
    p_window_seconds: windowSeconds,
  });
  if (error) throw new Error(`rate_limit failed: ${error.message}`);
  return data === true;
}

/** Throws 429 RATE_LIMITED when the bucket is exhausted. Bucket e.g. `key-grant:${userId}`. */
export async function enforceRateLimit(
  bucket: string,
  max: number,
  windowSeconds: number,
): Promise<void> {
  if (!(await rateLimit(bucket, max, windowSeconds))) {
    throw new HttpError("RATE_LIMITED", "Too many requests. Try again later.", {
      retryAfterSeconds: windowSeconds,
    });
  }
}

/** Best-effort daily counter in service_metrics (never throws). */
export async function incrementMetric(name: string, by = 1): Promise<void> {
  try {
    await serviceClient().rpc("metric_increment", { p_name: name, p_by: by });
  } catch {
    // Metrics must never fail a request.
  }
}

/**
 * Per-endpoint limits for sensitive functions (PRD Phase 5 abuse controls).
 * One table so operations can review and tune them in one place
 * (docs/ops/monitoring.md). Use `enforceNamedRateLimit(name, subject)`;
 * the bucket is `<name>:<subject>` where subject is usually the caller's
 * user id (or the PKCE state / IP hash for unauthenticated endpoints).
 */
export const RATE_LIMITS = {
  // Distribution
  "apps-create": { max: 20, windowSeconds: 3600 },
  "apps-delete": { max: 10, windowSeconds: 3600 },
  "apps-transfer": { max: 10, windowSeconds: 3600 },
  "roles-sync": { max: 60, windowSeconds: 3600 },
  "invitations-create": { max: 30, windowSeconds: 3600 },
  "invitations-accept": { max: 20, windowSeconds: 600 },
  "members-update": { max: 120, windowSeconds: 3600 },
  "app-access-update": { max: 120, windowSeconds: 3600 },
  "archive-upload-url": { max: 60, windowSeconds: 3600 },
  "publish-checkpoint": { max: 30, windowSeconds: 3600 },
  "versions-resolve": { max: 20, windowSeconds: 3600 },
  "bundle-manifest": { max: 60, windowSeconds: 3600 },
  "restore-url": { max: 60, windowSeconds: 3600 },
  "sync-check": { max: 240, windowSeconds: 3600 },
  "backup-commit": { max: 60, windowSeconds: 3600 },
  // Credentials and desktop sign-in. key-grant's subject is `<userId>:<appId>`;
  // desktop-auth-exchange's is `ip:<ipHash>` (no caller JWT).
  "credential-envelope": { max: 60, windowSeconds: 3600 },
  "credential-delete": { max: 60, windowSeconds: 3600 },
  "key-grant": { max: 30, windowSeconds: 3600 },
  "devices-revoke": { max: 120, windowSeconds: 3600 },
  "desktop-auth-approve": { max: 20, windowSeconds: 600 },
  "desktop-auth-exchange": { max: 120, windowSeconds: 60 },
  // Commercial
  "billing-checkout": { max: 10, windowSeconds: 600 },
  "billing-fake-complete": { max: 10, windowSeconds: 600 },
  "billing-portal": { max: 20, windowSeconds: 600 },
  "billing-invoices": { max: 60, windowSeconds: 600 },
  "billing-cancel": { max: 10, windowSeconds: 600 },
  "account-export": { max: 5, windowSeconds: 3600 },
  "account-delete": { max: 5, windowSeconds: 3600 },
  "admin-support": { max: 120, windowSeconds: 3600 },
} as const satisfies Record<string, { max: number; windowSeconds: number }>;

export type RateLimitName = keyof typeof RATE_LIMITS;

/** The bucket key used for `name` and `subject` (exported for tests and support). */
export function rateLimitBucket(name: RateLimitName, subject: string): string {
  return `${name}:${subject}`;
}

/** Throws 429 RATE_LIMITED when `subject` exceeded the RATE_LIMITS entry for `name`. */
export async function enforceNamedRateLimit(name: RateLimitName, subject: string): Promise<void> {
  const { max, windowSeconds } = RATE_LIMITS[name];
  await enforceRateLimit(rateLimitBucket(name, subject), max, windowSeconds);
}
