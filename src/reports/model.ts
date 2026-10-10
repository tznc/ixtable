import { newId } from "../lib/utils";
import { MAX_SUBREPORT_DEPTH } from "./engine/subreport";
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
  subreport: [300, 60],
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
    case "subreport":
      return { ...base, kind, reportId: "", links: [] };
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

/** Deep copy with fresh ids for the report, its groups, components and table columns. */
export function duplicateReport(report: Report, name: string): Report {
  const copyBand = (band: Band): Band => ({
    ...band,
    components: band.components.map((c) =>
      c.kind === "table"
        ? { ...c, id: newId(), columns: c.columns.map((col) => ({ ...col, id: newId() })) }
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
  "parent",
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
  subreport: "Subreport",
};

export const isPageBand = (key: BandKey) => key === "pageHeader" || key === "pageFooter";

/** Ids of the reports a report's subreports print. */
export const subreportIds = (report: Report): string[] =>
  bandEntries(report).flatMap((e) =>
    e.band.components.flatMap((c) => (c.kind === "subreport" && c.reportId ? [c.reportId] : [])),
  );

/** Levels of subreports below report `id` (0 without any); a loop is infinitely deep. */
export function subreportDepth(reports: Report[], id: string, path = new Set<string>()): number {
  if (path.has(id)) return Number.POSITIVE_INFINITY;
  const report = reports.find((r) => r.id === id);
  const ids = report ? subreportIds(report) : [];
  if (!ids.length) return 0;
  const inner = new Set(path).add(id);
  return 1 + Math.max(...ids.map((child) => subreportDepth(reports, child, inner)));
}

/** Levels of subreports above report `id`: the longest chain of reports that print it. */
function subreportHeight(reports: Report[], id: string, path = new Set<string>()): number {
  if (path.has(id)) return Number.POSITIVE_INFINITY;
  const inner = new Set(path).add(id);
  const parents = reports.filter((r) => subreportIds(r).includes(id));
  return Math.max(0, ...parents.map((p) => 1 + subreportHeight(reports, p.id, inner)));
}

/**
 * Reports a subreport in `report` may print: never `report` itself or one that
 * prints it, and only while the whole chain stays within `MAX_SUBREPORT_DEPTH`.
 */
export function embeddableReports(reports: Report[], report: Report): Report[] {
  const above = subreportHeight(reports, report.id);
  return reports.filter(
    (r) =>
      r.id !== report.id &&
      above + 1 + subreportDepth(reports, r.id, new Set([report.id])) <= MAX_SUBREPORT_DEPTH,
  );
}

/** Definition problems the designer shows; mirrors `reports::validate` in Rust. */
export function reportProblems(report: Report, reports: Report[] = []): string[] {
  const subreportOf = (c: ReportComponent) =>
    c.kind === "subreport" ? reports.find((r) => r.id === c.reportId) : undefined;
  const depth = subreportDepth(reports, report.id);
  const nesting =
    depth === Number.POSITIVE_INFINITY
      ? ["its subreports print each other in a loop"]
      : depth > MAX_SUBREPORT_DEPTH
        ? [`its subreports nest ${depth} levels deep (at most ${MAX_SUBREPORT_DEPTH})`]
        : [];
  return [
    ...nesting,
    ...bandEntries(report).flatMap((e) => {
      const page = isPageBand(e.key);
      const subs = e.band.components.filter((c) => c.kind === "subreport");
      return [
        ...e.band.components.flatMap((c) =>
          page && (c.kind === "table" || c.kind === "subreport")
            ? [
                `${e.label} component ${c.id}: ${c.kind === "table" ? "tables" : "subreports"} are not supported in page headers or footers`,
              ]
            : [],
        ),
        ...(subs.length > 1 ? [`${e.label} has more than one subreport`] : []),
        ...(subs.length && e.band.components.some((c) => c.kind === "table")
          ? [`${e.label} has both a table and a subreport`]
          : []),
        ...subs.flatMap((c) =>
          c.kind === "subreport" && c.reportId && reports.length && !subreportOf(c)
            ? [`${e.label} component ${c.id}: the subreport's report does not exist`]
            : [],
        ),
      ];
    }),
  ];
}
