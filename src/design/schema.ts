import type { CSSProperties } from "react";
import {
  defaultGridLayout,
  layoutToCss,
  nextPlacement as gridNextPlacement,
  normalizeLayout,
  normalizePlacement,
  placementToCss,
} from "../grid/engine";
import type { GridLayout, Placement } from "../grid/types";
import { newId } from "../lib/utils";
import type { ConditionalStyle } from "../runtime/conditions";

/** TypeScript mirror of `src-tauri/src/design.rs` (serde camelCase). */
export const DESIGN_SCHEMA_VERSION = 3;

export type ControlKind =
  | "label"
  | "text"
  | "multiline"
  | "number"
  | "decimal"
  | "boolean"
  | "date"
  | "time"
  | "datetime"
  | "select"
  | "relationship"
  | "computed"
  | "button"
  | "section"
  | "tabs"
  | "relatedList"
  | "image";

export type FormMode = "list" | "detail" | "create" | "edit";
export const FORM_MODES: FormMode[] = ["list", "detail", "create", "edit"];

export type {
  Breakpoint,
  GridAlign,
  GridLayout,
  GridTrack,
  NamedRegion,
  Placement,
  TrackKind,
} from "../grid/types";

export type FormSource = {
  kind: "table" | "query";
  table?: string | null;
  queryId?: string | null;
  /** Query sources: parameter name to an expression over `app` and `params` (page parameters). */
  params?: Record<string, string> | null;
};
export type ControlValidation = {
  required: boolean;
  min?: number | null;
  max?: number | null;
  pattern?: string | null;
  /** Expression over {record, form, app, value}; must be true. */
  expression?: string | null;
  message?: string | null;
};
export type SelectOption = { value: string; label: string };
/** One column of a multi-column key: `column` on this side, `target` on the other table. */
export type KeyPair = { column: string; target: string };
/**
 * Foreign-key lookup. `valueColumn` is the target column stored in the bound column. For a
 * multi-column key, `keys` lists every pair (bound column first) and choosing writes them all.
 */
export type Relationship = {
  table: string;
  valueColumn: string;
  displayColumn: string;
  keys?: KeyPair[];
  /** Row filter over the choices: `record` is a choice row, `parent` the edited record. */
  filter?: string | null;
};
export type TabPage = { id: string; label: string };
/**
 * Child rows of `table` whose `foreignKey` equals the parent's `parentColumn`. For a
 * multi-column key, `keys` lists every pair (`column` on the child, `target` on the parent).
 */
export type RelatedList = {
  table: string;
  foreignKey: string;
  parentColumn: string;
  keys?: KeyPair[];
  columns: string[];
  formId?: string | null;
  /** Row filter: `record` is a child row, `parent` the parent record. */
  filter?: string | null;
};
export type ControlParent = { id: string; tab?: string | null };

export type DesignControl = {
  id: string;
  kind: ControlKind;
  label: string;
  binding?: { column: string; table?: string | null } | null;
  validation: ControlValidation;
  placement: Placement;
  parent?: ControlParent | null;
  visibleWhen?: string | null;
  enabledWhen?: string | null;
  computed?: string | null;
  format?: string | null;
  defaultValue?: string | null;
  text?: string | null;
  options?: SelectOption[];
  optionsQueryId?: string | null;
  relationship?: Relationship | null;
  actionId?: string | null;
  tabs?: TabPage[];
  layout?: GridLayout | null;
  related?: RelatedList | null;
  assetId?: string | null;
  readOnly?: boolean;
  variant?: string | null;
  /** Conditional styles; the first rule whose `when` holds sets the tone. */
  styles?: ConditionalStyle[];
};
export type FormRule = { id: string; expression: string; message: string };
/** Form events (PRD §17.4); each runs a declarative action (src/runtime/formEvents.ts). */
export type FormEventName = "onLoad" | "onCurrent" | "beforeUpdate" | "afterUpdate";
export type FormEvents = Partial<Record<FormEventName, string | null>>;
export const FORM_EVENTS: { name: FormEventName; label: string }[] = [
  { name: "onLoad", label: "On load" },
  { name: "onCurrent", label: "On current" },
  { name: "beforeUpdate", label: "Before update" },
  { name: "afterUpdate", label: "After update" },
];
export type DesignForm = {
  id: string;
  name: string;
  source?: FormSource | null;
  modes: FormMode[];
  controls: DesignControl[];
  layout: GridLayout;
  listColumns: string[];
  pageSize: number;
  detailFormId?: string | null;
  rules: FormRule[];
  /** List mode row filter: an expression over `record`, `app` and `params`. */
  filter?: string | null;
  /** Event → action id. */
  events?: FormEvents;
};
export type NavKind = "form" | "report" | "dashboard" | "table" | "group";
export type NavigationItem = {
  id: string;
  label: string;
  kind: NavKind;
  targetId?: string | null;
  mode?: FormMode | null;
  children?: NavigationItem[];
};
export type DesignSchema = {
  version: number;
  forms: DesignForm[];
  navigation: NavigationItem[];
  startPage?: string | null;
};

