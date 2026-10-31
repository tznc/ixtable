/**
 * Chaos: the cloud versions deploy flow (archive-upload-url → PUT →
 * publish-checkpoint / versions-resolve → bundle-manifest / sync-check) under
 * lost replies, tampering, concurrency, rollback, auth loss, storage faults and
 * injected server errors. Real local stack only. Each test records what the
 * system actually did; `expect` pins the observed behaviour so a fix flips it.
 */
import { execFileSync } from "node:child_process";
import { createHmac } from "node:crypto";
import { getServiceClient, localStack } from "../clients";
import { type CloudFixture, expect, test } from "../fixture";
import { recordOutcome } from "../record";
import { addMember, createInstallation, createRole, grantSubscription } from "../seed";
import {
  ARCHIVE_BUCKET,
  ArchiveTracker,
  call,
  type ErrorBody,
  fakeArchive,
  objectExists,
  publish,
  publishOk,
  SQLITE_SECURITY,
  sha256,
  uploadArchive,
} from "./distribution-fixtures";

const archives = new ArchiveTracker();
test.afterEach(() => archives.cleanup());

function psql(sql: string): string {
  return execFileSync(
    "docker",
    ["exec", "supabase_db_ixtable", "psql", "-U", "postgres", "-tA", "-c", sql],
    { encoding: "utf8" },
  ).trim();
}

async function put(signedUrl: string, bytes: Buffer) {
  const res = await fetch(signedUrl, {
    method: "PUT",
    headers: {
      apikey: localStack().anonKey,
      "content-type": "application/octet-stream",
      "x-upsert": "false",
    },
    body: new Uint8Array(bytes),
  });
  return { status: res.status, body: await res.text() };
}

async function setup(cloud: CloudFixture) {
  const owner = await cloud.user();
  const app = await cloud.app(await cloud.org(owner), owner);
  archives.add(app.id);
  await grantSubscription(app.id, "team");
  return { owner, app };
}

test("c01 lost upload reply: the retry PUT gets storage 400 KeyAlreadyExists", async ({ cloud }) => {
  const { owner, app } = await setup(cloud);
  const bytes = fakeArchive();
  const up = await uploadArchive(owner, app.id, { bytes });
  const retry = await put(up.signedUrl, bytes);
  const after = await publish(owner, app.id, {
    version: "1.0.0",
    expectedHeadVersionId: null,
    uploadId: up.uploadId,
  });
  recordOutcome("chaos-01-upload-retry-409", {
    expectations: [
      "A retried PUT after a lost 200 is refused by Storage with HTTP 400 (body statusCode 409, KeyAlreadyExists).",
      "The first upload is complete, so publish-checkpoint on the same uploadId still succeeds.",
    ],
    details: { firstPut: up.putStatus, retry, publishAfter: after.result.status },
  });
  expect(up.putStatus).toBe(200);
  expect(retry.status).toBe(400);
  expect(retry.body).toContain("KeyAlreadyExists");
  expect(after.result.status).toBe(200);
});

test("c02 same-size tampered upload publishes a head no runtime can verify", async ({
  cloud,
}) => {
  const { owner, app } = await setup(cloud);
  const declared = fakeArchive(4096);
  const actual = fakeArchive(4096);
  const url = await call<{ uploadId: string; signedUrl: string }>("archive-upload-url", owner, {
    appId: app.id,
    kind: "version",
    size: declared.length,
    sha256: sha256(declared),
  });
  const putRes = await put(url.body.signedUrl, actual);
  const pub = await call<{ version: { id: string } } & ErrorBody>("publish-checkpoint", owner, {
    appId: app.id,
    uploadId: url.body.uploadId,
    version: "1.0.0",
    security: SQLITE_SECURITY,
    expectedHeadVersionId: null,
  });
  const bundle = await call<{ manifest: { archiveSha256: string }; archiveUrl: string }>(
    "bundle-manifest",
    owner,
    { appId: app.id, installationId: crypto.randomUUID() },
  );
  const downloaded = Buffer.from(await (await fetch(bundle.body.archiveUrl)).arrayBuffer());
  recordOutcome("chaos-02-tampered-same-size", {
    expectations: [
      "publish-checkpoint accepts bytes whose sha256 differs from the declared sha256.",
      "The signed manifest names the declared hash, so every runtime download fails verification.",
    ],
    details: {
      put: putRes.status,
      publish: pub.status,
      manifestSha: bundle.body.manifest.archiveSha256,
      downloadedSha: sha256(downloaded),
    },
  });
  expect(pub.status).toBe(200);
  expect(bundle.body.manifest.archiveSha256).not.toBe(sha256(downloaded));
});

