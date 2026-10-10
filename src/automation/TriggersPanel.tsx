import { useId, useState } from "react";
import { useDocumentConfig } from "../lib/config-store";
import { newId } from "../lib/utils";
import { useShell } from "../shell/context";
import { ActionPicker } from "./ActionPicker";
import { ExprInput, NumberField, SelectField, TextField } from "./fields";
import type { Trigger } from "./types";

export function TriggersPanel({ focusId }: { focusId?: string }) {
  const { config, update } = useDocumentConfig();
  const { objects } = useShell();
  const triggers = config.triggers ?? [];
  const [selected, setSelected] = useState<string | null>(focusId ?? triggers[0]?.id ?? null);
  const trigger = triggers.find((t) => t.id === selected) ?? null;
  const before = trigger?.event === "beforeChange";
  const enabledId = useId();
  const tables = objects
    .filter((o) => o.objectType === "table" && !o.name.startsWith("_ixtable_"))
    .map((o) => ({ value: o.name, label: o.name }));

  const edit = (patch: Partial<Trigger>) =>
    update(
      (draft) => ({
        ...draft,
        triggers: draft.triggers.map((t) => (t.id === selected ? { ...t, ...patch } : t)),
      }),
      "Edit trigger",
    );
  const add = () => {
    const created: Trigger = {
      id: newId(),
      name: `Trigger ${triggers.length + 1}`,
      table: tables[0]?.value ?? "",
      event: "created",
      actionId: config.actions[0]?.id ?? "",
      mode: "sync",
      enabled: true,
      maxAttempts: 3,
      backoffMs: 1000,
    };
    setSelected(created.id);
    return update((draft) => ({ ...draft, triggers: [...draft.triggers, created] }), "Add trigger");
  };
  const remove = () => {
    const index = triggers.findIndex((t) => t.id === selected);
    setSelected(triggers[index + 1]?.id ?? triggers[index - 1]?.id ?? null);
    return update(
      (draft) => ({ ...draft, triggers: draft.triggers.filter((t) => t.id !== selected) }),
      "Delete trigger",
    );
  };

  return (
    <div className="ax-split">
      <nav className="ax-list" aria-label="Triggers">
        <button type="button" onClick={add}>
          New trigger
        </button>
        <ul>
          {triggers.map((t) => (
            <li key={t.id}>
              <button
                type="button"
                aria-current={t.id === selected ? "true" : undefined}
                onClick={() => setSelected(t.id)}
              >
                {t.name || "Untitled trigger"}
                {t.enabled ? "" : " (off)"}
              </button>
            </li>
          ))}
        </ul>
        {!triggers.length && <p>No triggers yet.</p>}
      </nav>
      {trigger ? (
        <div className="ax-editor" aria-label={`Trigger ${trigger.name}`}>
          <div className="ax-row">
            <TextField
              label="Trigger name"
              value={trigger.name}
              onChange={(name) => edit({ name })}
            />
            <label className="ax-check" htmlFor={enabledId}>
              <input
                id={enabledId}
                type="checkbox"
                checked={trigger.enabled}
                onChange={(e) => edit({ enabled: e.target.checked })}
              />
              Enabled
            </label>
            <button type="button" onClick={remove}>
              Delete trigger
            </button>
          </div>
          <div className="ax-row">
            <SelectField
              label="Table"
              value={trigger.table}
              onChange={(table) => edit({ table })}
              options={tables}
            />
            <SelectField
              label="Event"
              value={trigger.event}
              onChange={(event) =>
                edit(event === "beforeChange" ? { event, mode: "sync" } : { event })
              }
              options={[
                { value: "beforeChange", label: "Before a record is saved" },
                { value: "created", label: "Record created" },
                { value: "updated", label: "Record updated" },
                { value: "deleted", label: "Record deleted" },
              ]}
            />
            <ActionPicker
              value={trigger.actionId}
              onChange={(actionId) => edit({ actionId: actionId ?? "" })}
            />
          </div>
          <ExprInput
            label="Condition"
            placeholder="always"
            value={trigger.condition}
            onChange={(condition) => edit({ condition: condition || undefined })}
          />
          <SelectField
            label="Run as"
            value={trigger.runAs ?? "app"}
            onChange={(runAs) => edit({ runAs })}
            options={[
              { value: "app", label: "App — the trigger's own steps" },
              { value: "user", label: "Signed-in user's role" },
            ]}
          />
          {before ? (
            <p>
              Runs before each new or changed record is saved, including rows an action query
              writes. Its action can set fields with Set field steps, read queries, and reject the
              save with a Fail step; it cannot write records or open anything. In the condition and
              action, <code>old</code> is the row before an update and empty for a new record.
            </p>
          ) : (
            <div className="ax-row">
              <SelectField
                label="Run"
                value={trigger.mode}
                onChange={(mode) => edit({ mode })}
                options={[
                  { value: "sync", label: "Synchronously, inside the write" },
                  { value: "async", label: "Asynchronously, on the job queue" },
                ]}
              />
              {trigger.mode === "async" && (
                <>
                  <NumberField
                    label="Max attempts"
                    min={1}
                    value={trigger.maxAttempts}
                    onChange={(maxAttempts) => edit({ maxAttempts })}
                  />
                  <NumberField
                    label="Retry backoff (ms)"
                    value={trigger.backoffMs}
                    onChange={(backoffMs) => edit({ backoffMs })}
                  />
                </>
              )}
            </div>
          )}
          {!before && trigger.mode === "async" && (
            <ExprInput
              label="Idempotency key"
              placeholder="trigger id : table : key : event : hash of values"
              value={trigger.idempotencyKey}
              onChange={(idempotencyKey) => edit({ idempotencyKey: idempotencyKey || undefined })}
            />
          )}
          <p>
            Created, updated and deleted triggers see the record as <code>record</code> (a deleted
            record as it was). Sync triggers run after the record is saved, as part of the same
            operation; a failing action reports an error to whoever saved the record, but the save
            stays. Async triggers run in the background while the app is open, with retries. Run as
            app lets the trigger make its own writes even when the user's role cannot; run as the
            signed-in user refuses the save up front when the user's role cannot run the trigger.
          </p>
        </div>
      ) : (
        <div className="ax-editor">
          <p>Select or create a trigger.</p>
        </div>
      )}
    </div>
  );
}
