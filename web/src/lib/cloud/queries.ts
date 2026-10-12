import type { SupabaseClient } from "@supabase/supabase-js";
import { fromPostgrestError } from "./errors";
import type {
  AppCollaborator,
  AppMember,
  AppRole,
  AppVersion,
  AuditEvent,
  Backup,
  Capability,
  CloudApp,
  CredentialEnvelopeMeta,
  Entitlement,
  Installation,
  Invitation,
  OrgMember,
  Organization,
  Plan,
  Profile,
  Subscription,
} from "./types";

type Result<T> = { data: T | null; error: { code?: string; message?: string } | null };

function unwrap<T>(result: Result<T>): T {
  if (result.error) throw fromPostgrestError(result.error);
  return result.data as T;
}

const APP_COLUMNS =
  "id,org_id,owner_id,name,document_id,datasource_kind,backups_enabled,retention_versions,retention_days,head_version_id,created_at";
const VERSION_COLUMNS =
  "id,app_id,version,developer_id,created_at,archive_sha256,archive_size,migrations,min_runtime_version,security,release_notes,status,parent_version_id,resolution,published_at,withdrawn_at";
// Envelope metadata only. RLS grants no client access to ciphertext, nonce, aad, or wrapped keys.
const ENVELOPE_COLUMNS =
  "id,app_id,datasource_id,scope,user_id,kek_version,created_at,updated_at,revoked_at";
const ORG_COLUMNS = "id,name,created_by,created_at";

export interface AuditFilter {
  appId?: string;
  orgId?: string;
  action?: string;
  since?: string;
  limit?: number;
}