test("c03 concurrent publishes of one app: one winner, losers orphan uploads", async ({
  cloud,
}) => {
  const { owner, app } = await setup(cloud);
  const v1 = await publishOk(owner, app.id, { version: "1.0.0", expectedHeadVersionId: null });
  const uploads = await Promise.all(
    [0, 1, 2, 3, 4, 5].map(() => uploadArchive(owner, app.id)),
  );
  const results = await Promise.all(
    uploads.map((u, i) =>
      call<ErrorBody>("publish-checkpoint", owner, {
        appId: app.id,
        uploadId: u.uploadId,
        version: `1.${i + 1}.0`,
        security: SQLITE_SECURITY,
        expectedHeadVersionId: v1.id,
      }),
    ),
  );
  const statuses = results.map((r) => r.status);
  const codes = results.map((r) => r.body?.error?.code ?? "ok");
  const admin = getServiceClient();
  const { data: appRow } = await admin
    .from("cloud_apps")
    .select("head_version_id")
    .eq("id", app.id)
    .single();
  const { data: pending } = await admin
    .from("archive_uploads")
    .select("id, status")
    .eq("app_id", app.id)
    .eq("status", "pending");
  recordOutcome("chaos-03-concurrent-publish", {
    expectations: [
      "Exactly one of six concurrent publishes on the same head wins; the rest get 409.",
      "Losing uploads stay pending (stored objects) until they expire.",
    ],
    details: { statuses, codes, head: appRow?.head_version_id, pending: pending?.length },
  });
  expect(statuses.filter((s) => s === 200)).toHaveLength(1);
  expect(statuses.filter((s) => s === 409)).toHaveLength(5);
  expect(pending?.length).toBe(5);
});

test("c04 lost publish reply: the replay is a 409 against the caller's own version", async ({
  cloud,
}) => {
  const { owner, app } = await setup(cloud);
  const up = await uploadArchive(owner, app.id);
  const body = {
    appId: app.id,
    uploadId: up.uploadId,
    version: "1.0.0",
    security: SQLITE_SECURITY,
    expectedHeadVersionId: null,
  };
  const first = await call<{ version: { id: string } }>("publish-checkpoint", owner, body);
  const replay = await call<ErrorBody>("publish-checkpoint", owner, body);
  recordOutcome("chaos-04-publish-replay", {
    expectations: [
      "Replaying an already-committed publish returns 409 VERSION_CONFLICT, not the committed version.",
      "details.headVersionId is the caller's own just-published version.",
    ],
    details: { first: first.status, replay: replay.status, body: replay.body },
  });
  expect(replay.status).toBe(409);
  expect(replay.body.error.details?.headVersionId).toBe(first.body.version.id);
});

test("c05 audit failure after commit: 500 reported, version is live", async ({ cloud }) => {
  const { owner, app } = await setup(cloud);
  const up = await uploadArchive(owner, app.id);
  const sig = "public.audit(text, uuid, uuid, uuid, text, jsonb, text)";
  psql(`revoke execute on function ${sig} from service_role`);
  let res;
  try {
    res = await call<ErrorBody>("publish-checkpoint", owner, {
      appId: app.id,
      uploadId: up.uploadId,
      version: "1.0.0",
      security: SQLITE_SECURITY,
      expectedHeadVersionId: null,
    });
  } finally {
    psql(`grant execute on function ${sig} to service_role`);
  }
  const head = psql(`select head_version_id from public.cloud_apps where id = '${app.id}'`);
  const audits = psql(
    `select count(*) from public.audit_events where app_id = '${app.id}' and action = 'version.publish'`,
  );
  recordOutcome("chaos-05-audit-after-commit", {
    expectations: [
      "When the audit write fails, the client gets a 5xx although the version is published.",
      "The published version has no version.publish audit event.",
    ],
    details: { status: res.status, body: res.body, head, audits },
  });
  expect(res.status).toBeGreaterThanOrEqual(500);
  expect(head).toBe(up.uploadId);
  expect(audits).toBe("0");
});

