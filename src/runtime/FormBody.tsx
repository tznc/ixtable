import { type KeyboardEvent, useState } from "react";
import { controlConstraints } from "../design/constraints";
import type { DesignControl, DesignForm, TabPage } from "../design/schema";
import { isInputKind } from "../design/schema";
import { GridCanvas, GridItem } from "../grid";
import { ComputedValue, Field, ImageView } from "./controls";
import { toneFor } from "./conditions";
import { compute, type FormScope } from "./formState";
import { RelatedRecords } from "./RelatedList";
import type { DataValue } from "../lib/types";

/** What the control tree needs from the record view. */
export type BodyContext = {
  form: DesignForm;
  scope: FormScope;
  visible: Set<string>;
  /** Controls whose own and every ancestor container's `enabledWhen` holds. */
  enabled: Set<string>;
  errors: Record<string, string>;
  /** True in detail mode or when the form is read-only. */
  readOnly: boolean;
  /** Columns that may not be edited (the related-list link, auto keys). */
  locked: Set<string>;
  identity: DataValue[] | null;
  setField: (column: string, value: unknown) => void;
  blur: (control: DesignControl) => void;
  runButton: (control: DesignControl) => void;
  canRunButton: (control: DesignControl) => boolean;
};

const childrenOf = (form: DesignForm, parentId: string | null, tab?: string | null) =>
  form.controls.filter(
    (control) =>
      (control.parent?.id ?? null) === parentId &&
      (tab === undefined || (control.parent?.tab ?? null) === tab),
  );

/** Renders the controls of one container (the form itself when `parent` is null) on the grid. */
export function ControlGrid({
  ctx,
  parent,
  tab,
}: {
  ctx: BodyContext;
  parent: DesignControl | null;
  tab?: string | null;
}) {
  const layout = parent?.layout ?? ctx.form.layout;
  const items = childrenOf(ctx.form, parent?.id ?? null, tab).filter((control) =>
    ctx.visible.has(control.id),
  );
  return (
    <GridCanvas layout={layout}>
      {items.map((control) => (
        <GridItem
          key={control.id}
          id={control.id}
          placement={control.placement}
          label={control.label}
          constraints={controlConstraints(control.kind, layout.columns.length)}
        >
          <ControlView ctx={ctx} control={control} />
        </GridItem>
      ))}
    </GridCanvas>
  );
}

function ControlView({ ctx, control }: { ctx: BodyContext; control: DesignControl }) {
  const { scope } = ctx;
  const column = control.binding?.column;
  switch (control.kind) {
    case "label":
      return <p className="rt-static">{control.text || control.label}</p>;
    case "section":
      return (
        <fieldset className="rt-section">
          <legend>{control.label}</legend>
          <ControlGrid ctx={ctx} parent={control} />
        </fieldset>
      );
    case "tabs":
      return <TabGroup ctx={ctx} control={control} />;
    case "computed": {
      const result = compute(control.computed, scope);
      return (
        <ComputedValue
          control={control}
          value={result.value}
          error={result.error}
          tone={toneFor(control.styles, { ...scope, value: result.value })}
        />
      );
    }
    case "button":
      return (
        <button
          type="button"
          className="rt-button"
          disabled={!ctx.enabled.has(control.id) || !ctx.canRunButton(control)}
          onClick={() => ctx.runButton(control)}
        >
          {control.label}
        </button>
      );
    case "image":
      return <ImageView control={control} />;
    case "relatedList":
      return <RelatedRecords ctx={ctx} control={control} disabled={!ctx.enabled.has(control.id)} />;
    default:
      break;
  }
  if (!isInputKind(control.kind)) return null;
  const derived = control.computed ? compute(control.computed, scope) : null;
  const value = derived ? derived.value : column ? scope.record[column] : null;
  const readOnly =
    ctx.readOnly ||
    !!derived ||
    !!control.readOnly ||
    (column ? ctx.locked.has(column) : true) ||
    !ctx.enabled.has(control.id);
  const tone = toneFor(control.styles, { ...scope, value });
  if (ctx.readOnly && control.format && value != null && control.kind !== "relationship") {
    return <ComputedValue control={control} value={value} tone={tone} />;
  }
  return (
    <Field
      control={control}
      value={value}
      readOnly={readOnly}
      error={ctx.errors[control.id]}
      tone={tone}
      filterScope={
        control.relationship?.filter
          ? { parent: scope.record, form: scope.form, app: scope.app, params: {} }
          : undefined
      }
      onChange={(next) => column && ctx.setField(column, next)}
      onBlur={() => ctx.blur(control)}
      keyValues={scope.record}
      onKeys={(values) => {
        for (const [key, next] of Object.entries(values)) ctx.setField(key, next);
      }}
    />
  );
}

function TabGroup({ ctx, control }: { ctx: BodyContext; control: DesignControl }) {
  const tabs: TabPage[] = control.tabs ?? [];
  const [active, setActive] = useState(tabs[0]?.id ?? "");
  const current = tabs.find((tab) => tab.id === active) ?? tabs[0];
  const errorTabs = new Set(
    ctx.form.controls
      .filter((c) => c.parent?.id === control.id && ctx.errors[c.id])
      .map((c) => c.parent?.tab),
  );
  const onKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const index = tabs.findIndex((tab) => tab.id === current?.id);
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (!step || index < 0) return;
    event.preventDefault();
    const next = tabs[(index + step + tabs.length) % tabs.length];
    setActive(next.id);
    const button = event.currentTarget.querySelector<HTMLButtonElement>(`[data-tab="${next.id}"]`);
    button?.focus();
  };
  return (
    <div className="rt-tabs">
      <div role="tablist" aria-label={control.label} onKeyDown={onKey}>
        {tabs.map((tab) => (
          <button
            key={tab.id}
            type="button"
            role="tab"
            data-tab={tab.id}
            id={`${control.id}-${tab.id}`}
            aria-selected={tab.id === current?.id}
            tabIndex={tab.id === current?.id ? 0 : -1}
            onClick={() => setActive(tab.id)}
          >
            {tab.label}
            {errorTabs.has(tab.id) && <span className="rt-error-dot"> (has errors)</span>}
          </button>
        ))}
      </div>
      {current && (
        <div role="tabpanel" aria-labelledby={`${control.id}-${current.id}`}>
          <ControlGrid ctx={ctx} parent={control} tab={current.id} />
        </div>
      )}
    </div>
  );
}
