// Grants, changes or removes an organization member's per-app console role
// (PRD §20.3). Only organization owners and admins may call it. The target
// must belong to the app's organization and must not be the app owner, who
// already has every capability. Per-app access never grants Runtime access.
// Audits access.grant, access.change and access.remove.
//
// POST {appId, userId, role: "admin"|"billing"|"viewer"|null} → {collaborator | null}
import { audit } from "../_shared/audit.ts";
import { serviceClient } from "../_shared/db.ts";
import { loadApp, orgRole } from "../_shared/distribution.ts";
import { handler, HttpError, readJson, requireUser } from "../_shared/http.ts";
import { enforceNamedRateLimit, incrementMetric } from "../_shared/rateLimit.ts";
import { oneOf, uuid } from "../_shared/validate.ts";

const ROLES = ["admin", "billing", "viewer"] as const;
const COLUMNS = "app_id, user_id, role, granted_by, created_at, updated_at";

Deno.serve(
  handler(async (req) => {
    const { user } = await requireUser(req);
    const body = await readJson(req);
    const appId = uuid(body, "appId");
    const userId = uuid(body, "userId");
    if (!("role" in body))
      throw new HttpError("VALIDATION", "role: is required (null removes access)", {
        field: "role",
      });
    const role = body.role === null ? null : oneOf(body, "role", ROLES);

    await enforceNamedRateLimit("app-access-update", user.id);
    const app = await loadApp(appId);
    const callerRole = await orgRole(app.org_id, user.id);
    if (callerRole !== "owner" && callerRole !== "admin")
      throw new HttpError(
        "FORBIDDEN",
        "Only an organization owner or admin can change per-app access",
      );
    if (userId === app.owner_id)
      throw new HttpError("VALIDATION", "userId: the app owner already has full access", {
        field: "userId",
      });
    if (role !== null && !(await orgRole(app.org_id, userId)))
      throw new HttpError("VALIDATION", "userId: is not a member of the app's organization", {
        field: "userId",
      });

    const db = serviceClient();
    const { data: before, error: loadError } = await db
      .from("app_collaborators")
      .select(COLUMNS)
      .eq("app_id", appId)
      .eq("user_id", userId)
      .maybeSingle();
    if (loadError) throw new Error(`load access failed: ${loadError.message}`);

    let collaborator: Record<string, unknown> | null = null;
    if (role === null) {
      const { error } = await db
        .from("app_collaborators")
        .delete()
        .eq("app_id", appId)
        .eq("user_id", userId);
      if (error) throw new Error(`remove access failed: ${error.message}`);
    } else {
      const { data, error } = await db
        .from("app_collaborators")
        .upsert(
          { app_id: appId, user_id: userId, role, granted_by: user.id },
          { onConflict: "app_id,user_id" },
        )
        .select(COLUMNS)
        .single();
      if (error) throw new Error(`grant access failed: ${error.message}`);
      collaborator = data;
    }

    const action =
      role === null
        ? before
          ? "access.remove"
          : null
        : before
          ? before.role === role
            ? null
            : "access.change"
          : "access.grant";
    if (action) {
      await audit({
        action,
        actorId: user.id,
        orgId: app.org_id,
        appId,
        target: `user:${userId}`,
        details: { from: before?.role ?? null, to: role },
        req,
      });
      await incrementMetric(`app_access.${action.split(".")[1]}`);
    }
    return { collaborator };
  }),
);
