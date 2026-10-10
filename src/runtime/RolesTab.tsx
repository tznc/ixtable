import { Plus, Trash2 } from "lucide-react";
import { useState } from "react";
import { flattenNavigation } from "../design/schema";
import { useDocumentConfig } from "../lib/config-store";
import { readQueries } from "../query/types";
import type { DocumentConfig } from "../lib/types";
import { newId } from "../lib/utils";
import { useShell } from "../shell/context";
import { setObjectPermission, toggleListed } from "./rbac";
import { userTriggerGaps } from "./trigger-warnings";
import "./runtime.css";
import type { ObjectKind, Permissions, Role } from "./types";

const FLAGS = ["read", "create", "update", "delete"] as const;

/** PRD §20.2: ixtable RBAC is not a database security boundary. */
export const RBAC_LIMITATION =
  "Roles control what the ixtable Runtime shows and allows: navigation, forms, reports, dashboards, queries, and actions. " +
  "They are not a database security boundary. With a direct PostgreSQL connection, a user who extracts valid database " +
  "credentials can bypass these rules. Strong isolation needs separate least-privileged database credentials and database " +
  "permissions that you set up yourself.";

const permissionsOf = (role: Role | null): Permissions => ({
  navigation: role?.permissions?.navigation ?? [],
  objects: role?.permissions?.objects ?? [],
  actions: role?.permissions?.actions ?? [],
});

type Row = { kind: ObjectKind; id: string; name: string };

function objectRows(config: DocumentConfig, tables: string[]): Row[] {
  return [
    ...config.design.forms.map((f) => ({ kind: "form" as const, id: f.id, name: f.name })),
    ...(config.reports ?? []).map((r) => ({ kind: "report" as const, id: r.id, name: r.name })),
    ...(config.dashboards ?? []).map((d) => ({
      kind: "dashboard" as const,
      id: d.id,
      name: d.name,
    })),
    ...tables.map((t) => ({ kind: "table" as const, id: t, name: t })),
    // Action queries are governed by their target table's permissions.
    ...readQueries(config.savedQueries).map((q) => ({
      kind: "query" as const,
      id: q.id,
      name: q.name,
    })),
  ];
}