test("c06 rollback by withdraw: lower version publishes, runtimes on the withdrawn one are told to downgrade", async ({
  cloud,
}) => {
  const { owner, app } = await setup(cloud);
  const role = await createRole(app.id);
  const member = await cloud.user();
  await addMember(app.id, member.user.id, role.id);
  const v1 = await publishOk(owner, app.id, { version: "1.0.0", expectedHeadVersionId: null });
  const v2 = await publishOk(owner, app.id, { version: "2.0.0", expectedHeadVersionId: v1.id });
  const inst = await createInstallation(app.id, member.user.id);
  await call("sync-check", member, { appId: app.id, installationId: inst, installedVersionId: v2.id });
  const withdrawn = await call<{ headVersionId: string }>("versions-resolve", owner, {
    appId: app.id,
    action: "withdraw",
    versionId: v2.id,
  });
  const lower = await publish(owner, app.id, {
    version: "1.5.0",
    expectedHeadVersionId: withdrawn.body.headVersionId,
  });
  const sync = await call<{ upToDate: boolean; latest: { version: string } }>("sync-check", member, {
    appId: app.id,
    installationId: inst,
    installedVersionId: v2.id,
  });
  recordOutcome("chaos-06-withdraw-then-lower", {
    expectations: [
      "After withdrawing 2.0.0 the head is 1.0.0 and 1.5.0 (below the withdrawn 2.0.0) publishes.",
      "A runtime still on 2.0.0 is told it is not up to date and that 1.5.0 is latest (the desktop refuses downgrades).",
    ],
    details: {
      withdraw: withdrawn.status,
      headAfterWithdraw: withdrawn.body.headVersionId === v1.id ? "1.0.0" : withdrawn.body,
      lowerPublish: lower.result.status,
      sync: sync.body,
    },
  });
  expect(lower.result.status).toBe(200);
  expect(sync.body.upToDate).toBe(false);
  expect(sync.body.latest.version).toBe("1.5.0");
});

test("c07 withdraw the only version with confirm: runtimes on it read as up to date", async ({
  cloud,
}) => {
  const { owner, app } = await setup(cloud);
  const v1 = await publishOk(owner, app.id, { version: "1.0.0", expectedHeadVersionId: null });
  const inst = await createInstallation(app.id, owner.user.id);
  await call("sync-check", owner, { appId: app.id, installationId: inst, installedVersionId: v1.id });
  const w = await call("versions-resolve", owner, {
    appId: app.id,
    action: "withdraw",
    versionId: v1.id,
    confirm: true,
  });
  const sync = await call<{ upToDate: boolean; latest: unknown }>("sync-check", owner, {
    appId: app.id,
    installationId: inst,
    installedVersionId: v1.id,
  });
  recordOutcome("chaos-07-withdraw-last", {
    expectations: [
      "Withdrawing the only installed version with confirm succeeds.",
      "sync-check then reports upToDate:true and latest:null; the runtime keeps running the withdrawn build silently.",
    ],
    details: { withdraw: w.status, sync: sync.body },
  });
  expect(w.status).toBe(200);
  expect(sync.body).toEqual({ upToDate: true, latest: null });
});

function hs256(payload: Record<string, unknown>, secret: string): string {
  const b64 = (v: unknown) => Buffer.from(JSON.stringify(v)).toString("base64url");
  const head = b64({ alg: "HS256", typ: "JWT" });
  const body = b64(payload);
  const sig = createHmac("sha256", secret).update(`${head}.${body}`).digest("base64url");
  return `${head}.${body}.${sig}`;
}

test("c08 expired and signed-out tokens", async ({ cloud }) => {
  const { owner, app } = await setup(cloud);
  const claims = JSON.parse(Buffer.from(owner.jwt.split(".")[1], "base64url").toString());
  const expired = hs256(
    { ...claims, exp: Math.floor(Date.now() / 1000) - 60 },
    "super-secret-jwt-token-with-at-least-32-characters-long",
  );
  const exp = await call<ErrorBody>("archive-upload-url", { ...owner, jwt: expired }, {
    appId: app.id,
    kind: "version",
    size: 10,
    sha256: sha256(Buffer.from("x")),
  });
  // Global sign-out, then reuse the old access token.
  await owner.client.auth.signOut({ scope: "global" });
  const revoked = await call<ErrorBody>("archive-upload-url", owner, {
    appId: app.id,
    kind: "version",
    size: 10,
    sha256: sha256(Buffer.from("x")),
  });
  recordOutcome("chaos-08-auth-loss", {
    expectations: [
      "An expired JWT is refused with 401.",
      "An access token whose session was signed out globally is refused (not honoured until exp).",
    ],
    details: { expired: [exp.status, exp.body], signedOut: [revoked.status, revoked.body] },
  });
  expect(exp.status).toBe(401);
  expect(revoked.status).toBe(401);
});

