import { type FocusEvent, useMemo, useState } from "react";
import { runAction } from "../automation/runner";
import type { DesignControl, DesignForm } from "../design/schema";
import { useDocumentConfig } from "../lib/config-store";
import { deleteRecord } from "../lib/records";
import type { DataValue } from "../lib/types";
import { type BodyContext, ControlGrid } from "./FormBody";
import { tableSchema } from "./data";
import {
  defaultRecord,
  enabledControls,
  type FormErrors,
  type FormScope,
  hasErrors,
  validateControl,
  validateForm,
  visibleControls,
} from "./formState";
import { useRuntimeNavigation } from "./navigation";
import { can, PermissionError } from "./rbac";
import { createRow, expectedValues, updateRow, valuesToWrite } from "./rowWrites";
import { type RecordValues, sameValue } from "./values";

type Props = {
  form: DesignForm;
  /** Source table; null for read-only (query) sources. */
  table: string | null;
  /** The stored row; absent for the new-record row. */
  record?: RecordValues;
  identity?: DataValue[] | null;
  /** Accessible name of the row. */
  label: string;
  canUpdate: boolean;
  canDelete: boolean;
  confirm: (message: string) => Promise<boolean>;
  /** Called after this row was saved or deleted, so the view re-reads its page. */
  onChanged: (message: string, tone?: "info" | "error") => void;
};

const NO_ERRORS: FormErrors = { fields: {}, form: [] };
const text = (reason: unknown) => {
  if (reason && typeof reason === "object" && "code" in reason && reason.code === "CONFLICT")
    return "Someone else changed this record. The list was reloaded with the latest values.";
  return reason instanceof Error ? reason.message : String(reason);
};

/**
 * One record of a continuous form: the form's own grid and controls, edited in place.
 * Edits save with the row's Save button, Enter, or when focus leaves a changed row
 * (as in Access). The row without `record` is the new-record row at the end.
 */
export function ContinuousRow({
  form,
  table,
  record: stored,
  identity = null,
  label,
  canUpdate,
  canDelete,
  confirm,
  onChanged,
}: Props) {
  const { config } = useDocumentConfig();
  const runtime = useRuntimeNavigation();
  const { app, roleId } = runtime;
  const creating = !stored;
  const [original] = useState<RecordValues>(() => stored ?? defaultRecord(form, { form: {}, app }));
  const [record, setRecord] = useState<RecordValues>(original);
  const [formState, setFormState] = useState<Record<string, unknown>>({});
  const [errors, setErrors] = useState<FormErrors>(NO_ERRORS);
  const [problem, setProblem] = useState("");
  const [busy, setBusy] = useState(false);
  const scope: FormScope = useMemo(
    () => ({ record, form: formState, app }),
    [record, formState, app],
  );
  const editable = creating ? !!table : canUpdate;
  const dirty = Object.keys(record).some((k) => !sameValue(record[k], original[k]));
  const locked = useMemo(() => new Set<string>(), []);

  const save = async () => {
    if (!table || busy || !dirty) return;
    const result = validateForm(form, scope);
    setErrors(result);
    if (hasErrors(result)) {
      setProblem("Fix the highlighted problems before saving.");
      return;
    }
    setBusy(true);
    setProblem("");
    try {
      const schema = await tableSchema(table);
      const values = valuesToWrite(form, scope, original);
      if (creating) {
        if (!editable) throw new PermissionError("This role cannot create records here.");
        const outcome = await createRow(table, schema, values);
        setRecord(original);
        onChanged(outcome.problem ?? "Record created.", outcome.problem ? "error" : "info");
      } else {
        if (!canUpdate) throw new PermissionError("This role cannot change records here.");
        if (!identity) throw new Error("This record cannot be identified for saving.");
        const outcome = await updateRow(table, schema, identity, values, original);
        onChanged(outcome.problem ?? "Changes saved.", outcome.problem ? "error" : "info");
      }
    } catch (reason) {
      setProblem(text(reason));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!table || !identity || !canDelete) return;
    if (!(await confirm("Delete this record? This cannot be undone."))) return;
    try {
      const schema = await tableSchema(table);
      await deleteRecord(table, identity, {
        expected: expectedValues(schema, original),
        old: original,
      });
      onChanged("Record deleted.");
    } catch (reason) {
      setProblem(text(reason));
    }
  };

  const runButton = (control: DesignControl) => {
    if (!control.actionId || busy) return;
    runAction(control.actionId, {
      config,
      record: { ...record },
      ...(!creating && table && { snapshot: { ...original } }),
      form: formState,
      app,
      navigate: (target) => runtime.navigate({ ...target, navId: undefined }),
      setState: (where, key, value) =>
        where === "form"
          ? setFormState((s) => ({ ...s, [key]: value }))
          : runtime.setAppState(key, value),
      confirm,
      notify: (message, tone = "info") => onChanged(message, tone),
      authorize: (kind, id, op) => can(config, roleId, kind, id, op as "read"),
      refresh: () => onChanged(""),
    })
      .then((result) => !result.ok && result.error && setProblem(result.error))
      .catch((reason) => setProblem(text(reason)));
  };

  const ctx: BodyContext = {
    form,
    scope,
    visible: visibleControls(form, scope),
    enabled: enabledControls(form, scope),
    errors: errors.fields,
    readOnly: !editable,
    locked,
    // One level of master/detail: a repeated row holds no related lists.
    embedded: true,
    identity,
    setField: (column, value) => setRecord((current) => ({ ...current, [column]: value })),
    blur: (control) => {
      const column = control.binding?.column;
      if (!column || !editable) return;
      const message = validateControl(control, record[column], scope);
      setErrors((current) => {
        const fields = { ...current.fields };
        if (message) fields[control.id] = message;
        else delete fields[control.id];
        return { ...current, fields };
      });
    },
    runButton,
    canRunButton: (control) =>
      !control.actionId || (!busy && can(config, roleId, "action", control.actionId, "execute")),
  };

  // Leaving a changed stored row saves it; the new-record row waits for Add.
  const leave = (event: FocusEvent<HTMLDivElement>) => {
    const next = event.relatedTarget as Node | null;
    if (!creating && dirty && next && !event.currentTarget.contains(next))
      save().catch(() => undefined);
  };

  return (
    // biome-ignore lint/a11y/useSemanticElements: a fieldset would restyle the row's grid.
    <div
      className={`rt-crow${creating ? " rt-crow-new" : ""}${dirty ? " rt-crow-dirty" : ""}`}
      role="group"
      aria-label={label}
      onBlur={leave}
      onKeyDown={(event) => {
        const target = event.target as HTMLElement;
        if (event.key !== "Enter" || target.tagName !== "INPUT" || !editable) return;
        event.preventDefault();
        save().catch(() => undefined);
      }}
    >
      <ControlGrid ctx={ctx} parent={null} />
      <div className="rt-crow-actions">
        {editable && (creating || dirty) && (
          <button
            type="button"
            className={creating ? "primary" : undefined}
            disabled={busy || !dirty}
            onClick={() => save().catch(() => undefined)}
          >
            {creating ? "Add" : "Save"}
          </button>
        )}
        {!creating && dirty && (
          <button type="button" onClick={() => setRecord(original)} disabled={busy}>
            Undo
          </button>
        )}
        {!creating && canDelete && (
          <button type="button" onClick={() => remove().catch(() => undefined)} disabled={busy}>
            Delete
          </button>
        )}
      </div>
      {(problem || errors.form.length > 0) && (
        <p className="rt-error" role="alert">
          {[problem, ...errors.form].filter(Boolean).join(" ")}
        </p>
      )}
    </div>
  );
}
