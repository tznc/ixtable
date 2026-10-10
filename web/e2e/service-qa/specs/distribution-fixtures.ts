/**
 * Seeding and call helpers for the distribution contract specs (apps,
 * invitations, members, publish, versions, bundle, backup, retention).
 * Archives are random bytes: the cloud never parses `.ixt` content, it only
 * stores bytes and checks size (and the desktop checks sha256).
 */
import { createHash, randomBytes, randomUUID } from "node:crypto";
import { callFunction, type FunctionResult, getServiceClient, localStack } from "../clients";
import type { TestUser } from "../seed";

export type Body = Record<string, unknown>;
export type ErrorBody = { error: { code: string; message: string; details?: Body } };

export const ARCHIVE_BUCKET = "app-archives";
/** A security summary that passes for a SQLite app. */
export const SQLITE_SECURITY = { store: "sqlite", tls: true, concurrencyPoliciesResolved: true };

export function call<T = Body>(
  name: string,
  user: TestUser | null,
  body: Body,
): Promise<FunctionResult<T>> {
  return callFunction<T>(name, { jwt: user?.jwt, body });
}

export function fakeArchive(size = 2048): Buffer {
  return randomBytes(size);
}

export function sha256(bytes: Buffer): string {
  return createHash("sha256").update(bytes).digest("hex");
}

export interface Uploaded {
  uploadId: string;
  path: string;
  signedUrl: string;
  bytes: Buffer;
  sha256: string;
  putStatus: number;
}

/** archive-upload-url, then PUT the bytes to the returned signed URL (as the desktop does). */
export async function uploadArchive(
  user: TestUser,
  appId: string,
  opts: {
    kind?: "version" | "backup";
    installationId?: string;
    bytes?: Buffer;
    declaredSize?: number;
  } = {},
): Promise<Uploaded> {
  const bytes = opts.bytes ?? fakeArchive();
  const digest = sha256(bytes);
  const result = await call<{ uploadId: string; path: string; signedUrl: string; token: string }>(
    "archive-upload-url",
    user,
    {
      appId,
      kind: opts.kind ?? "version",
      size: opts.declaredSize ?? bytes.length,
      sha256: digest,
      installationId: opts.installationId,
    },
  );
  if (result.status !== 200)
    throw new Error(`archive-upload-url ${result.status}: ${JSON.stringify(result.body)}`);
  const put = await fetch(result.body.signedUrl, {
    method: "PUT",
    headers: {
      apikey: localStack().anonKey,
      "content-type": "application/octet-stream",
      "x-upsert": "false",
    },
    body: new Uint8Array(bytes),
  });
  await put.text();
  return { ...result.body, bytes, sha256: digest, putStatus: put.status };
}

export interface PublishOptions {
  version: string;
  expectedHeadVersionId: string | null;
  security?: Body;
  migrations?: unknown[];
  uploadId?: string;
  bytes?: Buffer;
}

export interface VersionRow {
  id: string;
  app_id: string;
  version: string;
  status: string;
  archive_sha256: string;
  archive_size: number;
  storage_path: string;
  parent_version_id: string | null;
  resolution: string | null;
  security: Body;
  migrations: unknown[];
}

/** Uploads (unless uploadId is given) and calls publish-checkpoint. */
export async function publish(
  owner: TestUser,
  appId: string,
  opts: PublishOptions,
): Promise<{ result: FunctionResult<{ version: VersionRow } & ErrorBody>; upload?: Uploaded }> {
  const upload = opts.uploadId
    ? undefined
    : await uploadArchive(owner, appId, { bytes: opts.bytes });
  const result = await call<{ version: VersionRow } & ErrorBody>("publish-checkpoint", owner, {
    appId,
    uploadId: opts.uploadId ?? upload?.uploadId,
    version: opts.version,
    releaseNotes: `Release ${opts.version}`,
    minRuntimeVersion: "0.1.0",
    migrations: opts.migrations ?? [],
    security: opts.security ?? SQLITE_SECURITY,
    expectedHeadVersionId: opts.expectedHeadVersionId,
  });
  return { result, upload };
}