test("c09 oversize PUT: commit fails, object leaks past the sweep", async ({ cloud }) => {
  const { owner, app } = await setup(cloud);
  const up = await uploadArchive(owner, app.id, { bytes: fakeArchive(8192), declaredSize: 2048 });
  const pub = await publish(owner, app.id, {
    version: "1.0.0",
    expectedHeadVersionId: null,
    uploadId: up.uploadId,
  });
  psql(
    `update public.archive_uploads set expires_at = now() - interval '1 hour' where app_id = '${app.id}'`,
  );
  const sweep = await fetch(`${localStack().url}/functions/v1/retention-sweep`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${localStack().serviceRoleKey}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({ appId: app.id }),
  });
  const status = psql(`select status from public.archive_uploads where id = '${up.uploadId}'`);
  const stillThere = await objectExists(up.path);
  recordOutcome("chaos-09-oversize-leak", {
    expectations: [
      "An upload larger than declared is refused at commit (422) and marked failed.",
      "retention-sweep does not remove the failed upload's object, so it stays in storage.",
    ],
    details: { put: up.putStatus, publish: pub.result.status, sweep: sweep.status, status, stillThere },
  });
  expect(pub.result.status).toBe(422);
  expect(status).toBe("failed");
  expect(stillThere).toBe(true);
});

test("c10 entitlement lapses between upload and publish", async ({ cloud }) => {
  const { owner, app } = await setup(cloud);
  const up = await uploadArchive(owner, app.id);
  await getServiceClient().from("subscriptions").update({ status: "canceled" }).eq("app_id", app.id);
  const pub = await publish(owner, app.id, {
    version: "1.0.0",
    expectedHeadVersionId: null,
    uploadId: up.uploadId,
  });
  recordOutcome("chaos-10-entitlement-lapse", {
    expectations: [
      "publish-checkpoint returns 402 ENTITLEMENT_REQUIRED once the plan lapses after upload.",
      "The upload stays pending and its object remains until the sweep expires it.",
    ],
    details: { publish: pub.result.status, body: pub.result.body, object: await objectExists(up.path) },
  });
  expect(pub.result.status).toBe(402);
});

test("c11 storage quota: 500 MB upload slots are handed out without a running total", async ({
  cloud,
}) => {
  const { owner, app } = await setup(cloud);
  const slots = await Promise.all(
    [0, 1, 2, 3, 4].map(() =>
      call("archive-upload-url", owner, {
        appId: app.id,
        kind: "version",
        size: 524_288_000,
        sha256: sha256(Buffer.from(crypto.randomUUID())),
      }),
    ),
  );
  const { data: plan } = await getServiceClient()
    .from("plans")
    .select("storage_gb")
    .eq("id", "team")
    .single();
  recordOutcome("chaos-11-quota", {
    expectations: [
      "Five concurrent 500 MB upload URLs are all issued (2.5 GB outstanding) for one app.",
      "No check compares outstanding or stored bytes against the plan's storage_gb.",
    ],
    details: { statuses: slots.map((s) => s.status), planStorageGb: plan?.storage_gb },
  });
  expect(slots.every((s) => s.status === 200)).toBe(true);
});

test("c12 RLS and role denials on the deploy path", async ({ cloud }) => {
  const { owner, app } = await setup(cloud);
  const role = await createRole(app.id);
  const member = await cloud.user();
  await addMember(app.id, member.user.id, role.id);
  const v1 = await publishOk(owner, app.id, { version: "1.0.0", expectedHeadVersionId: null });
  const memberUpload = await call<ErrorBody>("archive-upload-url", member, {
    appId: app.id,
    kind: "version",
    size: 10,
    sha256: sha256(Buffer.from("x")),
  });
  const memberWithdraw = await call<ErrorBody>("versions-resolve", member, {
    appId: app.id,
    action: "withdraw",
    versionId: v1.id,
  });
  const direct = await member.client
    .from("app_versions")
    .update({ status: "withdrawn" })
    .eq("id", v1.id)
    .select();
  const head = await member.client
    .from("cloud_apps")
    .update({ head_version_id: null })
    .eq("id", app.id)
    .select();
  const storageWrite = await member.client.storage
    .from(ARCHIVE_BUCKET)
    .upload(`apps/${app.id}/versions/${crypto.randomUUID()}.ixt`, new Uint8Array([1, 2, 3]), {
      contentType: "application/octet-stream",
    });
  recordOutcome("chaos-12-rls", {
    expectations: [
      "A Runtime User cannot get a version upload URL or withdraw (403).",
      "Direct PostgREST and Storage writes to versions, head or archives change nothing.",
    ],
    details: {
      memberUpload: memberUpload.status,
      memberWithdraw: memberWithdraw.status,
      directVersion: [direct.error?.code ?? null, direct.data?.length ?? 0],
      directHead: [head.error?.code ?? null, head.data?.length ?? 0],
      storage: storageWrite.error?.message ?? "WROTE",
    },
  });
  expect(memberUpload.status).toBe(403);
  expect(memberWithdraw.status).toBe(403);
  expect(direct.data?.length ?? 0).toBe(0);
  expect(head.data?.length ?? 0).toBe(0);
  expect(storageWrite.error).not.toBeNull();
});

