// Shared code for the distribution Edge Functions (apps, roles, invitations,
// members, uploads, publishing, versions, bundles, sync, backups, retention).
// Pure helpers come first (unit tested in distribution_test.ts); the database
// and storage helpers after them use the service role and must only run once
// the caller is authorized. Contract: docs/decisions/cloud-architecture.md.
import { ARCHIVE_BUCKET, serviceClient } from "./db.ts";
import { HttpError } from "./http.ts";
import { arr, semver, str, uuid } from "./validate.ts";

export const BUNDLE_FORMAT = "ixtable-cloud-bundle/1";
/** Signed archive download URLs (bundle and restore) live this long. */
export const DOWNLOAD_URL_TTL_SECONDS = 900;
/** A bundle manifest may be verified for this long after issue. */
export const MANIFEST_TTL_SECONDS = 86_400;
export const PG_RESTORE_WARNING =
  "Restoring this archive does not restore external PostgreSQL records.";

// Paths ---------------------------------------------------------------------

/** Developer stream: `apps/<appId>/versions/<versionId>.ixt`. */
export function versionPath(appId: string, versionId: string): string {
  return `apps/${appId}/versions/${versionId}.ixt`;
}

/** Installation stream: `apps/<appId>/installations/<userId>/<installationId>/<backupId>.ixt`. */
export function backupPath(
  appId: string,
  userId: string,
  installationId: string,
  backupId: string,
): string {
  return `apps/${appId}/installations/${userId}/${installationId}/${backupId}.ixt`;
}

// Semver --------------------------------------------------------------------

const SEMVER = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/;

/** Semver 2.0 precedence: <0, 0 or >0. Build metadata is ignored. Throws on invalid input. */
export function compareSemver(a: string, b: string): number {
  const left = SEMVER.exec(a);
  const right = SEMVER.exec(b);
  if (!left || !right) throw new Error(`invalid semver: ${left ? b : a}`);
  for (let i = 1; i <= 3; i++) {
    const diff = BigInt(left[i]) - BigInt(right[i]);
    if (diff !== 0n) return diff < 0n ? -1 : 1;
  }
  const pa = left[4];
  const pb = right[4];
  if (pa === undefined || pb === undefined) {
    // A release outranks any prerelease of the same version.
    return pa === pb ? 0 : pa === undefined ? 1 : -1;
  }
  const ia = pa.split(".");
  const ib = pb.split(".");
  for (let i = 0; i < Math.max(ia.length, ib.length); i++) {
    if (ia[i] === undefined) return -1;
    if (ib[i] === undefined) return 1;
    const na = /^\d+$/.test(ia[i]);
    const nb = /^\d+$/.test(ib[i]);
    if (na && nb) {
      const diff = BigInt(ia[i]) - BigInt(ib[i]);
      if (diff !== 0n) return diff < 0n ? -1 : 1;
    } else if (na !== nb) {
      return na ? -1 : 1;
    } else if (ia[i] !== ib[i]) {
      return ia[i] < ib[i] ? -1 : 1;
    }
  }
  return 0;
}

// Security summary (PRD §19, §21.3, §21.4) ----------------------------------

export interface SecurityContext {
  /** The app (or the summary) uses an external PostgreSQL datasource. */
  postgres: boolean;
  /** More than one Runtime User can use the app (members or plan allowance). */
  concurrencyRequired: boolean;
}

export interface SecuritySummary {
  store: "sqlite" | "postgres";
  credentialMode: "shared" | "perUser" | null;
  tls: boolean;
  sslmode: string | null;
  insecureTransportConfirmed: boolean;
  insecureTransportConfirmedAt: string | null;
  sharedCredentialAcknowledged: boolean;
  concurrencyPoliciesResolved: boolean;
  unresolvedEntities: string[];
}

function invalid(field: string, message: string): never {
  throw new HttpError("VALIDATION", `security.${field}: ${message}`, {
    field: `security.${field}`,
  });
}

function flag(raw: Record<string, unknown>, ...keys: string[]): boolean {
  return keys.some((key) => raw[key] === true);
}

/**
 * Validates the publish security summary and returns the normalized form
 * stored on the version. Accepts the desktop preflight names as aliases
 * (insecureOverrideConfirmed, sharedCredentialWarningAcknowledged,
 * entityPoliciesResolved). Throws VALIDATION when a severe warning is not
 * confirmed or concurrency policies are unresolved.
 */