export { defaultGridLayout };

export const layoutStyle = (layout: GridLayout, width?: number): CSSProperties =>
  layoutToCss(layout, { width });

export const placementStyle = (
  placement: Placement,
  layout?: GridLayout,
  width?: number,
): CSSProperties => placementToCss(placement, layout ?? null, width);

type Placed = { placement: Placement; parent?: ControlParent | null };
const sameParent = (a?: ControlParent | null, b?: ControlParent | null) =>
  (a?.id ?? null) === (b?.id ?? null) && (a?.tab ?? null) === (b?.tab ?? null);

/** The grid a control is placed on: its container's inner layout, else the form's. */
export const containerLayout = (
  form: { layout: GridLayout; controls: DesignControl[] },
  parent?: ControlParent | null,
): GridLayout =>
  (parent && form.controls.find((control) => control.id === parent.id)?.layout) || form.layout;

/** First free full-width slot in the given container (the form grid by default). */
export const nextPlacement = (
  form: { layout: GridLayout; controls: Placed[] },
  parent?: ControlParent | null,
  size?: { columnSpan?: number; rowSpan?: number },
): Placement =>
  gridNextPlacement(
    parent
      ? containerLayout(form as { layout: GridLayout; controls: DesignControl[] }, parent)
      : form.layout,
    form.controls
      .filter((control) => sameParent(control.parent, parent))
      .map((control) => control.placement),
    size,
  );

const LABELS: Record<ControlKind, string> = {
  label: "Text",
  text: "Text field",
  multiline: "Notes",
  number: "Number",
  decimal: "Amount",
  boolean: "Yes/No",
  date: "Date",
  time: "Time",
  datetime: "Date and time",
  select: "Choice",
  relationship: "Lookup",
  computed: "Computed value",
  button: "Button",
  section: "Section",
  tabs: "Tabs",
  relatedList: "Related records",
  image: "Image",
};
export const controlKindLabel = (kind: ControlKind) => LABELS[kind];
export const CONTROL_KINDS = Object.keys(LABELS) as ControlKind[];

/** Static kinds have nothing to enable or disable, so `enabledWhen` does not apply to them. */
export const hasEnabledState = (kind: ControlKind) => kind !== "label" && kind !== "image";

/** Kinds that hold a record value bound to a column. */
export const isInputKind = (kind: ControlKind) =>
  [
    "text",
    "multiline",
    "number",
    "decimal",
    "boolean",
    "date",
    "time",
    "datetime",
    "select",
    "relationship",
  ].includes(kind);
export const isContainerKind = (kind: ControlKind) => kind === "section" || kind === "tabs";

const containerGrid = (): GridLayout => ({ ...defaultGridLayout(), columnGap: 12, rowGap: 12 });

export const newControl = (
  kind: ControlKind,
  form: DesignForm,
  parent?: ControlParent | null,
): DesignControl => {
  const wide = isContainerKind(kind) || kind === "relatedList";
  const control: DesignControl = {
    id: newId(),
    kind,
    label: kind === "section" ? "New section" : `New ${LABELS[kind].toLowerCase()}`,
    binding: null,
    validation: { required: false },
    placement: nextPlacement(form, parent, { columnSpan: wide ? undefined : 6 }),
    parent: parent ?? null,
  };
  if (kind === "label") control.text = "Text";
  if (kind === "select") control.options = [];
  if (kind === "computed") control.computed = "";
  if (kind === "section") control.layout = containerGrid();
  if (kind === "tabs") {
    control.layout = containerGrid();
    control.tabs = [
      { id: newId(), label: "Tab 1" },
      { id: newId(), label: "Tab 2" },
    ];
  }
  return control;
};

export const newForm = (name: string, source: FormSource | null = null): DesignForm => ({
  id: newId(),
  name,
  source,
  modes: source?.kind === "query" ? ["list", "detail"] : [...FORM_MODES],
  controls: [],
  layout: defaultGridLayout(),
  listColumns: [],
  pageSize: 25,
  detailFormId: null,
  rules: [],
});

