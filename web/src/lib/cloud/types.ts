// Row and Edge Function types for the ixtable Cloud control plane (PLAN-CLOUD contract).

export type OrgRole = "owner" | "admin" | "billing" | "member";
export type MemberStatus = "active" | "revoked";
export type VersionStatus = "pending" | "published" | "withdrawn";
export type CredentialScope = "shared" | "user";

export interface Organization {
  id: string;
  name: string;
  created_by: string | null;
  created_at: string;
}

export interface Profile {
  id: string;
  email: string | null;
  display_name: string | null;
  is_operator?: boolean;
}

export interface OrgMember {
  org_id: string;
  user_id: string;
  role: OrgRole;
  created_at: string;
}

export interface CloudApp {
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
  deleted_at?: string | null;
}

/** Mirrors desktop `roles.rs` Permissions (camelCase JSON). `name` is optional display text. */
export interface ObjectPermission {
  kind: "form" | "report" | "dashboard" | "table" | "query" | string;
  id: string;
  name?: string;
  read?: boolean;
  create?: boolean;
  update?: boolean;
  delete?: boolean;
}

export interface AppPermissions {
  navigation?: string[];
  objects?: ObjectPermission[];
  actions?: string[];
}

/** `id` is the desktop role id (roles.rs), kept so manifests and the Runtime agree. */
export interface AppRole {
  id: string;
  app_id: string;
  name: string;
  permissions: AppPermissions;
  updated_at: string;
}

export interface AppMember {
  app_id: string;
  user_id: string;
  role_id: string;
  status: MemberStatus;
  invited_by: string | null;
  created_at: string;
  revoked_at: string | null;
}

export interface Invitation {
  id: string;
  kind: "org" | "app";
  org_id: string | null;
  app_id: string | null;
  email: string;
  org_role: Exclude<OrgRole, "owner"> | null;
  role_id: string | null;
  expires_at: string;
  accepted_at: string | null;
  revoked_at: string | null;
  created_at: string;
}

/**
 * The security summary stored on a published version, as normalized by
 * publish-checkpoint (_shared/distribution.ts normalizeSecurity): camelCase.
 */
export interface SecuritySummary {
  store: "sqlite" | "postgres";
  /** Only for PostgreSQL apps: one shared database login, or one per user. */
  credentialMode: "shared" | "perUser" | null;
  tls: boolean;
  sslmode: string | null;
  /** The Developer confirmed publishing a datasource without TLS. */
  insecureTransportConfirmed: boolean;
  insecureTransportConfirmedAt: string | null;
  /** The Developer acknowledged the shared PostgreSQL credential warning. */
  sharedCredentialAcknowledged: boolean;
  concurrencyPoliciesResolved: boolean;
  unresolvedEntities: string[];
}

export interface MigrationRef {
  id: string;
  name?: string;
}

export interface AppVersion {
  id: string;
  app_id: string;
  version: string;
  developer_id: string;
  created_at: string;
  archive_sha256: string;
  archive_size: number;
  migrations: MigrationRef[] | string[] | null;
  min_runtime_version: string | null;
  security: SecuritySummary | null;
  release_notes: string;
  status: VersionStatus;
  parent_version_id: string | null;
  resolution: "overwrite" | "fork" | null;
  published_at: string | null;
  withdrawn_at: string | null;
}

export interface Backup {
  id: string;
  app_id: string;
  user_id: string;
  installation_id: string;
  created_at: string;
  archive_sha256: string;
  archive_size: number;
}

export interface Installation {
  id: string;
  app_id: string;
  user_id: string;
  device_name: string;
  installed_version_id: string | null;
  last_seen_at: string;
  revoked_at: string | null;
  created_at: string;
}

export interface CredentialEnvelopeMeta {
  id: string;
  app_id: string;
  datasource_id: string;
  scope: CredentialScope;
  user_id: string | null;
  kek_version: number;
  created_at: string;
  updated_at: string;
  revoked_at: string | null;
  last_grant_at?: string | null;
}

export interface AuditEvent {
  id: string;
  at: string;
  actor_id: string | null;
  org_id: string | null;
  app_id: string | null;
  action: string;
  target: string | null;
  details: Record<string, unknown> | null;
}

export type BillingInterval = "month" | "year";

/** Console capabilities from the app_capabilities RPC. */
export type Capability = "owner" | "admin" | "billing" | "view";

export type CollaboratorRole = "admin" | "billing" | "viewer";

export interface AppCollaborator {
  app_id: string;
  user_id: string;
  role: CollaboratorRole;
  granted_by: string | null;
  created_at: string;
  updated_at: string;
}

export interface Plan {
  id: string;
  name: string;
  price_cents: number;
  /** Always price_cents * 10 (two months free). */
  annual_price_cents: number;
  currency: string;
  interval: string;
  runtime_user_allowance: number;
  storage_gb: number;
}

export interface Entitlement {
  allowed: boolean;
  reason:
    | "ok"
    | "grace"
    | "not_found"
    | "app_deleted"
    | "no_subscription"
    | "subscription_inactive"
    | "over_allowance";
  allowance: number;
  used: number;
  status?: string;
  planId?: string;
  interval?: BillingInterval;
}