export function normalizeSecurity(raw: unknown, ctx: SecurityContext): SecuritySummary {
  if (raw === null || typeof raw !== "object" || Array.isArray(raw))
    throw new HttpError("VALIDATION", "security: must be an object", { field: "security" });
  const s = raw as Record<string, unknown>;
  if (typeof s.tls !== "boolean") invalid("tls", "must be true or false");
  if (s.store !== undefined && s.store !== null && s.store !== "sqlite" && s.store !== "postgres")
    invalid("store", "must be sqlite or postgres");
  const postgres = ctx.postgres || s.store === "postgres";
  const mode = s.credentialMode;
  if (
    mode !== undefined &&
    mode !== null &&
    !["shared", "perUser", "user"].includes(mode as string)
  )
    invalid("credentialMode", "must be shared or perUser");
  const credentialMode = postgres
    ? mode === "perUser" || mode === "user"
      ? "perUser"
      : "shared"
    : null;

  const insecureTransportConfirmed = flag(
    s,
    "insecureTransportConfirmed",
    "insecureOverrideConfirmed",
  );
  if (!s.tls && !insecureTransportConfirmed)
    invalid(
      "insecureTransportConfirmed",
      "a datasource without TLS needs the Developer's explicit confirmation",
    );
  const sharedCredentialAcknowledged = flag(
    s,
    "sharedCredentialAcknowledged",
    "sharedCredentialWarningAcknowledged",
  );
  if (credentialMode === "shared" && !sharedCredentialAcknowledged)
    invalid(
      "sharedCredentialAcknowledged",
      "a shared PostgreSQL credential needs the Developer's acknowledgement",
    );
  const concurrencyPoliciesResolved = flag(
    s,
    "concurrencyPoliciesResolved",
    "entityPoliciesResolved",
  );
  if (ctx.concurrencyRequired && !concurrencyPoliciesResolved)
    invalid(
      "concurrencyPoliciesResolved",
      "every entity exposed to multiple Runtime Users needs a resolved concurrency policy",
    );
  const confirmedAt = s.insecureTransportConfirmedAt ?? s.insecureOverrideConfirmedAt;
  const unresolved = Array.isArray(s.unresolvedEntities)
    ? s.unresolvedEntities.filter((item): item is string => typeof item === "string").slice(0, 200)
    : [];
  return {
    store: postgres ? "postgres" : "sqlite",
    credentialMode,
    tls: s.tls as boolean,
    sslmode: typeof s.sslmode === "string" ? s.sslmode.slice(0, 40) : null,
    insecureTransportConfirmed: !s.tls && insecureTransportConfirmed,
    insecureTransportConfirmedAt:
      !s.tls && typeof confirmedAt === "string" ? confirmedAt.slice(0, 40) : null,
    sharedCredentialAcknowledged: credentialMode === "shared" && sharedCredentialAcknowledged,
    concurrencyPoliciesResolved,
    unresolvedEntities: unresolved,
  };
}

/** Migrations listed on a version: ids (strings) or `{id, name?, ...}` objects. */
export function normalizeMigrations(raw: unknown[]): unknown[] {
  return raw.map((item, index) => {
    if (typeof item === "string" && item.length > 0 && item.length <= 200) return item;
    if (item && typeof item === "object" && !Array.isArray(item)) {
      const id = (item as Record<string, unknown>).id;
      if (typeof id === "string" && id.length > 0 && id.length <= 200) {
        const json = JSON.stringify(item);
        if (json.length > 20_000)
          throw new HttpError("VALIDATION", `migrations[${index}]: is too large`, {
            field: "migrations",
          });
        return item;
      }
    }
    throw new HttpError("VALIDATION", `migrations[${index}]: must be an id or {id, name}`, {
      field: "migrations",
    });
  });
}

// Retention -----------------------------------------------------------------

export interface RetentionItem {
  id: string;
  createdAt: string;
  /** Never expire (head version, a version installed somewhere, newest backup). */
  protected?: boolean;
}

/**
 * Ids beyond the newest `keep` items or older than `days` (when set),
 * skipping protected items. Order of `items` does not matter.
 */
export function expiredIds(
  items: RetentionItem[],
  policy: { keep: number; days: number | null; now?: Date },
): string[] {
  const now = (policy.now ?? new Date()).getTime();
  const cutoff = policy.days === null ? null : now - policy.days * 86_400_000;
  return [...items]
    .sort((a, b) => Date.parse(b.createdAt) - Date.parse(a.createdAt))
    .filter(
      (item, rank) =>
        !item.protected &&
        (rank >= policy.keep || (cutoff !== null && Date.parse(item.createdAt) < cutoff)),
    )
    .map((item) => item.id);
}

