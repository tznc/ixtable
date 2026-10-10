import { useEffect, useState } from "react";
import { inspectTable } from "../lib/api";
import { useDocumentConfig } from "../lib/config-store";
import { QueryPicker } from "../query/QueryPicker";
import { useShell } from "../shell/context";
import { ActionPicker } from "./ActionPicker";
import { ExprInput, SelectField, TextField, ValueMapEditor } from "./fields";
import { newStep, STEP_LABELS } from "./steps";
import { STEP_KINDS, type Step, type StepKind } from "./types";

function useTables() {
  const { objects } = useShell();
  return objects
    .filter((o) => o.objectType === "table" && !o.name.startsWith("_ixtable_"))
    .map((o) => ({ value: o.name, label: o.name }));
}

function useColumns(table: string): string[] {
  const [columns, setColumns] = useState<string[]>([]);
  useEffect(() => {
    let live = true;
    if (!table) return;
    inspectTable(table)
      .then((schema) => live && setColumns(schema.columns.map((c) => c.name)))
      .catch(() => live && setColumns([]));
    return () => {
      live = false;
    };
  }, [table]);
  return table ? columns : [];
}

/** Ordered step list with add/move/remove; `label` names the list for assistive tech. */
export function StepList({
  steps,
  onChange,
  actionId,
  label,
}: {
  steps: Step[];
  onChange: (steps: Step[]) => void;
  actionId: string;
  label: string;
}) {
  const [kind, setKind] = useState<StepKind>("updateRecord");
  const move = (index: number, delta: number) => {
    const next = [...steps];
    const [item] = next.splice(index, 1);
    next.splice(index + delta, 0, item);
    onChange(next);
  };
  return (
    <section aria-label={label} className="ax-editor-steps">
      {steps.length === 0 && <p>No steps yet.</p>}
      {steps.map((step, index) => {
        const name = `${label} ${index + 1}: ${STEP_LABELS[step.kind] ?? step.kind}`;
        return (
          <fieldset key={step.id || index} className="ax-step" aria-label={name}>
            <legend>{name}</legend>
            <div className="ax-row">
              <div className="ax-step-tools">
                <button
                  type="button"
                  aria-label={`Move ${label.toLowerCase()} ${index + 1} up`}
                  disabled={index === 0}
                  onClick={() => move(index, -1)}
                >
                  ↑
                </button>
                <button
                  type="button"
                  aria-label={`Move ${label.toLowerCase()} ${index + 1} down`}
                  disabled={index === steps.length - 1}
                  onClick={() => move(index, 1)}
                >
                  ↓
                </button>
                <button
                  type="button"
                  aria-label={`Remove ${label.toLowerCase()} ${index + 1}`}
                  onClick={() => onChange(steps.filter((_, i) => i !== index))}
                >
                  Remove
                </button>
              </div>
            </div>
            <StepFields
              step={step}
              actionId={actionId}
              onChange={(next) => onChange(steps.map((s, i) => (i === index ? next : s)))}
            />
          </fieldset>
        );
      })}
      <div className="ax-row">
        <SelectField
          label={`New ${label.toLowerCase()} kind`}
          value={kind}
          onChange={setKind}
          options={STEP_KINDS.map((k) => ({ value: k, label: STEP_LABELS[k] }))}
        />
        <button type="button" onClick={() => onChange([...steps, newStep(kind)])}>
          Add {label.toLowerCase()}
        </button>
      </div>
    </section>
  );
}

