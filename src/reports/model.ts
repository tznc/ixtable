import { newId } from "../lib/utils";
import type { Band, ComponentKind, PageSetup, Report, ReportComponent, ReportGroup } from "./types";

/** Page width and height in points. A4 is rounded to whole points (595 × 842). */
export function pageDimensions(page: PageSetup): { width: number; height: number } {
  const [w, h] = page.size === "Letter" ? [612, 792] : [595, 842];
  return page.orientation === "landscape" ? { width: h, height: w } : { width: w, height: h };
}

export const contentWidth = (page: PageSetup) =>
  pageDimensions(page).width - page.margins.left - page.margins.right;

export const emptyBand = (height = 0): Band => ({ height, keepTogether: false, components: [] });

export function newReport(name: string): Report {
  return {
    id: newId(),
    name,
    params: {},
    page: {
      size: "A4",
      orientation: "portrait",
      margins: { top: 36, right: 36, bottom: 36, left: 36 },
    },
    bands: {
      reportHeader: emptyBand(40),
      pageHeader: emptyBand(0),
      groups: [],
      detail: emptyBand(18),
      pageFooter: emptyBand(20),
      reportFooter: emptyBand(30),
    },
  };
}

export function newGroup(groupBy: string): ReportGroup {
  return { id: newId(), groupBy, header: emptyBand(22), footer: emptyBand(20) };
}

const KIND_SIZE: Record<ComponentKind, [number, number]> = {
  staticText: [120, 16],
  field: [120, 16],
  calculated: [120, 16],
  image: [80, 60],
  line: [200, 4],
  rectangle: [120, 40],
  table: [300, 40],
  chart: [280, 160],
};

/** A new component of `kind`, placed at (x, y) and clamped into a band of `bandWidth`. */
export function newComponent(
  kind: ComponentKind,
  bandWidth: number,
  at: { x: number; y: number } = { x: 0, y: 0 },
): ReportComponent {
  const [w0, h0] = KIND_SIZE[kind];
  const w = Math.min(w0, bandWidth);
  const base = { id: newId(), x: Math.max(0, Math.min(at.x, bandWidth - w)), y: at.y, w, h: h0 };
  switch (kind) {
    case "staticText":
      return { ...base, kind, text: "Text" };
    case "field":
      return { ...base, kind, expression: "" };
    case "calculated":
      return { ...base, kind, expression: "count(rows)" };
    case "image":
      return { ...base, kind, assetId: "" };
    case "line":
      return { ...base, kind, orientation: "horizontal", style: { borderWidth: 1 } };
    case "rectangle":
      return { ...base, kind, style: { borderWidth: 1 } };
    case "table":
      return { ...base, kind, queryId: "", columns: [] };
    case "chart":
      return { ...base, kind, chartType: "bar", queryId: "", xField: "", yFields: [] };
  }
}

/** Identifies one band of a report: fixed bands by name, group bands by group id. */
export type BandKey =
  | "reportHeader"
  | "pageHeader"
  | "detail"
  | "pageFooter"
  | "reportFooter"
  | `groupHeader:${string}`
  | `groupFooter:${string}`;

export interface BandEntry {
  key: BandKey;
  label: string;
  band: Band;
}

/** Bands in print order with display labels. */
export function bandEntries(report: Report): BandEntry[] {
  const b = report.bands;
  const groups = b.groups.map((g, i) => ({ g, n: groupLabel(g, i) }));
  return [
    { key: "reportHeader", label: "Report header", band: b.reportHeader },
    { key: "pageHeader", label: "Page header", band: b.pageHeader },
    ...groups.map(({ g, n }) => ({
      key: `groupHeader:${g.id}` as BandKey,
      label: `${n} header`,
      band: g.header,
    })),
    { key: "detail", label: "Detail", band: b.detail },
    ...[...groups].reverse().map(({ g, n }) => ({
      key: `groupFooter:${g.id}` as BandKey,
      label: `${n} footer`,
      band: g.footer,
    })),
    { key: "pageFooter", label: "Page footer", band: b.pageFooter },
    { key: "reportFooter", label: "Report footer", band: b.reportFooter },
  ];
}