// Manifest ------------------------------------------------------------------

export interface ManifestInput {
  app: { id: string; name: string };
  version: {
    id: string;
    version: string;
    archive_sha256: string;
    archive_size: number;
    min_runtime_version: string;
  };
  userId: string;
  role: { id: string; name: string; permissions: unknown } | null;
  /** The caller is the app's Developer/Owner (no runtime role; full access). */
  owner: boolean;
  installationId: string;
  fingerprint: string;
  issuedAt: Date;
}

/** The personalized bundle manifest (PLAN-CLOUD "Personalized bundles"). */
export function buildManifest(input: ManifestInput): Record<string, unknown> {
  return {
    format: BUNDLE_FORMAT,
    appId: input.app.id,
    appName: input.app.name,
    versionId: input.version.id,
    version: input.version.version,
    archiveSha256: input.version.archive_sha256,
    archiveSize: Number(input.version.archive_size),
    minRuntimeVersion: input.version.min_runtime_version,
    userId: input.userId,
    roleId: input.role?.id ?? null,
    roleName: input.role?.name ?? null,
    rolePermissions: input.role?.permissions ?? null,
    // Signed: the desktop grants developer access only when this is true; a
    // null role without it allows nothing (fail closed).
    owner: input.owner,
    installationId: input.installationId,
    fingerprint: input.fingerprint,
    issuedAt: input.issuedAt.toISOString(),
    expiresAt: new Date(input.issuedAt.getTime() + MANIFEST_TTL_SECONDS * 1000).toISOString(),
  };
}

// Errors and URLs -------------------------------------------------------------

export interface DbError {
  code?: string;
  message: string;
  details?: string | null;
}

/** Maps the IXnnn SQLSTATEs raised by the distribution_* SQL functions. */
export function mapDbError(error: DbError): HttpError | Error {
  switch (error.code) {
    case "IX404":
      return new HttpError("NOT_FOUND", error.message);
    case "IX409":
      return new HttpError("VERSION_CONFLICT", error.message, {
        headVersionId: error.details ? error.details : null,
      });
    case "IX410":
    case "IX422":
      return new HttpError("VALIDATION", error.message);
    case "IX423":
      return new HttpError("VALIDATION", error.message, {
        requiresConfirm: true,
        installations: Number(error.details ?? 0),
      });
    case "IX403":
      return new HttpError("FORBIDDEN", error.message);
    case "IX402": {
      const reason = error.message;
      return new HttpError("ENTITLEMENT_REQUIRED", entitlementText(reason), { reason });
    }
    default:
      return new Error(`database error ${error.code ?? ""}: ${error.message}`);
  }
}

function entitlementText(reason: string): string {
  switch (reason) {
    case "over_allowance":
      return "The plan's runtime-user allowance is used up.";
    case "no_subscription":
      return "This app needs an active ixtable Cloud plan.";
    case "subscription_inactive":
      return "The app's subscription is not active.";
    default:
      return "This app is not entitled to this operation.";
  }
}

/** Public API origin of the local CLI stack (Kong on the host). */
export const LOCAL_PUBLIC_URL = "http://127.0.0.1:54321";

function isLocalHost(hostname: string): boolean {
  return (
    ["localhost", "127.0.0.1", "::1", "[::1]", "kong", "host.docker.internal"].includes(hostname) ||
    !hostname.includes(".")
  );
}

/**
 * Rewrites a storage URL minted inside the Edge runtime to the public API
 * origin. The origin comes only from configuration, never from request
 * headers (a forged x-forwarded-host would hand out links to another host):
 * `IXTABLE_PUBLIC_API_URL` when set; otherwise, only when `SUPABASE_URL` is a
 * local stack, the CLI default http://127.0.0.1:54321. A hosted project
 * without `IXTABLE_PUBLIC_API_URL` keeps the URL Storage minted (already
 * public); an internal URL there is a misconfiguration and throws.
 */