test("c13 withdraw races publish on the same head", async ({ cloud }) => {
  const { owner, app } = await setup(cloud);
  const v1 = await publishOk(owner, app.id, { version: "1.0.0", expectedHeadVersionId: null });
  const v2 = await publishOk(owner, app.id, { version: "2.0.0", expectedHeadVersionId: v1.id });
  const up = await uploadArchive(owner, app.id);
  const [w, p] = await Promise.all([
    call("versions-resolve", owner, { appId: app.id, action: "withdraw", versionId: v2.id }),
    call<ErrorBody>("publish-checkpoint", owner, {
      appId: app.id,
      uploadId: up.uploadId,
      version: "2.1.0",
      security: SQLITE_SECURITY,
      expectedHeadVersionId: v2.id,
    }),
  ]);
  const head = psql(
    `select v.version from public.cloud_apps a join public.app_versions v on v.id = a.head_version_id where a.id = '${app.id}'`,
  );
  const published = psql(
    `select string_agg(version || ':' || status, ',' order by version) from public.app_versions where app_id = '${app.id}'`,
  );
  recordOutcome("chaos-13-withdraw-vs-publish", {
    expectations: [
      "Withdraw and publish serialize on the app row; the head ends on a published version.",
    ],
    details: { withdraw: w.status, publish: [p.status, p.body?.error?.code], head, published },
  });
  expect(["1.0.0", "2.1.0"]).toContain(head);
});

test("c14 head archive object lost from storage", async ({ cloud }) => {
  const { owner, app } = await setup(cloud);
  const v1 = await publishOk(owner, app.id, { version: "1.0.0", expectedHeadVersionId: null });
  await getServiceClient().storage.from(ARCHIVE_BUCKET).remove([v1.storage_path]);
  const bundle = await call<ErrorBody & { archiveUrl?: string }>("bundle-manifest", owner, {
    appId: app.id,
    installationId: crypto.randomUUID(),
  });
  const dl = bundle.body.archiveUrl ? (await fetch(bundle.body.archiveUrl)).status : null;
  recordOutcome("chaos-14-missing-object", {
    expectations: [
      "bundle-manifest for a head whose archive object is gone answers with a clear error or a URL that 4xxs.",
    ],
    details: { status: bundle.status, body: bundle.body.error ?? "signed", download: dl },
  });
  expect(bundle.status === 200 ? dl : bundle.status).toBeGreaterThanOrEqual(400);
});

test("c15 malformed and hostile publish bodies", async ({ cloud }) => {
  const { owner, app } = await setup(cloud);
  const raw = await fetch(`${localStack().url}/functions/v1/publish-checkpoint`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${owner.jwt}`,
      apikey: localStack().anonKey,
      "content-type": "application/json",
    },
    body: "{not json",
  });
  const huge = await call<ErrorBody>("publish-checkpoint", owner, {
    appId: app.id,
    uploadId: crypto.randomUUID(),
    version: `1.0.0-${"a".repeat(5000)}`,
    security: SQLITE_SECURITY,
    expectedHeadVersionId: null,
  });
  recordOutcome("chaos-15-malformed", {
    expectations: [
      "Malformed JSON and oversized fields are rejected with 4xx and the error contract shape.",
    ],
    details: { badJson: [raw.status, await raw.text()], hugeVersion: [huge.status, huge.body] },
  });
  expect(raw.status).toBeLessThan(500);
  expect(huge.status).toBeLessThan(500);
});