/** Settings tab: local role definitions and their permission matrix (PRD §20). */
export function RolesTab() {
  const { config, update } = useDocumentConfig();
  const { objects } = useShell();
  const roles = config.roles ?? [];
  const [selectedId, setSelectedId] = useState<string | null>(roles[0]?.id ?? null);
  const role = roles.find((item) => item.id === selectedId) ?? null;
  const tables = objects.filter((o) => o.objectType === "table").map((o) => o.name);

  const editRole = (id: string, change: (role: Role) => Role, label = "Edit role") =>
    update(
      (draft) => ({
        ...draft,
        roles: (draft.roles ?? []).map((item) => (item.id === id ? change(item) : item)),
      }),
      label,
    ).catch(() => undefined);
  const editPermissions = (change: (p: Permissions) => Permissions) =>
    role &&
    editRole(role.id, (item) => ({
      ...item,
      permissions: change(permissionsOf(item)),
    }));

  const addRole = () => {
    const created: Role = {
      id: newId(),
      name: `Role ${roles.length + 1}`,
      permissions: { navigation: [], objects: [], actions: [] },
    };
    setSelectedId(created.id);
    update((draft) => ({ ...draft, roles: [...(draft.roles ?? []), created] }), "Add role").catch(
      () => undefined,
    );
  };
  const removeRole = (id: string) => {
    setSelectedId(null);
    update(
      (draft) => ({ ...draft, roles: (draft.roles ?? []).filter((item) => item.id !== id) }),
      "Delete role",
    ).catch(() => undefined);
  };

  const permissions = permissionsOf(role);
  const flag = (row: Row, name: (typeof FLAGS)[number]) =>
    permissions.objects.some((o) => o.kind === row.kind && o.id === row.id && o[name]);

  return (
    <div className="rt-roles">
      <p className="rt-notice" role="note">
        {RBAC_LIMITATION}
      </p>
      <div className="rt-roles-body">
        <aside aria-label="Roles">
          <ul>
            {roles.map((item) => (
              <li key={item.id}>
                <button
                  type="button"
                  aria-pressed={item.id === role?.id}
                  onClick={() => setSelectedId(item.id)}
                >
                  {item.name || "Untitled role"}
                </button>
              </li>
            ))}
          </ul>
          <button type="button" onClick={addRole}>
            <Plus aria-hidden="true" />
            New role
          </button>
          <p className="rt-muted">The developer always has full access.</p>
        </aside>
        {role ? (
          <section aria-label={`Role ${role.name}`} className="rt-role-editor">
            <div className="rt-record-head">
              <label>
                Role name
                <input
                  value={role.name}
                  onChange={(e) =>
                    editRole(role.id, (r) => ({ ...r, name: e.target.value }), "Rename role")
                  }
                />
              </label>
              <button type="button" onClick={() => removeRole(role.id)}>
                <Trash2 aria-hidden="true" />
                Delete role
              </button>
            </div>
            {userTriggerGaps(config, role.id).map((gap) => (
              <p key={gap.triggerId} className="rt-error" role="status">
                Trigger "{gap.name}" runs as the signed-in user, but this role cannot{" "}
                {gap.missing.join(", ")}. Saves that fire it will be refused for this role.
              </p>
            ))}
            <h3>Objects</h3>
            <p className="rt-muted">
              Form permissions govern records changed through that form. Table permissions govern
              table pages and actions that write to the table. Reading a form, report, or dashboard
              also lets the role read the saved queries it shows.
            </p>
            <table className="rt-table rt-matrix">
              <thead>
                <tr>
                  <th scope="col">Object</th>
                  {FLAGS.map((name) => (
                    <th key={name} scope="col">
                      {name[0].toUpperCase() + name.slice(1)}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {objectRows(config, tables).map((row) => (
                  <tr key={`${row.kind}:${row.id}`}>
                    <th scope="row">
                      <small>{row.kind}</small> {row.name}
                    </th>
                    {FLAGS.map((name) => (
                      <td key={name}>
                        <input
                          type="checkbox"
                          aria-label={`${name} ${row.kind} ${row.name}`}
                          checked={flag(row, name)}
                          onChange={(e) =>
                            editPermissions((p) =>
                              setObjectPermission(p, row.kind, row.id, name, e.target.checked),
                            )
                          }
                        />
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
            <h3>Navigation</h3>
            <ul className="rt-checklist">
              {flattenNavigation(config.design.navigation).map((item) => (
                <li key={item.id}>
                  <label>
                    <input
                      type="checkbox"
                      checked={permissions.navigation.includes(item.id)}
                      onChange={(e) =>
                        editPermissions((p) => ({
                          ...p,
                          navigation: toggleListed(p.navigation, item.id, e.target.checked),
                        }))
                      }
                    />
                    Show {item.label} in navigation
                  </label>
                </li>
              ))}
            </ul>
            <h3>Actions</h3>
            <ul className="rt-checklist">
              {(config.actions ?? []).map((action) => (
                <li key={action.id}>
                  <label>
                    <input
                      type="checkbox"
                      checked={permissions.actions.includes(action.id)}
                      onChange={(e) =>
                        editPermissions((p) => ({
                          ...p,
                          actions: toggleListed(p.actions, action.id, e.target.checked),
                        }))
                      }
                    />
                    Run {action.name}
                  </label>
                </li>
              ))}
              {!(config.actions ?? []).length && <li className="rt-muted">No actions defined.</li>}
            </ul>
          </section>
        ) : (
          <p className="rt-muted">Create a role to choose what its users can see and change.</p>
        )}
      </div>
    </div>
  );
}