/** Publishes and asserts success; returns the version row. */
export async function publishOk(
  owner: TestUser,
  appId: string,
  opts: PublishOptions,
): Promise<VersionRow> {
  const { result } = await publish(owner, appId, opts);
  if (result.status !== 200)
    throw new Error(`publish ${result.status}: ${JSON.stringify(result.body)}`);
  return result.body.version;
}

/** A plan with a one-user allowance (inactive, so the website never lists it). */
export async function ensureSingleUserPlan(): Promise<string> {
  const { error } = await getServiceClient().from("plans").upsert(
    {
      id: "qa_single",
      name: "QA single user",
      price_cents: 0,
      annual_price_cents: 0,
      runtime_user_allowance: 1,
      storage_gb: 1,
      active: false,
      sort: 99,
    },
    { onConflict: "id" },
  );
  if (error) throw new Error(`ensureSingleUserPlan: ${error.message}`);
  return "qa_single";
}

export async function subscribe(appId: string, planId: string): Promise<void> {
  const { error } = await getServiceClient()
    .from("subscriptions")
    .upsert(
      {
        app_id: appId,
        plan_id: planId,
        provider: "fake",
        status: "active",
        current_period_end: new Date(Date.now() + 30 * 86_400_000).toISOString(),
      },
      { onConflict: "app_id" },
    );
  if (error) throw new Error(`subscribe: ${error.message}`);
}

/** Audit actions of an app, oldest first. */
export async function auditTrail(
  appId: string,
): Promise<{ action: string; actor_id: string | null; target: string | null; details: Body }[]> {
  const { data, error } = await getServiceClient()
    .from("audit_events")
    .select("action, actor_id, target, details, at")
    .eq("app_id", appId)
    .order("at", { ascending: true });
  if (error) throw new Error(`auditTrail: ${error.message}`);
  return data ?? [];
}

/** Whether an archive object exists in storage (service role). */
export async function objectExists(path: string): Promise<boolean> {
  const slash = path.lastIndexOf("/");
  const { data } = await getServiceClient()
    .storage.from(ARCHIVE_BUCKET)
    .list(path.slice(0, slash), { search: path.slice(slash + 1) });
  return (data ?? []).some((item) => item.name === path.slice(slash + 1));
}

/**
 * Removes every archive object recorded for these apps (versions, backups,
 * uploads). The fixture deletes rows; storage objects need this.
 */
export async function removeArchives(appIds: string[]): Promise<void> {
  if (appIds.length === 0) return;
  const admin = getServiceClient();
  const paths = new Set<string>();
  for (const table of ["app_versions", "installation_backups", "archive_uploads"]) {
    const { data } = await admin.from(table).select("storage_path").in("app_id", appIds);
    for (const row of data ?? []) paths.add(row.storage_path as string);
  }
  if (paths.size > 0) await admin.storage.from(ARCHIVE_BUCKET).remove([...paths]);
}

/** Tracks app ids of a test so `afterEach` can remove their archives. */
export class ArchiveTracker {
  private ids = new Set<string>();
  add(...ids: string[]): void {
    for (const id of ids) this.ids.add(id);
  }
  async cleanup(): Promise<void> {
    const ids = [...this.ids];
    this.ids.clear();
    await removeArchives(ids);
  }
}

export function newInstallationId(): string {
  return randomUUID();
}

/** Same canonical form the cloud signs (sorted keys, no whitespace). */
export function canonicalJson(value: unknown): string {
  if (value === null || typeof value !== "object") return JSON.stringify(value ?? null);
  if (Array.isArray(value)) return `[${value.map((item) => canonicalJson(item)).join(",")}]`;
  const entries = Object.entries(value as Record<string, unknown>)
    .filter(([, item]) => item !== undefined)
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
  return `{${entries.map(([key, item]) => `${JSON.stringify(key)}:${canonicalJson(item)}`).join(",")}}`;
}