function StepFields({
  step,
  onChange,
  actionId,
}: {
  step: Step;
  onChange: (step: Step) => void;
  actionId: string;
}) {
  const { config } = useDocumentConfig();
  const tables = useTables();
  const table = "table" in step ? step.table : "";
  const columns = useColumns(table);
  const set = (patch: Partial<Step>) => onChange({ ...step, ...patch } as Step);
  const forms = (config.design?.forms ?? []).map((f) => ({ value: f.id, label: f.name || f.id }));
  const reports = (config.reports ?? []).map((r) => ({ value: r.id, label: r.name || r.id }));
  const dashboards = (config.dashboards ?? []).map((d) => ({ value: d.id, label: d.name || d.id }));
  const when =
    step.kind === "condition" ? null : (
      <ExprInput
        label="Only when"
        placeholder="always"
        value={step.when}
        onChange={(v) => set({ when: v || undefined })}
      />
    );
  const tableField = (
    <SelectField label="Table" value={table} onChange={(v) => set({ table: v })} options={tables} />
  );
  const match = (m: Step & { match: unknown }) => (
    <>
      <SelectField
        label="Which rows"
        value={m.match === "current" ? "current" : "match"}
        onChange={(v) => set({ match: v === "current" ? "current" : {} } as Partial<Step>)}
        options={[
          { value: "current", label: "The current record" },
          { value: "match", label: "Rows matching columns" },
        ]}
      />
      {m.match !== "current" && (
        <ValueMapEditor
          label="Match"
          value={m.match as Record<string, string>}
          columns={columns}
          onChange={(v) => set({ match: v } as Partial<Step>)}
        />
      )}
    </>
  );
  switch (step.kind) {
    case "createRecord":
      return (
        <>
          <div className="ax-row">
            {tableField}
            <TextField
              label="Store result as"
              value={step.storeAs}
              onChange={(v) => set({ storeAs: v || undefined })}
            />
            {when}
          </div>
          <ValueMapEditor
            label="Values"
            value={step.values}
            columns={columns}
            onChange={(v) => set({ values: v })}
          />
        </>
      );
    case "updateRecord":
      return (
        <>
          <div className="ax-row">
            {tableField}
            {when}
          </div>
          {match(step)}
          <ValueMapEditor
            label="Values"
            value={step.values}
            columns={columns}
            onChange={(v) => set({ values: v })}
          />
        </>
      );
    case "deleteRecord":
      return (
        <>
          <div className="ax-row">
            {tableField}
            {when}
          </div>
          {match(step)}
        </>
      );
    case "runQuery":
      return (
        <>
          <div className="ax-row">
            <QueryPicker value={step.queryId} onChange={(v) => set({ queryId: v })} actions />
            <TextField
              label="Store rows as"
              value={step.storeAs}
              onChange={(v) => set({ storeAs: v })}
            />
            {when}
          </div>
          <ValueMapEditor
            label="Parameters"
            keyLabel="Parameter"
            value={step.params}
            onChange={(v) => set({ params: v })}
          />
        </>
      );
    case "navigate": {
      const targets =
        step.target.kind === "form"
          ? forms
          : step.target.kind === "report"
            ? reports
            : step.target.kind === "dashboard"
              ? dashboards
              : tables;
      return (
        <div className="ax-row">
          <SelectField
            label="Target kind"
            value={step.target.kind}
            onChange={(k) => set({ target: { kind: k, id: "" } })}
            options={["form", "report", "dashboard", "table"].map((k) => ({
              value: k as "form",
              label: k,
            }))}
          />
          <SelectField
            label="Target"
            value={step.target.id}
            onChange={(id) => set({ target: { ...step.target, id } })}
            options={targets}
          />
          <TextField
            label="Mode"
            value={step.target.mode}
            onChange={(m) => set({ target: { ...step.target, mode: m || undefined } })}
          />
          <ExprInput
            label="Record id"
            value={step.target.recordId}
            onChange={(r) => set({ target: { ...step.target, recordId: r || undefined } })}
          />
          {when}
        </div>
      );
    }
    case "openForm":
      return (
        <div className="ax-row">
          <SelectField
            label="Form"
            value={step.formId}
            onChange={(v) => set({ formId: v })}
            options={forms}
          />
          <SelectField
            label="Form mode"
            value={step.mode ?? "detail"}
            onChange={(v) => set({ mode: v })}
            options={["list", "detail", "create", "edit"].map((m) => ({ value: m, label: m }))}
          />
          <ExprInput
            label="Record id"
            value={step.recordId}
            onChange={(v) => set({ recordId: v || undefined })}
          />
          {when}
        </div>
      );
    case "openReport":
      return (
        <>
          <div className="ax-row">
            <SelectField
              label="Report"
              value={step.reportId}
              onChange={(v) => set({ reportId: v })}
              options={reports}
            />
            {when}
          </div>
          <ValueMapEditor
            label="Parameters"
            keyLabel="Parameter"
            value={step.params}
            onChange={(v) => set({ params: v })}
          />
        </>
      );
    case "openDashboard":
      return (
        <>
          <div className="ax-row">
            <SelectField
              label="Dashboard"
              value={step.dashboardId}
              onChange={(v) => set({ dashboardId: v })}
              options={dashboards}
            />
            {when}
          </div>
          <ValueMapEditor
            label="Parameters"
            keyLabel="Parameter"
            value={step.params}
            onChange={(v) => set({ params: v })}
          />
        </>
      );
    case "setState":
      return (
        <div className="ax-row">
          <SelectField
            label="Scope"
            value={step.scope}
            onChange={(v) => set({ scope: v })}
            options={[
              { value: "app", label: "Application" },
              { value: "form", label: "Form" },
            ]}
          />
          <TextField label="Key" value={step.key} onChange={(v) => set({ key: v })} />
          <ExprInput
            label="Value"
            required
            value={step.value}
            onChange={(v) => set({ value: v })}
          />
          {when}
        </div>
      );
    case "confirm":
      return (
        <div className="ax-row">
          <ExprInput
            label="Message"
            required
            value={step.message}
            onChange={(v) => set({ message: v })}
          />
          {when}
        </div>
      );
    case "message":
      return (
        <div className="ax-row">
          <ExprInput label="Text" required value={step.text} onChange={(v) => set({ text: v })} />
          <SelectField
            label="Tone"
            value={step.tone ?? "info"}
            onChange={(v) => set({ tone: v })}
            options={[
              { value: "info", label: "Information" },
              { value: "error", label: "Error" },
            ]}
          />
          {when}
        </div>
      );
    case "fail":
      return (
        <div className="ax-row">
          <ExprInput
            label="Failure message"
            required
            value={step.message}
            onChange={(v) => set({ message: v })}
          />
          {when}
        </div>
      );
    case "condition":
      return (
        <>
          <ExprInput label="If" required value={step.when} onChange={(v) => set({ when: v })} />
          <StepList
            label="Then step"
            actionId={actionId}
            steps={step.then ?? []}
            onChange={(then) => set({ then })}
          />
          <StepList
            label="Else step"
            actionId={actionId}
            steps={step.else ?? []}
            onChange={(otherwise) => set({ else: otherwise })}
          />
        </>
      );
    case "runAction":
      return (
        <div className="ax-row">
          <ActionPicker
            label="Action to run"
            value={step.actionId}
            exclude={actionId}
            onChange={(v) => set({ actionId: v ?? "" })}
          />
          {when}
        </div>
      );
    default:
      return <p>Unknown step kind.</p>;
  }
}