export const groupLabel = (group: ReportGroup, index: number) =>
  `Group ${index + 1} (${group.groupBy || "unset"})`;

export function getBand(report: Report, key: BandKey): Band | undefined {
  return bandEntries(report).find((entry) => entry.key === key)?.band;
}

/** Returns a copy of `report` with band `key` replaced by `change(band)`. */
export function withBand(report: Report, key: BandKey, change: (band: Band) => Band): Report {
  const bands = { ...report.bands };
  if (key.startsWith("groupHeader:") || key.startsWith("groupFooter:")) {
    const [kind, id] = key.split(":");
    bands.groups = bands.groups.map((g) =>
      g.id !== id
        ? g
        : kind === "groupHeader"
          ? { ...g, header: change(g.header) }
          : { ...g, footer: change(g.footer) },
    );
  } else {
    const fixed = key as Exclude<BandKey, `group${string}`>;
    bands[fixed] = change(bands[fixed]);
  }
  return { ...report, bands };
}

/** Finds a component and the band holding it. */
export function findComponent(
  report: Report,
  id: string,
): { key: BandKey; band: Band; component: ReportComponent } | undefined {
  for (const entry of bandEntries(report)) {
    const component = entry.band.components.find((c) => c.id === id);
    if (component) return { key: entry.key, band: entry.band, component };
  }
  return undefined;
}

/** Deep copy with fresh ids for the report, its groups, components, table columns and conditions. */
export function duplicateReport(report: Report, name: string): Report {
  const copyBand = (band: Band): Band => ({
    ...band,
    components: band.components.map((c) =>
      c.kind === "table"
        ? { ...c, id: newId(), columns: c.columns.map((col) => ({ ...col, id: newId() })) }
        : "conditions" in c && c.conditions
          ? { ...c, id: newId(), conditions: c.conditions.map((r) => ({ ...r, id: newId() })) }
          : { ...c, id: newId() },
    ),
  });
  const b = structuredClone(report.bands);
  return {
    ...structuredClone(report),
    id: newId(),
    name,
    bands: {
      reportHeader: copyBand(b.reportHeader),
      pageHeader: copyBand(b.pageHeader),
      groups: b.groups.map((g) => ({
        ...g,
        id: newId(),
        header: copyBand(g.header),
        footer: copyBand(g.footer),
      })),
      detail: copyBand(b.detail),
      pageFooter: copyBand(b.pageFooter),
      reportFooter: copyBand(b.reportFooter),
    },
  };
}

/** Names visible to report expressions (for `check()` diagnostics). */
export const REPORT_SCOPE_NAMES = [
  "record",
  "rows",
  "params",
  "page",
  "pages",
  "groupPage",
  "groupPages",
  "report",
  "group",
  "rowNumber",
];

/** `record.<column>` with bracket quoting for names that are not plain identifiers. */
export const fieldExpression = (column: string) =>
  /^[A-Za-z_][A-Za-z0-9_]*$/.test(column) ? `record.${column}` : `record.[${column}]`;

/** Display names of component kinds. */
export const KIND_LABELS: Record<ComponentKind, string> = {
  staticText: "Text",
  field: "Field",
  calculated: "Calculated",
  image: "Image",
  line: "Line",
  rectangle: "Rectangle",
  table: "Table",
  chart: "Chart",
};

export const isPageBand = (key: BandKey) => key === "pageHeader" || key === "pageFooter";

/** Definition problems the designer shows; mirrors `reports::validate` in Rust. */
export function reportProblems(report: Report): string[] {
  return bandEntries(report)
    .filter((e) => isPageBand(e.key))
    .flatMap((e) =>
      e.band.components
        .filter((c) => c.kind === "table")
        .map(
          (c) =>
            `${e.label} component ${c.id}: tables are not supported in page headers or footers`,
        ),
    );
}