const rec = (value: unknown): Record<string, unknown> =>
  value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
const arr = (value: unknown): unknown[] => (Array.isArray(value) ? value : []);
const str = (value: unknown, fallback = "") => (typeof value === "string" ? value : fallback);

function upgradeControl(raw: unknown): DesignControl {
  const control = rec(raw) as Partial<DesignControl> & Record<string, unknown>;
  const rawKind = str(control.kind, "text");
  const kind = (rawKind === "checkbox" ? "boolean" : rawKind) as ControlKind;
  const binding = control.binding ? rec(control.binding) : null;
  return {
    ...(control as DesignControl),
    id: str(control.id),
    kind: CONTROL_KINDS.includes(kind) ? kind : "text",
    label: str(control.label),
    binding: binding ? { ...binding, column: str(binding.column) } : null,
    validation: { required: false, ...rec(control.validation) },
    placement: normalizePlacement(control.placement),
    layout: control.layout ? normalizeLayout(control.layout) : (control.layout ?? null),
  };
}

function upgradeForm(raw: unknown): DesignForm {
  const form = rec(raw);
  const table = str(form.table);
  const source =
    (form.source as FormSource | null | undefined) ?? (table ? { kind: "table", table } : null);
  const modes = arr(form.modes).filter((m): m is FormMode => FORM_MODES.includes(m as FormMode));
  return {
    id: str(form.id),
    name: str(form.name),
    source,
    modes: form.modes === undefined ? [...FORM_MODES] : modes,
    controls: arr(form.controls).map(upgradeControl),
    layout: normalizeLayout(form.layout ?? {}),
    listColumns: arr(form.listColumns).map((c) => str(c)),
    pageSize: typeof form.pageSize === "number" ? form.pageSize : 25,
    detailFormId: (form.detailFormId as string | null | undefined) ?? null,
    rules: arr(form.rules).map((r) => ({ id: "", expression: "", message: "", ...rec(r) })),
    ...(typeof form.filter === "string" ? { filter: form.filter } : {}),
    ...upgradeEvents(form.events),
  };
}

function upgradeEvents(raw: unknown): { events?: FormEvents } {
  const events: FormEvents = {};
  for (const { name } of FORM_EVENTS) {
    const id = rec(raw)[name];
    if (typeof id === "string" && id) events[name] = id;
  }
  return Object.keys(events).length ? { events } : {};
}

function upgradeNav(raw: unknown): NavigationItem {
  const item = rec(raw);
  const legacyForm = typeof item.formId === "string" ? item.formId : null;
  return {
    id: str(item.id),
    label: str(item.label),
    kind: (legacyForm ? "form" : str(item.kind, "form")) as NavKind,
    targetId: legacyForm ?? ((item.targetId as string | null | undefined) || null),
    mode: (item.mode as FormMode | null | undefined) ?? null,
    children: arr(item.children).map(upgradeNav),
  };
}

/**
 * Brings any stored design (v1/v2/v3) to the current TS shape with defaults filled.
 * Mirrors `design/upgrade.rs`; Rust upgrades on load, this guards hand-written values.
 * The legacy `main` id rewrite needs the whole config: see `upgradeLegacyIds` (legacyIds.ts).
 */
export function upgradeDesign(raw: unknown): DesignSchema {
  const design = rec(raw);
  const navigation = arr(design.navigation).map(upgradeNav);
  const version = typeof design.version === "number" ? design.version : DESIGN_SCHEMA_VERSION;
  return {
    version: Math.max(version, DESIGN_SCHEMA_VERSION),
    forms: arr(design.forms).map(upgradeForm),
    navigation,
    startPage:
      "startPage" in design
        ? ((design.startPage as string | null) ?? null)
        : (navigation[0]?.id ?? null),
  };
}

/** Every navigation item, depth first. */
export const flattenNavigation = (items: NavigationItem[]): NavigationItem[] =>
  items.flatMap((item) => [item, ...flattenNavigation(item.children ?? [])]);

export const formTable = (form?: DesignForm | null) =>
  form?.source?.kind === "table" ? (form.source.table ?? null) : null;

/** Key pairs of a related list: `keys` when set, else `foreignKey` → `parentColumn`. */
export const relatedKeys = (related: RelatedList): KeyPair[] =>
  related.keys?.length
    ? related.keys
    : [{ column: related.foreignKey, target: related.parentColumn }];
