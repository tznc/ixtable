// App and membership lookups for the credential, key-grant and device
// functions. Callers that have no relationship to an app get NOT_FOUND, so
// they cannot probe which app ids exist.
import { serviceClient } from "./db.ts";
import { HttpError } from "./http.ts";

export interface LiveApp {
  id: string;
  org_id: string;
  owner_id: string;
  deleted_at: string | null;
}

export interface Membership {
  status: "active" | "revoked";
  role_id: string;
}

/** The app, or NOT_FOUND when it does not exist or was deleted. */
export async function loadLiveApp(appId: string): Promise<LiveApp> {
  const { data, error } = await serviceClient()
    .from("cloud_apps")
    .select("id, org_id, owner_id, deleted_at")
    .eq("id", appId)
    .maybeSingle();
  if (error) throw new Error(`load app failed: ${error.message}`);
  if (!data || data.deleted_at) throw new HttpError("NOT_FOUND", "App not found");
  return data as LiveApp;
}

export async function loadMembership(appId: string, userId: string): Promise<Membership | null> {
  const { data, error } = await serviceClient()
    .from("app_members")
    .select("status, role_id")
    .eq("app_id", appId)
    .eq("user_id", userId)
    .maybeSingle();
  if (error) throw new Error(`load membership failed: ${error.message}`);
  return (data as Membership | null) ?? null;
}

/** App owner, org owner/admin, or per-app admin (SQL `is_app_admin`). */
export async function isAppAdmin(app: LiveApp, userId: string): Promise<boolean> {
  if (app.owner_id === userId) return true;
  const { data, error } = await serviceClient().rpc("app_capabilities_for", {
    p_app_id: app.id,
    p_user_id: userId,
  });
  if (error) throw new Error(`load app capabilities failed: ${error.message}`);
  return ((data as string[] | null) ?? []).includes("admin");
}

/** Throws FORBIDDEN for related non-owners and NOT_FOUND for strangers. */
export async function requireAppOwner(app: LiveApp, userId: string): Promise<void> {
  if (app.owner_id === userId) return;
  if ((await isAppAdmin(app, userId)) || (await loadMembership(app.id, userId))) {
    throw new HttpError("FORBIDDEN", "Only the app owner can manage credentials");
  }
  throw new HttpError("NOT_FOUND", "App not found");
}