export function publicUrl(url: string, env = Deno.env.toObject()): string {
  const target = new URL(url);
  let origin: string | null = env.IXTABLE_PUBLIC_API_URL
    ? new URL(env.IXTABLE_PUBLIC_API_URL).origin
    : null;
  if (!origin) {
    const internal = env.SUPABASE_URL ? new URL(env.SUPABASE_URL) : null;
    if (internal && isLocalHost(internal.hostname)) origin = LOCAL_PUBLIC_URL;
    else if (isLocalHost(target.hostname))
      throw new Error("IXTABLE_PUBLIC_API_URL must be set: storage minted an internal URL");
  }
  if (!origin) return url;
  return new URL(target.pathname + target.search, origin).toString();
}

// Database and storage (service role) ----------------------------------------

export interface AppRow {
  id: string;
  org_id: string;
  owner_id: string;
  name: string;
  document_id: string;
  datasource_kind: "sqlite" | "postgres";
  backups_enabled: boolean;
  retention_versions: number;
  retention_days: number | null;
  head_version_id: string | null;
  created_at: string;
  updated_at: string;
  deleted_at: string | null;
}

export interface VersionRow {
  id: string;
  app_id: string;
  version: string;
  developer_id: string;
  created_at: string;
  archive_sha256: string;
  archive_size: number;
  storage_path: string;
  migrations: unknown[];
  min_runtime_version: string;
  security: Record<string, unknown>;
  release_notes: string;
  status: "pending" | "published" | "withdrawn";
  parent_version_id: string | null;
  resolution: "overwrite" | "fork" | null;
  published_at: string | null;
}

export interface UploadRow {
  id: string;
  app_id: string;
  user_id: string;
  kind: "version" | "backup";
  installation_id: string | null;
  storage_path: string;
  expected_size: number;
  expected_sha256: string;
  status: string;
  expires_at: string;
}

function must<T>(result: { data: T | null; error: DbError | null }, what: string): T | null {
  if (result.error) throw new Error(`${what}: ${result.error.message}`);
  return result.data;
}

/** Loads a live app (404 when missing or soft-deleted). */
export async function loadApp(appId: string): Promise<AppRow> {
  const app = must(
    await serviceClient().from("cloud_apps").select("*").eq("id", appId).maybeSingle(),
    "load app",
  ) as AppRow | null;
  if (!app || app.deleted_at) throw new HttpError("NOT_FOUND", "App not found");
  return app;
}

export async function orgRole(orgId: string, userId: string): Promise<string | null> {
  const row = must(
    await serviceClient()
      .from("org_members")
      .select("role")
      .eq("org_id", orgId)
      .eq("user_id", userId)
      .maybeSingle(),
    "load org membership",
  ) as { role: string } | null;
  return row?.role ?? null;
}

/** Owner, org owner/admin, or per-app admin (same rule as SQL `is_app_admin`). */
export async function isAppAdmin(app: AppRow, userId: string): Promise<boolean> {
  if (app.owner_id === userId) return true;
  return (await appCapabilitiesFor(app.id, userId)).includes("admin");
}

/** The user's capabilities on a live app (SQL `app_capabilities_for`). */
export async function appCapabilitiesFor(appId: string, userId: string): Promise<string[]> {
  const result = await serviceClient().rpc("app_capabilities_for", {
    p_app_id: appId,
    p_user_id: userId,
  });
  return (must(
    result as { data: string[] | null; error: DbError | null },
    "load app capabilities",
  ) ?? []) as string[];
}

export async function requireAppAdmin(app: AppRow, userId: string): Promise<void> {
  if (!(await isAppAdmin(app, userId)))
    throw new HttpError(
      "FORBIDDEN",
      "Only the app's Developer, an organization admin or an app admin can do this",
    );
}

/** The single Developer/Owner of the app (PRD §20.1). */
export function requireAppOwner(app: AppRow, userId: string): void {
  if (app.owner_id !== userId)
    throw new HttpError("FORBIDDEN", "Only the app's Developer/Owner can do this");
}

export interface MemberRow {
  app_id: string;
  user_id: string;
  role_id: string;
  status: "active" | "revoked";
}

/**
 * The caller's runtime access: owner, or an active member. A revoked member
 * gets 403 REVOKED, anyone else 403 FORBIDDEN.
 */
export async function requireRuntimeAccess(
  app: AppRow,
  userId: string,
): Promise<{ owner: boolean; member: MemberRow | null }> {
  if (app.owner_id === userId) return { owner: true, member: null };
  const member = must(
    await serviceClient()
      .from("app_members")
      .select("app_id, user_id, role_id, status")
      .eq("app_id", app.id)
      .eq("user_id", userId)
      .maybeSingle(),
    "load membership",
  ) as MemberRow | null;
  if (!member) throw new HttpError("FORBIDDEN", "You are not a Runtime User of this app");
  if (member.status !== "active")
    throw new HttpError("REVOKED", "Your access to this app was revoked");
  return { owner: false, member };
}

