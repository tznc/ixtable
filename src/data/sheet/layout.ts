import type { DocumentConfig, DbColumn } from "../../lib/types";
import type { TotalFunction } from "./types";

/** A table's datasheet view settings, kept in the document like the relationship layout. */
export interface DatasheetLayout {
  /** Column names hidden from the datasheet. */
  hidden: string[];
  /** Number of leftmost visible columns frozen while scrolling sideways. */
  frozen: number;
  /** The totals row, when shown: the aggregate per column (absent = none). */
  totals: Record<string, TotalFunction> | null;
}

export const EMPTY_LAYOUT: DatasheetLayout = { hidden: [], frozen: 0, totals: null };

type Layouts = Record<string, Partial<DatasheetLayout>>;

const layoutsOf = (config: Pick<DocumentConfig, "navigationState">): Layouts =>
  ((config.navigationState as Record<string, unknown> | null)?.datasheetLayouts ?? {}) as Layouts;

export function readLayout(
  config: Pick<DocumentConfig, "navigationState">,
  table: string,
): DatasheetLayout {
  const saved = layoutsOf(config)[table] ?? {};
  return {
    hidden: Array.isArray(saved.hidden) ? saved.hidden : [],
    frozen: typeof saved.frozen === "number" && saved.frozen > 0 ? saved.frozen : 0,
    totals: saved.totals && typeof saved.totals === "object" ? saved.totals : null,
  };
}

/** Returns `config` with `table`'s layout replaced (an empty layout removes the entry). */
export function writeLayout<C extends Pick<DocumentConfig, "navigationState">>(
  config: C,
  table: string,
  layout: DatasheetLayout,
): C {
  const navigation = (config.navigationState ?? {}) as Record<string, unknown>;
  const layouts = { ...layoutsOf(config) };
  const empty = !layout.hidden.length && !layout.frozen && !layout.totals;
  if (empty) delete layouts[table];
  else layouts[table] = layout;
  return { ...config, navigationState: { ...navigation, datasheetLayouts: layouts } };
}

/** Indexes (into `columns`) of the columns the datasheet shows, in table order. */
export const visibleColumns = (columns: DbColumn[], layout: DatasheetLayout): number[] =>
  columns.flatMap((c, i) => (layout.hidden.includes(c.name) ? [] : [i]));

/** Hides `column`; a frozen column that is hidden leaves the frozen count in range. */
export function hideColumn(
  layout: DatasheetLayout,
  columns: DbColumn[],
  column: string,
): DatasheetLayout {
  const shown = visibleColumns(columns, layout).map((i) => columns[i].name);
  const at = shown.indexOf(column);
  if (at < 0 || shown.length <= 1) return layout;
  return {
    ...layout,
    hidden: [...layout.hidden, column],
    frozen: at < layout.frozen ? layout.frozen - 1 : layout.frozen,
  };
}

/** Freezes the visible columns up to and including `column`. */
export function freezeThrough(
  layout: DatasheetLayout,
  columns: DbColumn[],
  column: string,
): DatasheetLayout {
  const shown = visibleColumns(columns, layout).map((i) => columns[i].name);
  const at = shown.indexOf(column);
  return at < 0 ? layout : { ...layout, frozen: at + 1 };
}