export interface Subscription {
  app_id: string;
  plan_id: string;
  provider: "stripe" | "fake";
  status: string;
  billing_interval: BillingInterval;
  current_period_end: string | null;
  cancel_at_period_end: boolean;
}

/** Provider invoice, as returned by the billing functions (_shared/billing.ts Invoice). */
export interface Invoice {
  id: string;
  number: string | null;
  status: string | null;
  amountDue: number;
  amountPaid: number;
  currency: string;
  created: string;
  hostedInvoiceUrl: string | null;
  pdfUrl: string | null;
}

export interface CloudSession {
  access_token: string;
  refresh_token: string;
  expires_at: number;
  user: { id: string; email?: string };
}

export interface RoleInput {
  id: string;
  name: string;
  permissions: AppPermissions;
}

// Edge Function inputs and outputs, keyed by function name.
export interface FunctionMap {
  "apps-create": {
    in: { orgId: string; name: string; documentId: string };
    out: { app: CloudApp };
  };
  "apps-delete": {
    // While the subscription bills, the call is refused (403, details.reason
    // "active_subscription") unless cancelSubscription is true.
    in: { appId: string; confirm: string; cancelSubscription?: boolean };
    out: {
      appId: string;
      deletedAt: string;
      subscriptionStatus: string | null;
      subscriptionCanceled: boolean;
    };
  };
  "apps-transfer": {
    in: { appId: string; newOwnerId: string; confirm: string };
    out: { app: CloudApp };
  };
  "invitations-create": {
    in: {
      kind: "org" | "app";
      orgId?: string;
      appId?: string;
      email: string;
      role?: Exclude<OrgRole, "owner">;
      roleId?: string;
    };
    // `delivery` is always "sent": the reply never reveals whether the email has an account.
    out: { invitation: Invitation; acceptUrl: string; delivery: "sent" };
  };
  "invitations-accept": {
    in: { token: string };
    out: {
      membership: {
        kind: "org" | "app";
        invitationId: string;
        orgId: string;
        appId?: string;
        userId: string;
        roleId?: string;
        role?: string;
        status?: string;
      };
    };
  };
  "members-update": {
    in: { appId: string; userId: string; roleId?: string; status?: MemberStatus };
    out: { member: AppMember };
  };
  "versions-resolve": {
    // The website forks a published version and withdraws versions. Overwrite needs the
    // pending upload from Studio, so it is resolved in the desktop app.
    in:
      | { appId: string; action: "fork"; fromVersionId: string; name?: string }
      // Withdrawing the last published version while installations run it is refused
      // (422, details {requiresConfirm, installations}) unless confirm is true.
      | { appId: string; action: "withdraw"; versionId: string; confirm?: boolean };
    out: {
      version?: AppVersion;
      app?: CloudApp;
      headVersionId?: string | null;
      dependentInstallations?: number;
    };
  };
  "restore-url": {
    in: { appId: string; versionId?: string; backupId?: string };
    out: {
      signedUrl: string;
      sha256: string;
      size: number;
      isPostgres: boolean;
      warning: string | null;
      kind: "version" | "backup";
      id: string;
      expiresAt: string;
    };
  };
  "credential-delete": {
    in: { appId: string; datasourceId: string; scope?: CredentialScope; userId?: string };
    out: { revokedEnvelopeIds: string[]; revokedGrants: number };
  };
  "devices-revoke": {
    in: { appId: string; userId: string; installationId: string };
    out: {
      installation: { id: string; revokedAt: string };
      revokedGrants: number;
      alreadyRevoked: boolean;
    };
  };
  "billing-checkout": {
    in: { appId: string; planId: string; interval?: BillingInterval };
    out: { url: string; overAllowance: boolean };
  };
  "app-access-update": {
    // A null role removes the grant; the collaborator is then null.
    in: { appId: string; userId: string; role: CollaboratorRole | null };
    out: { collaborator: AppCollaborator | null };
  };
  "billing-portal": { in: { appId: string }; out: { url: string } };
  "billing-invoices": { in: { appId: string }; out: { invoices: Invoice[] } };
  "billing-cancel": {
    in: { appId: string; atPeriodEnd: boolean };
    out: { subscription: Subscription | null };
  };
  "billing-fake-complete": {
    in: { sessionId: string; appId: string; planId: string };
    out: { ok: boolean; status: string | null };
  };
  "desktop-auth-approve": {
    in: { codeChallenge: string; state: string };
    out: { ok: boolean; expiresAt: string };
  };
  "account-export": { in: Record<string, never>; out: { export: unknown; archives: string[] } };
  "account-delete": {
    in: { confirmEmail: string; cancelSubscriptions?: boolean };
    out: Record<string, never>;
  };
  "admin-support": {
    in: { query: { email?: string; appId?: string } };
    out: { diagnostics: Record<string, unknown> };
  };
}

export type FunctionName = keyof FunctionMap;