/** PostgREST reads. Row Level Security decides what each signed-in user can see. */
export function createQueries(client: SupabaseClient) {
  return {
    async organizations(): Promise<Organization[]> {
      return unwrap(await client.from("organizations").select(ORG_COLUMNS).order("created_at"));
    },
    async orgMembers(orgId: string): Promise<OrgMember[]> {
      return unwrap(
        await client
          .from("org_members")
          .select("org_id,user_id,role,created_at")
          .eq("org_id", orgId)
          .order("created_at"),
      );
    },
    async apps(orgId?: string): Promise<CloudApp[]> {
      let query = client.from("cloud_apps").select(APP_COLUMNS).is("deleted_at", null);
      if (orgId) query = query.eq("org_id", orgId);
      return unwrap(await query.order("created_at"));
    },
    async app(appId: string): Promise<CloudApp> {
      return unwrap(await client.from("cloud_apps").select(APP_COLUMNS).eq("id", appId).single());
    },
    async roles(appId: string): Promise<AppRole[]> {
      return unwrap(
        await client
          .from("app_roles")
          .select("id,app_id,name,permissions,updated_at")
          .eq("app_id", appId)
          .order("name"),
      );
    },
    async appMembers(appId: string): Promise<AppMember[]> {
      return unwrap(
        await client
          .from("app_members")
          .select("app_id,user_id,role_id,status,invited_by,created_at,revoked_at")
          .eq("app_id", appId)
          .order("created_at"),
      );
    },
    async memberCounts(appIds: string[]): Promise<Record<string, number>> {
      if (appIds.length === 0) return {};
      const rows = unwrap<{ app_id: string }[]>(
        await client
          .from("app_members")
          .select("app_id")
          .in("app_id", appIds)
          .eq("status", "active"),
      );
      const counts: Record<string, number> = {};
      for (const row of rows) counts[row.app_id] = (counts[row.app_id] ?? 0) + 1;
      return counts;
    },
    async invitations(filter: { appId?: string; orgId?: string }): Promise<Invitation[]> {
      let query = client
        .from("invitations")
        .select(
          "id,kind,org_id,app_id,email,org_role,role_id,expires_at,accepted_at,revoked_at,created_at",
        )
        .is("accepted_at", null)
        .is("revoked_at", null);
      if (filter.appId) query = query.eq("app_id", filter.appId);
      if (filter.orgId) query = query.eq("org_id", filter.orgId).eq("kind", "org");
      return unwrap(await query.order("created_at", { ascending: false }));
    },
    async versions(appId: string): Promise<AppVersion[]> {
      return unwrap(
        await client
          .from("app_versions")
          .select(VERSION_COLUMNS)
          .eq("app_id", appId)
          .order("created_at", { ascending: false }),
      );
    },
    async latestVersions(appIds: string[]): Promise<Record<string, AppVersion>> {
      if (appIds.length === 0) return {};
      const rows = unwrap<AppVersion[]>(
        await client
          .from("app_versions")
          .select(VERSION_COLUMNS)
          .in("app_id", appIds)
          .eq("status", "published")
          .order("created_at", { ascending: false }),
      );
      const latest: Record<string, AppVersion> = {};
      for (const row of rows) latest[row.app_id] ??= row;
      return latest;
    },
    async backups(appId: string): Promise<Backup[]> {
      return unwrap(
        await client
          .from("installation_backups")
          .select("id,app_id,user_id,installation_id,created_at,archive_sha256,archive_size")
          .eq("app_id", appId)
          .order("created_at", { ascending: false }),
      );
    },
    async installations(appId: string): Promise<Installation[]> {
      return unwrap(
        await client
          .from("installations")
          .select(
            "id,app_id,user_id,device_name,installed_version_id,last_seen_at,revoked_at,created_at",
          )
          .eq("app_id", appId)
          .order("last_seen_at", { ascending: false }),
      );
    },
    async credentialEnvelopes(appId: string): Promise<CredentialEnvelopeMeta[]> {
      const envelopes = unwrap<CredentialEnvelopeMeta[]>(
        await client
          .from("credential_envelopes")
          .select(ENVELOPE_COLUMNS)
          .eq("app_id", appId)
          .order("datasource_id"),
      );
      const grants = await client
        .from("key_grants")
        .select("envelope_id,datasource_id,user_id,issued_at")
        .eq("app_id", appId)
        .order("issued_at", { ascending: false })
        .limit(500);
      if (grants.error || !grants.data) return envelopes;
      const rows = grants.data as { envelope_id: string | null; issued_at: string }[];
      return envelopes.map((envelope) => {
        const grant = rows.find((row) => row.envelope_id === envelope.id);
        return { ...envelope, last_grant_at: grant?.issued_at ?? null };
      });
    },
    async auditEvents(filter: AuditFilter): Promise<AuditEvent[]> {
      let query = client
        .from("audit_events")
        .select("id,at,actor_id,org_id,app_id,action,target,details");
      if (filter.appId) query = query.eq("app_id", filter.appId);
      if (filter.orgId) query = query.eq("org_id", filter.orgId);
      if (filter.action) query = query.like("action", `${filter.action}%`);
      if (filter.since) query = query.gte("at", filter.since);
      return unwrap(await query.order("at", { ascending: false }).limit(filter.limit ?? 200));
    },
    async plans(): Promise<Plan[]> {
      return unwrap(
        await client
          .from("plans")
          .select(
            "id,name,price_cents,annual_price_cents,currency,interval,runtime_user_allowance,storage_gb",
          )
          .order("sort"),
      );
    },
    async subscriptions(appIds: string[]): Promise<Record<string, Subscription>> {
      if (appIds.length === 0) return {};
      const rows = unwrap<Subscription[]>(
        await client
          .from("subscriptions")
          .select(
            "app_id,plan_id,provider,status,billing_interval,current_period_end,cancel_at_period_end",
          )
          .in("app_id", appIds),
      );
      return Object.fromEntries(rows.map((row) => [row.app_id, row]));
    },
    async profiles(userIds: string[]): Promise<Record<string, Profile>> {
      const ids = [...new Set(userIds)].filter(Boolean);
      if (ids.length === 0) return {};
      const result = await client.from("profiles").select("id,email,display_name").in("id", ids);
      // Profiles of other users may be hidden by RLS. Callers fall back to the user id.
      if (result.error || !result.data) return {};
      return Object.fromEntries((result.data as Profile[]).map((row) => [row.id, row]));
    },
    async myProfile(userId: string): Promise<Profile | null> {
      const result = await client
        .from("profiles")
        .select("id,email,display_name,is_operator")
        .eq("id", userId)
        .maybeSingle();
      return (result.data as Profile | null) ?? null;
    },
    async updateApp(
      appId: string,
      patch: Partial<
        Pick<CloudApp, "name" | "backups_enabled" | "retention_versions" | "retention_days">
      >,
    ): Promise<CloudApp> {
      return unwrap(
        await client.from("cloud_apps").update(patch).eq("id", appId).select(APP_COLUMNS).single(),
      );
    },
    async isAppAdmin(appId: string): Promise<boolean> {
      return Boolean(unwrap(await client.rpc("is_app_admin", { p_app_id: appId })));
    },
    async appCapabilities(appId: string): Promise<Capability[]> {
      const caps = unwrap<Capability[] | null>(
        await client.rpc("app_capabilities", { p_app_id: appId }),
      );
      return caps ?? [];
    },
    async appCollaborators(appId: string): Promise<AppCollaborator[]> {
      return unwrap(
        await client
          .from("app_collaborators")
          .select("app_id,user_id,role,granted_by,created_at,updated_at")
          .eq("app_id", appId)
          .order("created_at"),
      );
    },
    async myOrgRole(orgId: string, userId: string): Promise<string | null> {
      const result = await client
        .from("org_members")
        .select("role")
        .eq("org_id", orgId)
        .eq("user_id", userId)
        .maybeSingle();
      return (result.data as { role: string } | null)?.role ?? null;
    },
    async entitlement(appId: string): Promise<Entitlement> {
      return unwrap(await client.rpc("app_entitlement", { p_app_id: appId }));
    },
    async createOrganization(name: string, userId: string): Promise<Organization> {
      // A trigger makes the creator the organization's owner.
      return unwrap(
        await client
          .from("organizations")
          .insert({ name, created_by: userId })
          .select(ORG_COLUMNS)
          .single(),
      );
    },
    async revokeInvitation(invitationId: string): Promise<void> {
      unwrap(
        await client
          .from("invitations")
          .update({ revoked_at: new Date().toISOString() })
          .eq("id", invitationId),
      );
    },
    async updateOrgMember(orgId: string, userId: string, role: string): Promise<void> {
      unwrap(
        await client.from("org_members").update({ role }).eq("org_id", orgId).eq("user_id", userId),
      );
    },
    async removeOrgMember(orgId: string, userId: string): Promise<void> {
      unwrap(await client.from("org_members").delete().eq("org_id", orgId).eq("user_id", userId));
    },
    async updateProfile(userId: string, displayName: string): Promise<void> {
      unwrap(await client.from("profiles").update({ display_name: displayName }).eq("id", userId));
    },
  };
}

export type CloudQueries = ReturnType<typeof createQueries>;
