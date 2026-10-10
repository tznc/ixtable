import { Plus, Trash2 } from "lucide-react";
import type { ReactNode } from "react";
import { ActionPicker } from "../automation/ActionPicker";
import type { DbObject } from "../lib/types";
import { readQueries } from "../query/types";
import { newId } from "../lib/utils";
import { filterNames } from "../runtime/conditions";
import { ExpressionField } from "./ExpressionField";
import { type DesignForm, FORM_EVENTS, FORM_MODES, type FormMode } from "./schema";
import { useDesignEditor } from "./useDesignEditor";

/** Properties of the selected form: source, modes, list settings, validation rules, grid. */
export function FormProperties({
  form,
  objects,
  columns,
  layoutSettings,
}: {
  form: DesignForm;
  objects: DbObject[];
  columns: string[];
  layoutSettings: ReactNode;
}) {
  const { config, design, editForm } = useDesignEditor();
  const change = (patch: Partial<DesignForm>, label = "Edit form") =>
    editForm(form.id, (f) => ({ ...f, ...patch }), label);
  const source = form.source;
  const queries = readQueries(config.savedQueries);
  const boundQuery =
    source?.kind === "query" ? queries.find((q) => q.id === source.queryId) : undefined;
  // Bindings left behind by a renamed or removed parameter; checks.rs flags them.
  const declared = new Set((boundQuery?.parameters ?? []).map((p) => p.name));
  const orphanParams =
    source?.kind === "query"
      ? Object.keys(source.params ?? {}).filter((n) => !declared.has(n))
      : [];
  const toggleMode = (mode: FormMode, on: boolean) =>
    change({
      modes: on
        ? FORM_MODES.filter((m) => m === mode || form.modes.includes(m))
        : form.modes.filter((m) => m !== mode),
    });

  return (
    <>
      <label>
        Form name
        <input
          value={form.name}
          onChange={(e) => change({ name: e.target.value }, "Rename form")}
        />
      </label>
      <label>
        Source
        <select
          aria-label="Source kind"
          value={source?.kind ?? ""}
          onChange={(e) => {
            const kind = e.target.value;
            change({
              source:
                kind === "query"
                  ? { kind: "query", queryId: queries[0]?.id ?? "" }
                  : kind === "table"
                    ? { kind: "table", table: "" }
                    : null,
              modes:
                kind === "query"
                  ? form.modes.filter((m) => m === "list" || m === "detail")
                  : form.modes,
            });
          }}
        >
          <option value="">Unbound</option>
          <option value="table">Table</option>
          <option value="query">Saved query (read-only)</option>
        </select>
      </label>
      {source?.kind !== "query" && (
        <label>
          Data table
          <select
            aria-label="Data table"
            value={source?.table ?? ""}
            onChange={(e) =>
              change({ source: e.target.value ? { kind: "table", table: e.target.value } : null })
            }
          >
            <option value="">Unbound</option>
            {objects
              .filter((o) => o.objectType === "table" || o.objectType === "view")
              .map((o) => (
                <option key={o.name}>{o.name}</option>
              ))}
          </select>
        </label>
      )}
      {source?.kind === "query" && (
        <label>
          Saved query
          <select
            value={source.queryId ?? ""}
            onChange={(e) => change({ source: { kind: "query", queryId: e.target.value } })}
          >
            {queries.map((q) => (
              <option key={q.id} value={q.id}>
                {q.name}
              </option>
            ))}
          </select>
        </label>
      )}
      {source?.kind === "query" &&
        ((boundQuery?.parameters?.length ?? 0) > 0 || orphanParams.length > 0) && (
          <fieldset className="fd-fieldset">
            <legend>Query parameters</legend>
            {(boundQuery?.parameters ?? []).map((p) => (
              <ExpressionField
                key={p.name}
                label={`$${p.name}`}
                value={source.params?.[p.name]}
                names={["app", "params"]}
                placeholder={p.required ? "Required" : "Default value"}
                onChange={(expr) => {
                  const params = { ...source.params };
                  if (expr?.trim()) params[p.name] = expr;
                  else delete params[p.name];
                  change({ source: { ...source, params } }, "Bind query parameter");
                }}
              />
            ))}
            {orphanParams.map((name) => (
              <div key={name} className="fd-orphan">
                <span>${name} is no longer a parameter of this query.</span>
                <button
                  type="button"
                  aria-label={`Remove binding $${name}`}
                  onClick={() => {
                    const params = { ...source.params };
                    delete params[name];
                    change({ source: { ...source, params } }, "Remove query parameter binding");
                  }}
                >
                  Remove
                </button>
              </div>
            ))}
          </fieldset>
        )}
      <fieldset className="fd-fieldset">
        <legend>Modes</legend>
        {FORM_MODES.map((mode) => (
          <label key={mode} className="fd-check">
            <input
              type="checkbox"
              checked={form.modes.includes(mode)}
              disabled={source?.kind === "query" && (mode === "create" || mode === "edit")}
              onChange={(e) => toggleMode(mode, e.target.checked)}
            />
            {mode[0].toUpperCase() + mode.slice(1)} mode
          </label>
        ))}
      </fieldset>
      {form.modes.includes("list") && (
        <fieldset className="fd-fieldset">
          <legend>List</legend>
          <label>
            Rows per page
            <input
              type="number"
              min={1}
              max={500}
              value={form.pageSize}
              onChange={(e) => change({ pageSize: Math.max(1, Number(e.target.value) || 25) })}
            />
          </label>
          <label>
            Row opens
            <select
              aria-label="Detail form"
              value={form.detailFormId ?? ""}
              onChange={(e) => change({ detailFormId: e.target.value || null })}
            >
              <option value="">This form</option>
              {design.forms
                .filter((f) => f.id !== form.id)
                .map((f) => (
                  <option key={f.id} value={f.id}>
                    {f.name}
                  </option>
                ))}
            </select>
          </label>
          <ExpressionField
            label="Row filter"
            value={form.filter}
            names={filterNames(columns)}
            placeholder="record.status = 'open'"
            onChange={(filter) => change({ filter }, "Edit list filter")}
          />
          {columns.map((column) => (
            <label key={column} className="fd-check">
              <input
                type="checkbox"
                aria-label={`List column ${column}`}
                checked={form.listColumns.includes(column)}
                onChange={(e) =>
                  change({
                    listColumns: e.target.checked
                      ? columns.filter((c) => c === column || form.listColumns.includes(c))
                      : form.listColumns.filter((c) => c !== column),
                  })
                }
              />
              {column}
            </label>
          ))}
        </fieldset>
      )}
      <fieldset className="fd-fieldset">
        <legend>Form validation</legend>
        {form.rules.map((rule, index) => (
          <div key={rule.id} className="fd-rule">
            <ExpressionField
              label={`Rule ${index + 1}`}
              value={rule.expression}
              columns={columns}
              placeholder="record.end >= record.start"
              onChange={(expression) =>
                change({
                  rules: form.rules.map((r) =>
                    r.id === rule.id ? { ...r, expression: expression ?? "" } : r,
                  ),
                })
              }
            />
            <label>
              Rule {index + 1} message
              <input
                value={rule.message}
                onChange={(e) =>
                  change({
                    rules: form.rules.map((r) =>
                      r.id === rule.id ? { ...r, message: e.target.value } : r,
                    ),
                  })
                }
              />
            </label>
            <button
              type="button"
              aria-label={`Remove rule ${index + 1}`}
              onClick={() => change({ rules: form.rules.filter((r) => r.id !== rule.id) })}
            >
              <Trash2 aria-hidden="true" />
            </button>
          </div>
        ))}
        <button
          type="button"
          onClick={() =>
            change({ rules: [...form.rules, { id: newId(), expression: "", message: "" }] })
          }
        >
          <Plus aria-hidden="true" />
          Add rule
        </button>
      </fieldset>
      <fieldset className="fd-fieldset">
        <legend>Events</legend>
        {FORM_EVENTS.filter(
          // A query-sourced form is read-only, so it never saves.
          (e) => source?.kind !== "query" || e.name === "onLoad" || e.name === "onCurrent",
        ).map((event) => (
          <ActionPicker
            key={event.name}
            label={event.label}
            value={form.events?.[event.name]}
            onChange={(actionId) => {
              const events = { ...form.events };
              if (actionId) events[event.name] = actionId;
              else delete events[event.name];
              change({ events }, `Set ${event.label.toLowerCase()} action`);
            }}
          />
        ))}
      </fieldset>
      {layoutSettings}
    </>
  );
}
