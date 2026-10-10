/**
 * Report definitions (PRD §15). Mirrors `reports::Report` in src-tauri/src/reports.rs.
 * All positions and sizes are in PDF points (1/72 inch). Components sit at
 * absolute positions inside their band; the band is as wide as the page
 * content area (page width minus left and right margins).
 */

export type PageSize = "A4" | "Letter";
export type Orientation = "portrait" | "landscape";
export type TextAlign = "left" | "center" | "right";

export interface Margins {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

export interface PageSetup {
  size: PageSize;
  orientation: Orientation;
  margins: Margins;
}

/**
 * Presentation settings. Colors are gray levels only (0 = black, 1 = white) so
 * every report stays readable when printed without color (PRD §27.4).
 */
export interface ComponentStyle {
  /** Points. Default 10. */
  fontSize?: number;
  bold?: boolean;
  align?: TextAlign;
  /** Border (text, image, rectangle) or stroke (line) width in points. 0 = none. */
  borderWidth?: number;
  /** Background gray level; absent or null = transparent. */
  fill?: number | null;
  /** Text and stroke gray level. Default 0 (black). */
  gray?: number;
}

interface ComponentBase {
  id: string;
  x: number;
  y: number;
  w: number;
  h: number;
  style?: ComponentStyle;
}

/**
 * Text components can grow: when the text needs more lines than fit, the box
 * gets taller, components below it move down, and the band grows (PRD §15).
 */
interface TextBase extends ComponentBase {
  canGrow?: boolean;
  /** Conditional formatting rules; the first rule whose `when` holds applies its style. */
  conditions?: ReportCondition[];
}

/** Style a conditional formatting rule applies. Gray levels like `ComponentStyle`. */
export type ConditionStyle = Pick<ComponentStyle, "bold" | "gray" | "fill">;

/**
 * One conditional formatting rule. `when` sees the band scope plus `value`,
 * the component's own value (its text for static text).
 */
export interface ReportCondition {
  id: string;
  when: string;
  style: ConditionStyle;
}

/**
 * Running sum: the value accumulates over the rows (detail band) or band
 * instances (group bands) printed so far. "group" restarts at each instance
 * of the enclosing group; "all" never restarts.
 */
export type RunningSum = "group" | "all";

export interface StaticTextComponent extends TextBase {
  kind: "staticText";
  text: string;
}
/** A bound field: an expression over the current row, usually `record.<column>`. */
export interface FieldComponent extends TextBase {
  kind: "field";
  expression: string;
  format?: string;
  runningSum?: RunningSum;
}
/** A calculated value: totals (`sum(rows.amount)`), page numbers (`page & ' of ' & pages`), … */
export interface CalculatedComponent extends TextBase {
  kind: "calculated";
  expression: string;
  format?: string;
  runningSum?: RunningSum;
}
export interface ImageComponent extends ComponentBase {
  kind: "image";
  /** Application asset (attachment) id. */
  assetId: string;
}
export interface LineComponent extends ComponentBase {
  kind: "line";
  orientation: "horizontal" | "vertical";
}
export interface RectangleComponent extends ComponentBase {
  kind: "rectangle";
}
export interface TableColumn {
  id: string;
  header: string;
  /** Evaluated per table row with that row as `record`. */
  expression: string;
  format?: string;
  /** Points. */
  width: number;
  align?: TextAlign;
}
/**
 * A query-backed table. Rows come from `queryId` (a saved query) or, when it is
 * empty, from the band's `rows` (the whole report or the current group). The
 * table grows past its designed height and splits across pages, repeating the
 * header row on every page.
 */
export interface TableComponent extends ComponentBase {
  kind: "table";
  queryId?: string;
  columns: TableColumn[];
}

export type ReportChartType = "bar" | "line" | "area" | "pie" | "donut" | "scatter";
export const REPORT_CHART_TYPES: ReportChartType[] = [
  "bar",
  "line",
  "area",
  "pie",
  "donut",
  "scatter",
];

/**
 * A chart drawn with the dashboard chart data and geometry (PRD §15, §16).
 * Rows come from `queryId` (a saved query) or, when it is empty, from the
 * band's `rows`. Charts keep their designed size and never split.
 */
export interface ChartComponent extends ComponentBase {
  kind: "chart";
  chartType: ReportChartType;
  queryId?: string;
  /** Category column (numeric x column for scatter). */
  xField: string;
  /** Value columns, one series each. */
  yFields: string[];
  /** Splits the first y column into one series per value. */
  groupBy?: string;
  stacked?: boolean;
  /** src/expr format pattern for value labels. */
  format?: string;
  title?: string;
}

export type ReportComponent =
  | StaticTextComponent
  | FieldComponent
  | CalculatedComponent
  | ImageComponent
  | LineComponent
  | RectangleComponent
  | TableComponent
  | ChartComponent;
export type ComponentKind = ReportComponent["kind"];

export interface Band {
  height: number;
  /**
   * Never split this band across pages; on a group header, also keep the
   * header on the same page as the first row that follows it.
   */
  keepTogether: boolean;
  /** Start a new page before this band (skipped when the page is still empty). */
  pageBreakBefore?: boolean;
  /** Start a new page after this band (no blank page at the end of the report). */
  pageBreakAfter?: boolean;
  components: ReportComponent[];
}

export interface ReportGroup {
  id: string;
  /** Expression evaluated per row, e.g. `record.region`. */
  groupBy: string;
  descending?: boolean;
  header: Band;
  footer: Band;
  /** Start every group instance on a new page. */
  newPage?: boolean;
  /** Print the group header again at the top of each page the group continues on. */
  repeatHeader?: boolean;
  /**
   * Start every group instance on a new page and restart `groupPage` and
   * `groupPages` there. `page` and `pages` always count the whole report.
   */
  resetPageNumber?: boolean;
}

export interface Bands {
  reportHeader: Band;
  pageHeader: Band;
  /** Outer group first. */
  groups: ReportGroup[];
  detail: Band;
  pageFooter: Band;
  reportFooter: Band;
}

export interface Report {
  id: string;
  name: string;
  /** Saved query that supplies the rows. Takes precedence over `table`. */
  datasetQueryId?: string | null;
  /** Table or view read in full when no saved query is set. */
  table?: string | null;
  /** Default parameter values; passed to the dataset query and exposed as `params`. */
  params: Record<string, unknown>;
  page: PageSetup;
  bands: Bands;
}
