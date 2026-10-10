import type { User } from "@supabase/supabase-js";
import type { Capability, CloudApp, Entitlement, Profile } from "@site/src/lib/cloud";

export interface AppTabProps {
  app: CloudApp;
  user: User;
  /** The single Developer/Owner of the app. */
  isOwner: boolean;
  /** Console capabilities of the viewer (app_capabilities). */
  capabilities: Capability[];
  /** Has the "admin" capability: owner, org owner/admin, or per-app admin. Mutating controls need it. */
  isAdmin: boolean;
  /** Has the "view" capability: may read the console tabs. */
  canView: boolean;
  /** Has the "billing" capability: may manage the app's plan. */
  canBill: boolean;
  /** Organization role of the viewer, if any. */
  orgRole: string | null;
  owner: Profile | null;
  entitlement: Entitlement | null;
  reloadApp: () => void;
}