export interface InstallationRow {
  id: string;
  app_id: string;
  user_id: string;
  device_name: string;
  installed_version_id: string | null;
  revoked_at: string | null;
}

/**
 * Registers (or touches) the caller's installation. 403 FORBIDDEN when the id
 * belongs to another user or app, 403 REVOKED when it was revoked.
 */
export async function touchInstallation(
  app: AppRow,
  userId: string,
  installationId: string,
  deviceName?: string,
): Promise<InstallationRow> {
  const db = serviceClient();
  const existing = must(
    await db.from("installations").select("*").eq("id", installationId).maybeSingle(),
    "load installation",
  ) as InstallationRow | null;
  if (existing) {
    if (existing.app_id !== app.id || existing.user_id !== userId)
      throw new HttpError("FORBIDDEN", "This installation belongs to another user or app");
    if (existing.revoked_at) throw new HttpError("REVOKED", "This installation was revoked");
    const patch: Record<string, unknown> = { last_seen_at: new Date().toISOString() };
    if (deviceName !== undefined) patch.device_name = deviceName;
    return must(
      await db.from("installations").update(patch).eq("id", installationId).select().single(),
      "touch installation",
    ) as InstallationRow;
  }
  const inserted = await db
    .from("installations")
    .insert({
      id: installationId,
      app_id: app.id,
      user_id: userId,
      device_name: deviceName ?? "",
    })
    .select()
    .single();
  if (inserted.error?.code === "23505")
    throw new HttpError("FORBIDDEN", "This installation belongs to another user or app");
  return must(inserted, "register installation") as InstallationRow;
}

/** Newest published version: the head when published, else the latest published one. */
export async function latestPublished(app: AppRow): Promise<VersionRow | null> {
  const db = serviceClient();
  if (app.head_version_id) {
    const head = must(
      await db.from("app_versions").select("*").eq("id", app.head_version_id).maybeSingle(),
      "load head",
    ) as VersionRow | null;
    if (head?.status === "published") return head;
  }
  return must(
    await db
      .from("app_versions")
      .select("*")
      .eq("app_id", app.id)
      .eq("status", "published")
      .order("published_at", { ascending: false })
      .limit(1)
      .maybeSingle(),
    "load latest version",
  ) as VersionRow | null;
}

/** Loads a pending upload row of the caller. 404/422 when unusable. */
export async function loadPendingUpload(
  uploadId: string,
  expect: { appId: string; userId: string; kind: "version" | "backup" },
): Promise<UploadRow> {
  const upload = must(
    await serviceClient().from("archive_uploads").select("*").eq("id", uploadId).maybeSingle(),
    "load upload",
  ) as UploadRow | null;
  if (!upload || upload.app_id !== expect.appId || upload.user_id !== expect.userId)
    throw new HttpError("NOT_FOUND", "Upload not found");
  if (upload.kind !== expect.kind)
    throw new HttpError("VALIDATION", `uploadId: is not a ${expect.kind} upload`, {
      field: "uploadId",
    });
  if (upload.status !== "pending" || Date.parse(upload.expires_at) <= Date.now())
    throw new HttpError("VALIDATION", "uploadId: the upload is expired or already used", {
      field: "uploadId",
    });
  return upload;
}

/** Size of a stored archive object, or null when it does not exist. */
export async function objectSize(path: string): Promise<number | null> {
  const bucket = serviceClient().storage.from(ARCHIVE_BUCKET);
  const slash = path.lastIndexOf("/");
  const { data, error } = await bucket.list(path.slice(0, slash), {
    search: path.slice(slash + 1),
    limit: 10,
  });
  if (error) throw new Error(`storage list failed: ${error.message}`);
  const entry = (data ?? []).find((item) => item.name === path.slice(slash + 1));
  if (!entry) return null;
  const size = Number((entry.metadata as Record<string, unknown> | null)?.size);
  return Number.isFinite(size) ? size : null;
}

