import React, { useId, useState, type FormEvent, type ReactNode } from "react";
import { formatDate, useCloudApi, type AppMember, type AppRole } from "@site/src/lib/cloud";
import { Badge, Empty, ErrorNotice, Loading, Notice, Section, TableWrap } from "../ui";
import { useAction, useAsync } from "../useAsync";
import type { AppTabProps } from "./types";

function InviteUser({
  appId,
  roles,
  onInvited,
}: {
  appId: string;
  roles: AppRole[];
  onInvited: () => void;
}): ReactNode {
  const api = useCloudApi();
  const emailId = useId();
  const roleId = useId();
  const [email, setEmail] = useState("");
  const [role, setRole] = useState(roles[0]?.id ?? "");
  const [sentTo, setSentTo] = useState<string | null>(null);
  const invite = useAction(async () => {
    await api.call("invitations-create", {
      kind: "app",
      appId,
      email: email.trim().toLowerCase(),
      roleId: role,
    });
    setSentTo(email);
    setEmail("");
    onInvited();
  });
  const submit = (event: FormEvent) => {
    event.preventDefault();
    invite.run();
  };
  if (roles.length === 0) {
    return (
      <Empty>
        This app has no runtime roles yet. Define roles in Studio and publish so they sync here,
        then invite runtime users.
      </Empty>
    );
  }
  return (
    <form className="cloud-inline-form" onSubmit={submit} aria-label="Invite runtime user">
      <div className="cloud-field">
        <label htmlFor={emailId}>Email</label>
        <input
          id={emailId}
          type="email"
          required
          value={email}
          onChange={(event) => setEmail(event.target.value)}
        />
      </div>
      <div className="cloud-field">
        <label htmlFor={roleId}>Runtime role</label>
        <select id={roleId} value={role} onChange={(event) => setRole(event.target.value)}>
          {roles.map((row) => (
            <option key={row.id} value={row.id}>
              {row.name}
            </option>
          ))}
        </select>
      </div>
      <button type="submit" className="button button--primary" disabled={invite.pending || !role}>
        {invite.pending ? "Sending..." : "Invite runtime user"}
      </button>
      <ErrorNotice error={invite.error} testId="app-invite-error" />
      {sentTo && (
        <Notice tone="success" testId="app-invite-sent">
          Invitation sent to {sentTo}.
        </Notice>
      )}
    </form>
  );
}

/** Runtime users: role changes, revoke and restore, invitations. Seats count active users only. */
export default function UsersTab({ app, entitlement, isAdmin, reloadApp }: AppTabProps): ReactNode {
  const api = useCloudApi();
  const state = useAsync(async () => {
    const q = api.q();
    const [members, roles, invitations] = await Promise.all([
      q.appMembers(app.id),
      q.roles(app.id),
      q.invitations({ appId: app.id }).catch(() => []),
    ]);
    const profiles = await q.profiles(members.map((member) => member.user_id));
    return { members, roles, invitations, profiles };
  }, [api, app.id]);
  const update = useAction(
    async (member: AppMember, patch: { roleId?: string; status?: "active" | "revoked" }) => {
      await api.call("members-update", { appId: app.id, userId: member.user_id, ...patch });
      state.reload();
      reloadApp();
    },
  );
  const revokeInvite = useAction(async (id: string) => {
    await api.q().revokeInvitation(id);
    state.reload();
  });

  if (state.loading && !state.data) return <Loading label="Loading runtime users" />;
  const data = state.data;
  return (
    <>
      <Section
        title="Runtime users"
        description={
          entitlement
            ? `${entitlement.used} of ${entitlement.allowance} runtime user seats in use. Revoked users do not count.`
            : "Runtime users install the app in the desktop Runtime."
        }
      >
        <ErrorNotice error={state.error ?? update.error ?? revokeInvite.error} />
        {data && data.members.length === 0 && (
          <Empty>
            {isAdmin ? "No runtime users yet. Invite someone below." : "No runtime users yet."}
          </Empty>
        )}
        {data && data.members.length > 0 && (
          <TableWrap label="Runtime users">
            <thead>
              <tr>
                <th scope="col">User</th>
                <th scope="col">Role</th>
                <th scope="col">Status</th>
                <th scope="col">Added</th>
                {isAdmin && <th scope="col">Access</th>}
              </tr>
            </thead>
            <tbody>
              {data.members.map((member) => {
                const email = data.profiles[member.user_id]?.email || member.user_id;
                const active = member.status === "active";
                return (
                  <tr key={member.user_id}>
                    <th scope="row">{email}</th>
                    <td>
                      {isAdmin ? (
                        <select
                          aria-label={`Role for ${email}`}
                          value={member.role_id}
                          disabled={update.pending}
                          onChange={(event) => update.run(member, { roleId: event.target.value })}
                        >
                          {data.roles.map((role) => (
                            <option key={role.id} value={role.id}>
                              {role.name}
                            </option>
                          ))}
                        </select>
                      ) : (
                        (data.roles.find((role) => role.id === member.role_id)?.name ?? "Unknown")
                      )}
                    </td>
                    <td>
                      <Badge tone={active ? "success" : "danger"}>
                        {active ? "Active" : "Revoked"}
                      </Badge>
                      {member.revoked_at && (
                        <div className="cloud-muted">{formatDate(member.revoked_at)}</div>
                      )}
                    </td>
                    <td>{formatDate(member.created_at)}</td>
                    {isAdmin && (
                      <td>
                        <button
                          type="button"
                          className={`button button--sm ${active ? "button--outline button--danger" : "button--secondary"}`}
                          disabled={update.pending}
                          onClick={() =>
                            update.run(member, { status: active ? "revoked" : "active" })
                          }
                        >
                          {active ? `Revoke ${email}` : `Restore ${email}`}
                        </button>
                      </td>
                    )}
                  </tr>
                );
              })}
            </tbody>
          </TableWrap>
        )}
        <p className="cloud-muted">
          Revoking blocks new bundle downloads and credential key grants right away. A key grant
          already issued stays valid until it expires, at most 24 hours. Revocation cannot erase an
          app copy or a credential the user already received.
        </p>
      </Section>
      {isAdmin && data && data.invitations.length > 0 && (
        <Section title="Pending invitations">
          <ul>
            {data.invitations.map((invitation) => (
              <li key={invitation.id}>
                {invitation.email} as{" "}
                {data.roles.find((role) => role.id === invitation.role_id)?.name ?? "unknown role"},
                expires {formatDate(invitation.expires_at)}{" "}
                <button
                  type="button"
                  className="button button--sm button--link"
                  onClick={() => revokeInvite.run(invitation.id)}
                >
                  Revoke invitation for {invitation.email}
                </button>
              </li>
            ))}
          </ul>
        </Section>
      )}
      {isAdmin && (
        <Section
          title="Invite a runtime user"
          description="The invitation email links to a page where they sign in and accept."
        >
          {data && <InviteUser appId={app.id} roles={data.roles} onInvited={state.reload} />}
        </Section>
      )}
    </>
  );
}
