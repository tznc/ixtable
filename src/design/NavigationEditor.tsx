import { ArrowDown, ArrowUp, FolderPlus, Plus, Trash2 } from "lucide-react";
import type { DbObject } from "../lib/types";
import { newId } from "../lib/utils";
import { mapNavigation, regroupNavigation, shiftNavigation, takeNavigation } from "./operations";
import { flattenNavigation, FORM_MODES, type NavigationItem, type NavKind } from "./schema";
import { useDesignEditor } from "./useDesignEditor";

const KINDS: NavKind[] = ["form", "report", "dashboard", "table", "group"];

/** Navigation tree (nested groups) and the start page. */
export function NavigationEditor({ objects }: { objects: DbObject[] }) {
  const { config, design, editDesign } = useDesignEditor();
  const all = flattenNavigation(design.navigation);
  const groups = all.filter((item) => item.kind === "group");
  const targets: Record<NavKind, { id: string; name: string }[]> = {
    form: design.forms.map((f) => ({ id: f.id, name: f.name })),
    report: (config.reports ?? []).map((r) => ({ id: r.id, name: r.name })),
    dashboard: (config.dashboards ?? []).map((d) => ({ id: d.id, name: d.name })),
    table: objects
      .filter((o) => o.objectType === "table")
      .map((o) => ({ id: o.name, name: o.name })),
    group: [],
  };
  const edit = (change: (items: NavigationItem[]) => NavigationItem[], label = "Edit navigation") =>
    editDesign((d) => ({ ...d, navigation: change(d.navigation) }), label);
  const patch = (id: string, values: Partial<NavigationItem>) =>
    edit((items) => mapNavigation(items, id, (item) => ({ ...item, ...values })));
  const add = (kind: NavKind) => {
    const item: NavigationItem = {
      id: newId(),
      label: kind === "group" ? "New group" : (targets.form[0]?.name ?? "New page"),
      kind,
      targetId: kind === "group" ? null : (targets.form[0]?.id ?? null),
      children: [],
    };
    editDesign(
      (d) => ({
        ...d,
        navigation: [...d.navigation, item],
        startPage: d.startPage ?? (kind === "group" ? null : item.id),
      }),
      "Add navigation item",
    );
  };
  const parentOf = (id: string) =>
    groups.find((g) => (g.children ?? []).some((c) => c.id === id))?.id ?? "";

  const renderItems = (items: NavigationItem[], depth: number) => (
    <ul className="fd-nav-list" aria-label={depth ? undefined : "Navigation items"}>
      {items.map((item) => (
        <li key={item.id}>
          <div className="fd-nav-item">
            <input
              aria-label="Navigation label"
              value={item.label}
              onChange={(e) => patch(item.id, { label: e.target.value })}
            />
            <select
              aria-label={`${item.label} kind`}
              value={item.kind}
              onChange={(e) => {
                const kind = e.target.value as NavKind;
                patch(item.id, {
                  kind,
                  targetId: kind === "group" ? null : (targets[kind][0]?.id ?? null),
                });
              }}
            >
              {KINDS.map((kind) => (
                <option key={kind} value={kind}>
                  {kind}
                </option>
              ))}
            </select>
            {item.kind !== "group" && (
              <select
                aria-label={`${item.label} opens`}
                value={item.targetId ?? ""}
                onChange={(e) => patch(item.id, { targetId: e.target.value || null })}
              >
                <option value="">—</option>
                {targets[item.kind].map((t) => (
                  <option key={t.id} value={t.id}>
                    {t.name}
                  </option>
                ))}
              </select>
            )}
            {item.kind === "form" && (
              <select
                aria-label={`${item.label} mode`}
                value={item.mode ?? ""}
                onChange={(e) =>
                  patch(item.id, { mode: (e.target.value || null) as NavigationItem["mode"] })
                }
              >
                <option value="">Default mode</option>
                {FORM_MODES.filter((m) => m !== "detail" && m !== "edit").map((mode) => (
                  <option key={mode} value={mode}>
                    {mode}
                  </option>
                ))}
              </select>
            )}
            <select
              aria-label={`${item.label} group`}
              value={parentOf(item.id)}
              onChange={(e) =>
                edit(
                  (items) => regroupNavigation(items, item.id, e.target.value || null),
                  "Move navigation item",
                )
              }
            >
              <option value="">Top level</option>
              {groups
                .filter((g) => g.id !== item.id)
                .map((g) => (
                  <option key={g.id} value={g.id}>
                    In {g.label}
                  </option>
                ))}
            </select>
            <button
              type="button"
              aria-label={`Move ${item.label} up`}
              onClick={() => edit((items) => shiftNavigation(items, item.id, -1))}
            >
              <ArrowUp aria-hidden="true" />
            </button>
            <button
              type="button"
              aria-label={`Move ${item.label} down`}
              onClick={() => edit((items) => shiftNavigation(items, item.id, 1))}
            >
              <ArrowDown aria-hidden="true" />
            </button>
            <button
              type="button"
              aria-label={`Remove ${item.label}`}
              onClick={() =>
                edit((items) => takeNavigation(items, item.id).items, "Remove navigation item")
              }
            >
              <Trash2 aria-hidden="true" />
            </button>
          </div>
          {item.kind === "group" && renderItems(item.children ?? [], depth + 1)}
        </li>
      ))}
    </ul>
  );

  return (
    <section className="fd-navigation" aria-label="Navigation editor">
      <h2>Navigation</h2>
      <p className="fd-hint">
        Groups nest navigation items. Roles can hide items in Settings › Roles.
      </p>
      {renderItems(design.navigation, 0)}
      <div className="fd-actions">
        <button type="button" onClick={() => add("form")}>
          <Plus aria-hidden="true" />
          Add navigation item
        </button>
        <button type="button" onClick={() => add("group")}>
          <FolderPlus aria-hidden="true" />
          Add group
        </button>
      </div>
      <label>
        Start page
        <select
          value={design.startPage ?? ""}
          onChange={(e) =>
            editDesign((d) => ({ ...d, startPage: e.target.value || null }), "Set start page")
          }
        >
          <option value="">First item</option>
          {all
            .filter((item) => item.kind !== "group")
            .map((item) => (
              <option key={item.id} value={item.id}>
                {item.label}
              </option>
            ))}
        </select>
      </label>
    </section>
  );
}