/** Throws VALIDATION unless the uploaded object exists with the declared size. */
export async function verifyUploadedObject(upload: UploadRow): Promise<void> {
  const size = await objectSize(upload.storage_path);
  if (size === null)
    throw new HttpError("VALIDATION", "uploadId: the archive was not uploaded", {
      field: "uploadId",
    });
  if (size !== Number(upload.expected_size)) {
    await serviceClient()
      .from("archive_uploads")
      .update({ status: "failed" })
      .eq("id", upload.id)
      .eq("status", "pending");
    throw new HttpError("VALIDATION", "uploadId: the uploaded size does not match", {
      field: "uploadId",
      expectedSize: Number(upload.expected_size),
      actualSize: size,
    });
  }
}

/** Short-lived signed download URL on the public API origin. */
export async function signedDownloadUrl(
  path: string,
  req: Request,
  ttlSeconds = DOWNLOAD_URL_TTL_SECONDS,
): Promise<{ url: string; expiresAt: string }> {
  const { data, error } = await serviceClient()
    .storage.from(ARCHIVE_BUCKET)
    .createSignedUrl(path, ttlSeconds);
  if (error || !data) throw new Error(`createSignedUrl failed: ${error?.message ?? "no url"}`);
  return {
    url: publicUrl(data.signedUrl),
    expiresAt: new Date(Date.now() + ttlSeconds * 1000).toISOString(),
  };
}

/** Calls a distribution_* SQL function; maps IXnnn errors to the API contract. */
export async function rpc<T>(name: string, args: Record<string, unknown>): Promise<T> {
  const { data, error } = await serviceClient().rpc(name, args);
  if (error) throw mapDbError(error);
  return data as T;
}

/** Active Runtime Users of an app. */
export async function activeMemberCount(appId: string): Promise<number> {
  const { count, error } = await serviceClient()
    .from("app_members")
    .select("user_id", { count: "exact", head: true })
    .eq("app_id", appId)
    .eq("status", "active");
  if (error) throw new Error(`count members: ${error.message}`);
  return count ?? 0;
}

// Publishing (publish-checkpoint and versions-resolve) -------------------------

export interface PublishInput {
  uploadId: string;
  version: string;
  releaseNotes: string;
  minRuntimeVersion: string;
  migrations: unknown[];
  security: unknown;
}

export interface PreparedPublish {
  upload: UploadRow;
  security: SecuritySummary;
  migrations: unknown[];
  head: VersionRow | null;
}

/**
 * Checks everything a publish needs except the head precondition (the SQL
 * function re-checks that under a lock): the caller's pending version upload
 * of `uploadAppId` exists in storage with the declared size, the version is
 * strictly greater than the head's, and the security summary passes.
 */
export async function preparePublish(
  app: AppRow,
  input: PublishInput,
  userId: string,
  options: { uploadAppId?: string; allowance: number },
): Promise<PreparedPublish> {
  const upload = await loadPendingUpload(input.uploadId, {
    appId: options.uploadAppId ?? app.id,
    userId,
    kind: "version",
  });
  const head = app.head_version_id
    ? ((
        await serviceClient()
          .from("app_versions")
          .select("*")
          .eq("id", app.head_version_id)
          .maybeSingle()
      ).data as VersionRow | null)
    : null;
  if (head && compareSemver(input.version, head.version) <= 0)
    throw new HttpError(
      "VALIDATION",
      `version: must be greater than the published head ${head.version}`,
      { field: "version", headVersion: head.version },
    );
  const members = await activeMemberCount(app.id);
  const security = normalizeSecurity(input.security, {
    postgres: app.datasource_kind === "postgres",
    concurrencyRequired: members > 1 || options.allowance > 1,
  });
  const migrations = normalizeMigrations(input.migrations);
  await verifyUploadedObject(upload);
  return { upload, security, migrations, head };
}

/** Reads the publish fields shared by publish-checkpoint and versions-resolve. */
export function readPublishInput(body: Record<string, unknown>): PublishInput {
  const minRuntime = str(body, "minRuntimeVersion", { optional: true, max: 100 });
  if (minRuntime !== undefined && !SEMVER.test(minRuntime))
    throw new HttpError("VALIDATION", "minRuntimeVersion: must be a semantic version", {
      field: "minRuntimeVersion",
    });
  if (body.security === undefined || body.security === null)
    throw new HttpError("VALIDATION", "security: is required", { field: "security" });
  return {
    uploadId: uuid(body, "uploadId"),
    version: semver(body, "version"),
    releaseNotes: str(body, "releaseNotes", { optional: true, min: 0, max: 20_000 }) ?? "",
    minRuntimeVersion: minRuntime ?? "0.0.0",
    migrations: arr(body, "migrations", { optional: true, max: 1000 }) ?? [],
    security: body.security,
  };
}
