import React, { useId, useState, type FormEvent, type ReactNode } from "react";
import {
  formatDate,
  useCloudApi,
  type AppCollaborator,
  type CollaboratorRole,
} from "@site/src/lib/cloud";
import { Badge, Empty, ErrorNotice, Loading, Notice, Section, TableWrap } from "../ui";
import { useAction, useAsync } from "../useAsync";
import type { AppTabProps } from "./types";

const ROLES: { value: CollaboratorRole; label: string; help: string }[] = [
  {
    value: "admin",
    label: "Admin",
    help: "Manages Runtime Users, roles, installations, backups, settings and billing",
  },
  {
    value: "billing",
    label: "Billing",
    help: "Manages the app's plan and billing; reads the overview",
  },
  { value: "viewer", label: "Viewer", help: "Reads the console; changes nothing" },
];
const ROLE_LABEL = Object.fromEntries(ROLES.map((role) => [role.value, role.label]));

function GrantForm({
  candidates,
  onGrant,
  pending,
}: {
  candidates: { userId: string; label: string }[];
  onGrant: (userId: string, role: CollaboratorRole) => void;
  pending: boolean;
}): ReactNode {
  const memberId = useId();
  const roleId = useId();
  const [userId, setUserId] = useState("");
  const [role, setRole] = useState<CollaboratorRole>("viewer");
  if (candidates.length === 0) {
    return <Empty>Every other organization member already has access to this app.</Empty>;
  }
  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (userId) onGrant(userId, role);
  };
  return (
    <form className="cloud-inline-form" onSubmit={submit} aria-label="Grant app access">
      <div className="cloud-field">
        <label htmlFor={memberId}>Organization member</label>
        <select id={memberId} value={userId} onChange={(event) => setUserId(event.target.value)}>
          <option value="">Choose a member</option>
          {candidates.map((row) => (
            <option key={row.userId} value={row.userId}>
              {row.label}
            </option>
          ))}
        </select>
      </div>
      <div className="cloud-field">
        <label htmlFor={roleId}>App role</label>
        <select
          id={roleId}
          value={role}
          onChange={(event) => setRole(event.target.value as CollaboratorRole)}
        >
          {ROLES.map((row) => (
            <option key={row.value} value={row.value}>
              {row.label}
            </option>
          ))}
        </select>
      </div>
      <button type="submit" className="button button--primary" disabled={pending || !userId}>
        {pending ? "Saving..." : "Grant access"}
      </button>
    </form>
  );
}

/** Who can use this app's console: the owner, organization owners and admins, and per-app grants. */
export default function AccessTab({ app, owner, orgRole }: AppTabProps): ReactNode {
  const api = useCloudApi();
  const canGrant = orgRole === "owner" || orgRole === "admin";
  const state = useAsync(async () => {
    const q = api.q();
    const [collaborators, members] = await Promise.all([
      q.appCollaborators(app.id),
      q.orgMembers(app.org_id).catch(() => []),
    ]);
    const profiles = await q.profiles([
      ...collaborators.map((row) => row.user_id),
      ...members.map((row) => row.user_id),
    ]);
    return { collaborators, members, profiles };
  }, [api, app.id, app.org_id]);
  const update = useAction(async (userId: string, role: CollaboratorRole | null) => {
    await api.call("app-access-update", { appId: app.id, userId, role });
    state.reload();
  });

  if (state.loading && !state.data) return <Loading label="Loading access" />;
  const data = state.data;
  const nameOf = (userId: string) => data?.profiles[userId]?.email || userId;
  const granted = new Set(data?.collaborators.map((row) => row.user_id));
  const implicit = (data?.members ?? []).filter(
    (row) => (row.role === "owner" || row.role === "admin") && row.user_id !== app.owner_id,
  );
  const candidates = (data?.members ?? [])
    .filter(
      (row) =>
        row.role !== "owner" &&
        row.role !== "admin" &&
        row.user_id !== app.owner_id &&
        !granted.has(row.user_id),
    )
    .map((row) => ({ userId: row.user_id, label: nameOf(row.user_id) }));

  const roleCell = (row: AppCollaborator, email: string) =>
    canGrant ? (
      <select
        aria-label={`App role for ${email}`}
        value={row.role}
        disabled={update.pending}
        onChange={(event) => update.run(row.user_id, event.target.value as CollaboratorRole)}
      >
        {ROLES.map((role) => (
          <option key={role.value} value={role.value}>
            {role.label}
          </option>
        ))}
      </select>
    ) : (
      <Badge>{ROLE_LABEL[row.role] ?? row.role}</Badge>
    );

  return (
    <>
      <Notice tone="info" title="Console access only">
        Per-app access is console access. It does not grant Runtime access: Runtime Users are
        managed on the Runtime users tab.
      </Notice>
      <ErrorNotice error={state.error ?? update.error} testId="access-error" />
      <Section
        title="Who has access"
        description="Organization and app roles combine: a person gets everything either role allows."
      >
        <TableWrap label="App access">
          <thead>
            <tr>
              <th scope="col">Member</th>
              <th scope="col">Access</th>
              <th scope="col">Since</th>
              {canGrant && <th scope="col">Actions</th>}
            </tr>
          </thead>
          <tbody>
            <tr>
              <th scope="row">{owner?.email || app.owner_id}</th>
              <td>
                <Badge tone="info">Developer/Owner</Badge>
              </td>
              <td>{formatDate(app.created_at)}</td>
              {canGrant && <td />}
            </tr>
            {implicit.map((row) => (
              <tr key={row.user_id}>
                <th scope="row">{nameOf(row.user_id)}</th>
                <td>
                  <Badge>Admin via organization</Badge>
                  <div className="cloud-muted">Organization {row.role}</div>
                </td>
                <td>{formatDate(row.created_at)}</td>
                {canGrant && <td />}
              </tr>
            ))}
            {data?.collaborators.map((row) => {
              const email = nameOf(row.user_id);
              return (
                <tr key={row.user_id}>
                  <th scope="row">{email}</th>
                  <td>
                    {roleCell(row, email)}
                    <div className="cloud-muted">
                      {ROLES.find((role) => role.value === row.role)?.help}
                    </div>
                  </td>
                  <td>{formatDate(row.created_at)}</td>
                  {canGrant && (
                    <td>
                      <button
                        type="button"
                        className="button button--sm button--outline button--danger"
                        disabled={update.pending}
                        onClick={() => update.run(row.user_id, null)}
                      >
                        Remove access for {email}
                      </button>
                    </td>
                  )}
                </tr>
              );
            })}
          </tbody>
        </TableWrap>
        {data?.collaborators.length === 0 && <p className="cloud-muted">No per-app grants yet.</p>}
      </Section>
      {canGrant && data && (
        <Section
          title="Grant access"
          description="Give an organization member a role on this app. Only owners and admins can do this."
        >
          <GrantForm
            candidates={candidates}
            pending={update.pending}
            onGrant={(userId, role) => update.run(userId, role)}
          />
        </Section>
      )}
    </>
  );
}
