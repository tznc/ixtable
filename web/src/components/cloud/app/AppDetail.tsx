import React, { type ReactNode } from "react";
import Link from "@docusaurus/Link";
import { useHistory, useLocation } from "@docusaurus/router";
import type { User } from "@supabase/supabase-js";
import { useCloudApi, type Capability } from "@site/src/lib/cloud";
import { entitlementStatus } from "@site/src/lib/cloud/status";
import { Badge, ErrorNotice, Loading, Notice, TabPanel, Tabs, type TabDef } from "../ui";
import { useAsync } from "../useAsync";
import AccessTab from "./AccessTab";
import AuditTab from "./AuditTab";
import BackupsTab from "./BackupsTab";
import BillingTab from "./BillingTab";
import CredentialsTab from "./CredentialsTab";
import InstallationsTab from "./InstallationsTab";
import OverviewTab from "./OverviewTab";
import RolesTab from "./RolesTab";
import SettingsTab from "./SettingsTab";
import type { AppTabProps } from "./types";
import UsersTab from "./UsersTab";
import VersionsTab from "./VersionsTab";

type Access = "all" | Capability;

const TABS: (TabDef & { access: Access; render: (props: AppTabProps) => ReactNode })[] = [
  { id: "overview", label: "Overview", access: "all", render: (p) => <OverviewTab {...p} /> },
  { id: "users", label: "Runtime users", access: "view", render: (p) => <UsersTab {...p} /> },
  { id: "roles", label: "Roles", access: "view", render: (p) => <RolesTab {...p} /> },
  { id: "versions", label: "Versions", access: "view", render: (p) => <VersionsTab {...p} /> },
  { id: "backups", label: "Backups", access: "admin", render: (p) => <BackupsTab {...p} /> },
  {
    id: "installations",
    label: "Installations",
    access: "view",
    render: (p) => <InstallationsTab {...p} />,
  },
  {
    id: "credentials",
    label: "Credentials",
    access: "owner",
    render: (p) => <CredentialsTab {...p} />,
  },
  { id: "audit", label: "Audit history", access: "view", render: (p) => <AuditTab {...p} /> },
  { id: "access", label: "Access", access: "view", render: (p) => <AccessTab {...p} /> },
  { id: "billing", label: "Billing", access: "billing", render: (p) => <BillingTab {...p} /> },
  { id: "settings", label: "Settings", access: "admin", render: (p) => <SettingsTab {...p} /> },
];

function allowed(access: Access, capabilities: Capability[]): boolean {
  return access === "all" || capabilities.includes(access);
}

/** /cloud/app?id=…&tab=…: one cloud app. Tabs a viewer cannot use are hidden, and RLS still applies. */
export default function AppDetail({ user }: { user: User }): ReactNode {
  const api = useCloudApi();
  const history = useHistory();
  const location = useLocation();
  const params = new URLSearchParams(location.search);
  const appId = params.get("id") ?? "";
  const state = useAsync(async () => {
    const q = api.q();
    const app = await q.app(appId);
    const [capabilities, orgRole, profiles, entitlement] = await Promise.all([
      q.appCapabilities(appId).catch((): Capability[] => []),
      q.myOrgRole(app.org_id, user.id),
      q.profiles([app.owner_id]),
      q.entitlement(appId).catch(() => null),
    ]);
    return { app, capabilities, orgRole, owner: profiles[app.owner_id] ?? null, entitlement };
  }, [api, appId, user.id]);

  if (!appId)
    return (
      <Notice tone="danger">
        No app selected. Open an app from the <Link to="/cloud">Cloud dashboard</Link>.
      </Notice>
    );
  if (state.loading && !state.data) return <Loading label="Loading app" />;
  if (state.error || !state.data) return <ErrorNotice error={state.error} testId="app-error" />;

  const { app, capabilities, orgRole, owner, entitlement } = state.data;
  const props: AppTabProps = {
    app,
    user,
    isOwner: capabilities.includes("owner"),
    capabilities,
    isAdmin: capabilities.includes("admin"),
    canView: capabilities.includes("view"),
    canBill: capabilities.includes("billing"),
    orgRole,
    owner,
    entitlement,
    reloadApp: state.reload,
  };
  const tabs = TABS.filter((tab) => allowed(tab.access, capabilities));
  const selected = tabs.find((tab) => tab.id === params.get("tab")) ?? tabs[0];
  const status = entitlementStatus(entitlement);
  const selectTab = (id: string) => history.replace(`/cloud/app?id=${app.id}&tab=${id}`);

  return (
    <>
      <nav aria-label="Breadcrumb" className="cloud-muted">
        <Link to={`/cloud?org=${app.org_id}`}>Cloud dashboard</Link> / {app.name}
      </nav>
      <div className="cloud-header">
        <h1>{app.name}</h1>
        <Badge tone={status.tone}>{status.label}</Badge>
      </div>
      <Tabs tabs={tabs} selected={selected.id} onSelect={selectTab} label="App sections" />
      <TabPanel id={selected.id}>{selected.render(props)}</TabPanel>
    </>
  );
}
