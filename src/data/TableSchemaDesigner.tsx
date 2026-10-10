import { Plus, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { FieldSettingsEditor } from "../fields/FieldSettingsEditor";
import { asTauriError } from "../lib/api";
import { useDocumentConfig } from "../lib/config-store";
import type { AlterTableOperation, TableSchema } from "../lib/types";
import {
  applyTableChanges,
  createIndex,
  dropDatabaseTable,
  dropIndex,
  previewTableChanges,
  tableDropImpact,
} from "../schema/api";
import { ColumnFields } from "../schema/ColumnFields";
import { type ColumnDraft, draftFromColumn, emptyColumn, specFromDraft } from "../schema/columns";
import {
  CheckEditor,
  ColumnPicker,
  ForeignKeyEditor,
  IndexEditor,
  UniqueEditor,
} from "../schema/ConstraintEditors";
import { ImpactDialog } from "../schema/ImpactDialog";
import { modeLabel, storeLabel, storeTypes } from "../schema/logical";
import type { ChangePlan, StoreCapabilities, TableImpact } from "../schema/types";
import "../schema/schema.css";

const same = (a: string[], b: string[]) => a.length === b.length && a.every((x, i) => x === b[i]);
const RENAME_ACK =
  "I understand the definitions listed above still use the old name and must be updated.";

/** Impact preview for dropping an index: in place, no rows removed, the exact statement shown. */
const dropIndexPlan = (table: string, name: string): ChangePlan => ({
  table,
  operations: [{ summary: `Drop index ${name}`, mode: "inPlace", destructive: false }],
  rebuild: false,
  destructive: false,
  statements: [`DROP INDEX "${name.replaceAll('"', '""')}"`],
  warnings: ["Queries and lookups that relied on this index may run slower."],
  impact: null,
});

/**
 * Table designer: changes are staged, previewed by the record store (in place vs table rebuild),
 * and applied in one transaction. Destructive changes and rebuilds go through an impact preview.
 */
export function TableSchemaDesigner({
  schema,
  tables,
  capabilities,
  onChanged,
  onDropped,
  onCancel,
}: {
  schema: TableSchema;
  tables: TableSchema[];
  capabilities: StoreCapabilities | null;
  onChanged: (tableName: string) => Promise<void> | void;
  onDropped: () => Promise<void> | void;
  onCancel: () => void;
}) {
  // DDL can rewrite entity settings in the config: let queued edits land first.
  const { settled } = useDocumentConfig();
  const initialDrafts = useMemo(
    () => Object.fromEntries(schema.columns.map((c) => [c.name, draftFromColumn(c, schema)])),
    [schema],
  );
  const [drafts, setDrafts] = useState<Record<string, ColumnDraft>>(initialDrafts);
  const [tableName, setTableName] = useState(schema.name);
  const [newColumn, setNewColumn] = useState<ColumnDraft>(emptyColumn());
  const [primaryKey, setPrimaryKey] = useState<string[]>(
    schema.primaryKey ?? schema.columns.filter((c) => c.primaryKeyPosition).map((c) => c.name),
  );
  const [pending, setPending] = useState<AlterTableOperation[]>([]);
  const [plan, setPlan] = useState<ChangePlan | null>(null);
  const [planError, setPlanError] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [dialog, setDialog] = useState<
    | { kind: "apply"; plan: ChangePlan }
    | { kind: "drop"; impact: TableImpact }
    | { kind: "dropIndex"; name: string }
    | null
  >(null);
  const types = storeTypes(capabilities);
  const store = storeLabel(capabilities);
  const maxPrecision =
    capabilities?.logicalTypes.find((t) => t.logicalType.startsWith("decimal"))?.maxPrecision ?? 38;
  const columns = schema.columns.map((c) => c.name);

  useEffect(() => {
    if (!pending.length) {
      setPlan(null);
      setPlanError("");
      return;
    }
    let live = true;
    previewTableChanges(schema.name, pending)
      .then((next) => {
        if (!live) return;
        setPlan(next);
        setPlanError("");
      })
      .catch((e) => {
        if (!live) return;
        setPlan(null);
        setPlanError(asTauriError(e).message);
      });
    return () => {
      live = false;
    };
  }, [pending, schema.name]);

  const stage = (...ops: AlterTableOperation[]) => setPending((p) => [...p, ...ops]);
  const run = async (task: () => Promise<void>) => {
    setBusy(true);
    setError("");
    try {
      await task();
    } catch (e) {
      setError(asTauriError(e).message);
    } finally {
      setBusy(false);
    }
  };
  const stageColumn = (name: string) => {
    const before = initialDrafts[name];
    const after = drafts[name];
    const ops: AlterTableOperation[] = [];
    const target = after.name.trim() || name;
    if (target !== name) ops.push({ operation: "rename_column", column: name, newName: target });
    const checkChanged = after.check.trim() !== before.check.trim();
    const changed =
      after.logicalType !== before.logicalType ||
      after.required !== before.required ||
      after.defaultExpression !== before.defaultExpression ||
      after.unique !== before.unique ||
      checkChanged;
    // An untouched check is sent as null (keep, and follow a rename); "" removes it.
    if (changed)
      ops.push({
        operation: "alter_column",
        column: target,
        definition: {
          ...specFromDraft({ ...after, name: target }),
          check: checkChanged ? after.check.trim() : null,
        },
      });
    if (ops.length) stage(...ops);
  };
  const finalName = () =>
    [...pending]
      .reverse()
      .find(
        (op): op is Extract<AlterTableOperation, { operation: "rename_table" }> =>
          op.operation === "rename_table",
      )?.newName ?? schema.name;
  const apply = () =>
    run(async () => {
      const fresh = await previewTableChanges(schema.name, pending);
      // `impact` is also set when a rename leaves definitions using the old name.
      if (fresh.destructive || fresh.rebuild || fresh.impact)
        setDialog({ kind: "apply", plan: fresh });
      else {
        await settled();
        await applyTableChanges(schema.name, pending);
        await onChanged(finalName());
      }
    });
  const confirmApply = () =>
    run(async () => {
      await settled();
      await applyTableChanges(schema.name, pending);
      setDialog(null);
      await onChanged(finalName());
    });

  return (
    <div className="table-designer schema-designer">
      <div className="flex items-center justify-between gap-3">
        <div>
          <h2>Design {schema.name}</h2>
          <p className="mt-1 text-slate-600">
            {store ? `${store} record store.` : "Checking the record store…"} Changes are staged,
            previewed, and applied together in one transaction.
          </p>
        </div>
        <div className="flex gap-2">
          <button
            className="text-red-700"
            disabled={busy}
            onClick={() =>
              run(async () =>
                setDialog({ kind: "drop", impact: await tableDropImpact(schema.name) }),
              )
            }
          >
            <Trash2 aria-hidden="true" />
            Drop table
          </button>
          <button onClick={onCancel}>Close</button>
        </div>
      </div>
      {error && !dialog && (
        <div className="error" role="alert">
          {error}
        </div>
      )}
      <section aria-label="Table">
        <h3>Table</h3>
        <div className="flex flex-wrap items-center gap-3">
          <label className="grid gap-1">
            Table name
            <input value={tableName} onChange={(e) => setTableName(e.target.value)} />
          </label>
          <button
            disabled={!tableName.trim() || tableName === schema.name}
            onClick={() => stage({ operation: "rename_table", newName: tableName.trim() })}
          >
            Rename table
          </button>
        </div>
      </section>
      <section aria-label="Columns">
        <h3>Columns</h3>
        {schema.columns.map((column) => (
          <div
            className="flex flex-wrap items-center gap-2 border-b border-slate-100"
            key={column.name}
          >
            <b className="min-w-28">{column.name}</b>
            <ColumnFields
              draft={drafts[column.name]}
              label={`Column ${column.name}`}
              maxPrecision={maxPrecision}
              types={types}
              onChange={(next) => setDrafts((d) => ({ ...d, [column.name]: next }))}
            />
            <button onClick={() => stageColumn(column.name)}>Stage changes to {column.name}</button>
            <button
              className="text-red-700"
              aria-label={`Drop ${column.name}`}
              onClick={() => stage({ operation: "drop_column", column: column.name })}
            >
              Drop
            </button>
          </div>
        ))}
        <div className="flex flex-wrap items-center gap-2 bg-slate-50 p-3">
          <ColumnFields
            draft={newColumn}
            label="New column"
            maxPrecision={maxPrecision}
            types={types}
            onChange={setNewColumn}
          />
          <button
            disabled={!newColumn.name.trim()}
            onClick={() => {
              stage({ operation: "add_column", column: specFromDraft(newColumn) });
              setNewColumn(emptyColumn());
            }}
          >
            <Plus aria-hidden="true" />
            Add column
          </button>
        </div>
      </section>
      <FieldSettingsEditor table={schema.name} columns={schema.columns} />
      <section aria-label="Primary key">
        <h3>Primary key</h3>
        <ColumnPicker
          label="Primary key columns"
          columns={columns}
          selected={primaryKey}
          onChange={setPrimaryKey}
        />
        <button
          disabled={same(primaryKey, schema.primaryKey ?? [])}
          onClick={() => stage({ operation: "set_primary_key", columns: primaryKey })}
        >
          Set primary key
        </button>
      </section>
      <section aria-label="Relationships">
        <h3>Relationships</h3>
        {schema.foreignKeys.map((key) => (
          <div className="flex flex-wrap items-center gap-3" key={key.id}>
            <span>
              ({key.fromColumns.join(", ")}) → {key.targetTable} ({key.targetColumns.join(", ")}) ·
              update {key.onUpdate} · delete {key.onDelete}
            </span>
            <button
              onClick={() => stage({ operation: "drop_foreign_key", columns: key.fromColumns })}
            >
              Remove relationship ({key.fromColumns.join(", ")})
            </button>
          </div>
        ))}
        <ForeignKeyEditor
          columns={columns}
          tables={tables}
          actions={capabilities?.foreignKeyActions}
          onAdd={(foreignKey) => stage({ operation: "add_foreign_key", foreignKey })}
        />
      </section>
      <section aria-label="Constraints">
        <h3>Unique and check constraints</h3>
        {(schema.uniques ?? []).map((u) => (
          <div key={u.columns.join()}>
            Unique ({u.columns.join(", ")}){" "}
            <button onClick={() => stage({ operation: "drop_unique", columns: u.columns })}>
              Remove unique ({u.columns.join(", ")})
            </button>
          </div>
        ))}
        {(schema.checks ?? []).map((c) => (
          <div key={c.expression}>
            Check ({c.expression}){" "}
            <button
              onClick={() => stage({ operation: "drop_check", expression: c.name ?? c.expression })}
            >
              Remove check ({c.expression})
            </button>
          </div>
        ))}
        <UniqueEditor
          columns={columns}
          onAdd={(cols) => stage({ operation: "add_unique", columns: cols })}
        />
        <CheckEditor onAdd={(expression) => stage({ operation: "add_check", expression })} />
      </section>
      <section aria-label="Indexes">
        <h3>Indexes</h3>
        <p className="text-slate-600">
          Index changes are not staged: they apply on their own and always run in place.
        </p>
        {(schema.indexes ?? []).map((index) => (
          <div key={index.name} className="flex items-center gap-2">
            {index.unique ? "Unique index" : "Index"} {index.name} ({index.columns.join(", ")})
            <button
              disabled={busy}
              onClick={() => setDialog({ kind: "dropIndex", name: index.name })}
            >
              Drop index {index.name}
            </button>
          </div>
        ))}
        <IndexEditor
          table={schema.name}
          columns={columns}
          onAdd={(index) =>
            run(async () => {
              await createIndex({ ...index, table: schema.name });
              await onChanged(schema.name);
            })
          }
        />
      </section>
      <section aria-label="Pending changes" className="pending-changes">
        <h3>Pending changes</h3>
        {!pending.length && <p className="text-slate-600">No staged changes.</p>}
        <ol>
          {pending.map((op, i) => {
            const planned = plan?.operations[i];
            return (
              <li key={i}>
                <span>{planned?.summary ?? op.operation.replaceAll("_", " ")}</span>
                <span className={`mode-badge ${planned?.mode ?? "pending"}`}>
                  {planned ? modeLabel(planned.mode) : "Checking…"}
                </span>
                {planned?.destructive && (
                  <span className="mode-badge destructive">Destructive</span>
                )}
                <button
                  aria-label={`Remove pending change ${i + 1}`}
                  onClick={() => setPending((p) => p.filter((_, j) => j !== i))}
                >
                  <Trash2 aria-hidden="true" />
                </button>
              </li>
            );
          })}
        </ol>
        {planError && (
          <div className="error" role="alert">
            {planError}
          </div>
        )}
        <div className="settings-actions">
          <button disabled={!pending.length} onClick={() => setPending([])}>
            Discard changes
          </button>
          <button
            className="save"
            disabled={busy || !pending.length || !!planError}
            onClick={apply}
          >
            Apply changes
          </button>
        </div>
      </section>
      {dialog?.kind === "apply" && (
        <ImpactDialog
          title={`Apply ${dialog.plan.operations.length} change${dialog.plan.operations.length === 1 ? "" : "s"} to ${schema.name}?`}
          plan={dialog.plan}
          confirmLabel="Apply changes"
          acknowledgement={dialog.plan.impact?.dependents.length ? RENAME_ACK : undefined}
          busy={busy}
          error={error}
          onConfirm={confirmApply}
          onCancel={() => setDialog(null)}
        />
      )}
      {dialog?.kind === "dropIndex" && (
        <ImpactDialog
          title={`Drop index ${dialog.name}?`}
          plan={dropIndexPlan(schema.name, dialog.name)}
          confirmLabel="Drop index"
          acknowledgement="I reviewed the statement; the index can be recreated later."
          busy={busy}
          error={error}
          onConfirm={() =>
            run(async () => {
              await dropIndex(dialog.name);
              setDialog(null);
              await onChanged(schema.name);
            })
          }
          onCancel={() => setDialog(null)}
        />
      )}
      {dialog?.kind === "drop" && (
        <ImpactDialog
          title={`Drop table ${schema.name}?`}
          impact={dialog.impact}
          confirmLabel="Drop table"
          busy={busy}
          error={error}
          onConfirm={() =>
            run(async () => {
              await settled();
              await dropDatabaseTable(schema.name);
              setDialog(null);
              await onDropped();
            })
          }
          onCancel={() => setDialog(null)}
        />
      )}
    </div>
  );
}
