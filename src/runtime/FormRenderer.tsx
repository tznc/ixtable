import { useState } from "react";
import { type BackLink, BackCrumb } from "./BackCrumb";
import type { FormMode } from "../design/schema";
import { useDocumentConfig } from "../lib/config-store";
import { ContinuousView } from "./ContinuousView";
import type { RecordCursor } from "./cursor";
import { ListView } from "./ListView";
import { RecordNavBar } from "./RecordNavBar";
import { SplitView } from "./SplitView";
import { type PageKind, useRuntimeNavigation } from "./navigation";
import { can } from "./rbac";
import { type Link, RecordView } from "./RecordView";
import { isDesignedForm, resolveForm } from "./registry";
import "./runtime.css";
import "./form-views.css";

export type FormRendererProps = {
  formId: string;
  mode?: FormMode;
  recordId?: unknown;
  /** Fixed foreign-key value for a child form opened from a related-record list. */
  link?: Link;
  /** Embedded child form: no related lists of its own (one level of master/detail). */
  embedded?: boolean;
  /** Called when the form is closed from its first view (embedded or after delete). */
  onDone?: () => void;
  /** Back link to the previous runtime page, shown while the form is on its first view. */
  back?: BackLink | null;
  /** Receives an embedded form's messages (it closes after saving), to show near the list. */
  onNotify?: (text: string, tone: "info" | "error") => void;
  /** Page parameters (navigation `params`, dashboard filter values) for query source bindings. */
  params?: Record<string, unknown>;
  /** Told whether the shown record has unsaved edits. */
  onDirty?: (dirty: boolean) => void;
  /** Receives the saved values after a create or edit commits (popup forms). */
  onSaved?: (record: Record<string, unknown>) => void;
};

/** `cursor`: the record's place in the list it was opened from (record navigation bar). */
type View = { formId: string; mode: FormMode; recordId?: unknown; cursor?: RecordCursor | null };

/**
 * Renders a form in list, detail, create, or edit mode. Navigation between list and
 * detail stays inside the renderer (with a back link), so it also works embedded in dashboards.
 */
export function FormRenderer(props: FormRendererProps) {
  const key = JSON.stringify([props.formId, props.mode ?? null, props.recordId ?? null]);
  return <FormStack key={key} {...props} />;
}

function FormStack({
  formId,
  mode,
  recordId,
  link,
  embedded = false,
  onDone,
  back = null,
  onNotify,
  params,
  onDirty,
  onSaved,
}: FormRendererProps) {
  const { config } = useDocumentConfig();
  const runtime = useRuntimeNavigation();
  const initial = resolveForm(config, formId);
  // Record navigation is blocked while the shown record has unsaved edits.
  const [dirty, setDirty] = useState(false);
  const [stack, setStack] = useState<View[]>([
    { formId, mode: mode ?? initial?.modes[0] ?? "list", recordId },
  ]);
  const view = stack[stack.length - 1];
  const form = resolveForm(config, view.formId);
  if (!form)
    return (
      <p className="rt-error" role="alert">
        This form does not exist.
      </p>
    );
  const subject = isDesignedForm(config, form)
    ? { kind: "form", id: form.id }
    : { kind: "table", id: form.source?.table ?? "" };
  if (!can(config, runtime.roleId, subject.kind, subject.id, "read"))
    return (
      <p className="rt-error" role="alert">
        You do not have access to {form.name}.
      </p>
    );

  const push = (next: View) => setStack((items) => [...items, next]);
  const replace = (next: View) => setStack((items) => [...items.slice(0, -1), next]);
  const close = () => {
    if (stack.length > 1) setStack((items) => items.slice(0, -1));
    else (onDone ?? back?.onBack)?.();
  };
  // One back affordance: to the view this one was opened from, else to the previous page.
  const previous = stack.length > 1 ? stack[stack.length - 2] : null;
  const crumb: BackLink | null = embedded
    ? null
    : previous
      ? { label: resolveForm(config, previous.formId)?.name ?? "previous view", onBack: close }
      : back;
  const navigate = (target: { kind: string; id: string; mode?: string; recordId?: unknown }) => {
    if (runtime.attached && !embedded) {
      runtime.navigate({ ...target, kind: target.kind as PageKind });
      return;
    }
    if (target.kind === "form")
      push({
        formId: target.id,
        mode: (target.mode as FormMode) ?? "list",
        recordId: target.recordId,
      });
  };

  if (view.mode === "list") {
    const detailId = form.detailFormId || form.id;
    return (
      <div className="rt-form">
        {crumb && <BackCrumb back={crumb} />}
        <ListView
          form={form}
          onOpen={(id, cursor) => push({ formId: detailId, mode: "detail", recordId: id, cursor })}
          onCreate={() => push({ formId: detailId, mode: "create" })}
          params={params}
        />
      </div>
    );
  }
  if (view.mode === "continuous" || view.mode === "split")
    return (
      <div className="rt-form">
        {crumb && <BackCrumb back={crumb} />}
        {view.mode === "continuous" ? (
          <ContinuousView form={form} params={params} />
        ) : (
          <SplitView form={form} params={params} onNavigate={navigate} />
        )}
      </div>
    );
  // The bar walks the list the record was opened from, else this form's own records.
  const listForm = (view.cursor && resolveForm(config, view.cursor.formId)) || form;
  const canCreate =
    form.modes.includes("create") &&
    form.source?.kind === "table" &&
    can(config, runtime.roleId, subject.kind, subject.id, "create");
  const showBar = !!form.navigationBar && view.mode === "detail" && !!form.source;
  return (
    <div className="rt-form">
      {crumb && <BackCrumb back={crumb} />}
      {showBar && (
        <RecordNavBar
          form={listForm}
          cursor={view.cursor ?? null}
          disabled={dirty}
          onGo={(id, cursor) => replace({ formId: form.id, mode: "detail", recordId: id, cursor })}
          onNew={canCreate ? () => push({ formId: form.id, mode: "create" }) : undefined}
        />
      )}
      <RecordView
        form={form}
        mode={view.mode}
        recordId={view.recordId}
        link={link}
        embedded={embedded}
        onMode={(next, id) =>
          replace({ formId: form.id, mode: next, recordId: id, cursor: view.cursor })
        }
        onClose={close}
        onNavigate={navigate}
        onNotify={onNotify}
        onDirty={(next) => {
          setDirty(next);
          onDirty?.(next);
        }}
        onSaved={onSaved}
      />
    </div>
  );
}
