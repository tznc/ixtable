import type {
  AccessFormat,
  AccessImportReport,
  AccessInventory,
  ImportItem,
  ItemStatus,
} from "./types";

export const FORMAT_LABELS: Record<AccessFormat, string> = {
  template: "Access template (.accdt)",
  jet3: "Access 97 database (.mdb)",
  jet4: "Access 2000–2003 database (.mdb)",
  ace: "Access 2007 or later database (.accdb)",
};

/** Kinds in the order the report lists them. */
export const KIND_ORDER = ["table", "relationship", "query", "form", "report", "macro", "module"];

const PLURALS: Record<string, string> = {
  table: "Tables",
  relationship: "Relationships",
  query: "Queries",
  form: "Forms",
  report: "Reports",
  macro: "Macros",
  module: "Modules",
};

export const kindLabel = (kind: string) => PLURALS[kind] ?? kind;

export function totalRows(inventory: AccessInventory): number {
  return inventory.tables.reduce((sum, t) => sum + (t.rows ?? 0), 0);
}

/** Counts per status for each kind, in report order. */
export function statusCounts(report: AccessImportReport) {
  const kinds = [...new Set(report.items.map((i) => i.kind))].sort((a, b) => order(a) - order(b));
  return kinds.map((kind) => {
    const items = report.items.filter((i) => i.kind === kind);
    const count = (status: ItemStatus) => items.filter((i) => i.status === status).length;
    return {
      kind,
      converted: count("converted"),
      partial: count("partial"),
      skipped: count("skipped"),
    };
  });
}

const order = (kind: string) => {
  const i = KIND_ORDER.indexOf(kind);
  return i < 0 ? KIND_ORDER.length : i;
};

/** Items that lost something, worst first, then by kind and name. */
export function itemsNeedingAttention(report: AccessImportReport): ImportItem[] {
  const rank = (s: ItemStatus) => (s === "skipped" ? 0 : s === "partial" ? 1 : 2);
  return report.items
    .filter((i) => i.status !== "converted")
    .sort(
      (a, b) =>
        rank(a.status) - rank(b.status) ||
        order(a.kind) - order(b.kind) ||
        a.name.localeCompare(b.name),
    );
}
