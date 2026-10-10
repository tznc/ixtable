import React, { useId, useState, type FormEvent, type ReactNode } from "react";
import {
  formatDate,
  useCloudApi,
  type OrgMember,
  type OrgRole,
  type Organization,
} from "@site/src/lib/cloud";
import { Badge, ErrorNotice, Loading, Notice, Section, TableWrap } from "../ui";
import { useAction, useAsync } from "../useAsync";

const ROLE_HELP: Record<OrgRole, string> = {
  owner: "Admin on every app; full control, including deleting the organization",
  admin: "Admin on every app; manages members and invitations",
  billing: "Manages billing of every app; no other app access",
  member: "No app access until granted per app on its Access tab",
};
const INVITABLE: Exclude<OrgRole, "owner">[] = ["admin", "billing", "member"];

function InviteForm({ org, onInvited }: { org: Organization; onInvited: () => void }): ReactNode {
  const api = useCloudApi();
  const emailId = useId();
  const roleId = useId();
  const [email, setEmail] = useState("");
  const [role, setRole] = useState<Exclude<OrgRole, "owner">>("member");
  const [sentTo, setSentTo] = useState<string | null>(null);
  const invite = useAction(async () => {
    await api.call("invitations-create", {
      kind: "org",
      orgId: org.id,
      email: email.trim().toLowerCase(),
      role,
    });
    setSentTo(email);
    setEmail("");
    onInvited();
  });
  const submit = (event: FormEvent) => {
    event.preventDefault();
    invite.run();
  };
  return (
    <form className="cloud-inline-form" onSubmit={submit} aria-label="Invite to organization">
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
        <label htmlFor={roleId}>Organization role</label>
        <select
          id={roleId}
          value={role}
          onChange={(event) => setRole(event.target.value as typeof role)}
        >
          {INVITABLE.map((value) => (
            <option key={value} value={value}>
              {value}
            </option>
          ))}
        </select>
      </div>
      <button type="submit" className="button button--primary" disabled={invite.pending}>
        {invite.pending ? "Sending..." : "Send invitation"}
      </button>
      <ErrorNotice error={invite.error} />
      {sentTo && (
        <Notice tone="success" testId="org-invite-sent">
          Invitation sent to {sentTo}.
        </Notice>
      )}
    </form>
  );
}

/** Organization members, their roles, pending invitations, and the invite form. */
export default function OrgMembers({
  org,
  userId,
}: {
  org: Organization;
  userId: string;
}): ReactNode {
  const api = useCloudApi();
  const state = useAsync(async () => {
    const q = api.q();
    const members = await q.orgMembers(org.id);
    const [profiles, invitations] = await Promise.all([
      q.profiles(members.map((member) => member.user_id)),
      q.invitations({ orgId: org.id }).catch(() => []),
    ]);
    return { members, profiles, invitations };
  }, [api, org.id]);
  const change = useAction(async (member: OrgMember, role: OrgRole) => {
    await api.q().updateOrgMember(org.id, member.user_id, role);
    state.reload();
  });
  const remove = useAction(async (member: OrgMember) => {
    await api.q().removeOrgMember(org.id, member.user_id);
    state.reload();
  });
  const revoke = useAction(async (invitationId: string) => {
    await api.q().revokeInvitation(invitationId);
    state.reload();
  });

  if (state.loading && !state.data) return <Loading label="Loading members" />;
  const data = state.data;
  const myRole = data?.members.find((member) => member.user_id === userId)?.role;
  const canManage = myRole === "owner" || myRole === "admin";

  return (
    <Section
      title="Members"
      description="Organization roles control who manages apps and billing. Runtime users of an app are managed on the app page."
    >
      <ErrorNotice error={state.error ?? change.error ?? remove.error ?? revoke.error} />
      {data && (
        <TableWrap label="Organization members">
          <thead>
            <tr>
              <th scope="col">Member</th>
              <th scope="col">Role</th>
              <th scope="col">Joined</th>
              {canManage && <th scope="col">Actions</th>}
            </tr>
          </thead>
          <tbody>
            {data.members.map((member) => {
              const email = data.profiles[member.user_id]?.email || member.user_id;
              const editable =
                canManage &&
                member.user_id !== userId &&
                (myRole === "owner" || member.role !== "owner");
              return (
                <tr key={member.user_id}>
                  <th scope="row">
                    {email}
                    {member.user_id === userId && <span className="cloud-muted"> (you)</span>}
                  </th>
                  <td>
                    {editable ? (
                      <select
                        aria-label={`Role for ${email}`}
                        value={member.role}
                        disabled={change.pending}
                        onChange={(event) => change.run(member, event.target.value as OrgRole)}
                      >
                        {(myRole === "owner"
                          ? (["owner", ...INVITABLE] as OrgRole[])
                          : INVITABLE
                        ).map((value) => (
                          <option key={value} value={value}>
                            {value}
                          </option>
                        ))}
                      </select>
                    ) : (
                      <Badge>{member.role}</Badge>
                    )}
                    <div className="cloud-muted">{ROLE_HELP[member.role]}</div>
                  </td>
                  <td>{formatDate(member.created_at)}</td>
                  {canManage && (
                    <td>
                      {editable && (
                        <button
                          type="button"
                          className="button button--sm button--outline button--danger"
                          disabled={remove.pending}
                          onClick={() => remove.run(member)}
                        >
                          Remove {email}
                        </button>
                      )}
                    </td>
                  )}
                </tr>
              );
            })}
          </tbody>
        </TableWrap>
      )}
      {canManage && data && data.invitations.length > 0 && (
        <>
          <h3>Pending invitations</h3>
          <ul>
            {data.invitations.map((invitation) => (
              <li key={invitation.id}>
                {invitation.email} as {invitation.org_role}, expires{" "}
                {formatDate(invitation.expires_at)}{" "}
                <button
                  type="button"
                  className="button button--sm button--link"
                  onClick={() => revoke.run(invitation.id)}
                >
                  Revoke invitation for {invitation.email}
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
      {canManage && (
        <>
          <h3>Invite someone</h3>
          <InviteForm org={org} onInvited={state.reload} />
        </>
      )}
    </Section>
  );
}
