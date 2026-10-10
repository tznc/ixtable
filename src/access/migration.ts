import { FORMAT_LABELS, KIND_ORDER, kindLabel } from "./summary";
import type { AccessFormat, AccessMigration, ImportItem, ItemStatus } from "./types";

/**
 * The migration report kept in `settings.accessImport` of an imported
 * document (docs/decisions/access-import.md): reading it, filtering it,
 * marking items reviewed, and rendering it as Markdown or CSV.
 */

const STATUS_LABELS: Record<ItemStatus, string> = {
  converted: "Converted",
  partial: "Partly converted",
  skipped: "Not converted",
};

export const statusLabel = (status: ItemStatus) => STATUS_LABELS[status];

const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

const strings = (v: unknown) =>
  Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];

/** The migration report of a document, or null when it was not imported from Access. */
export function readMigration(settings: unknown): AccessMigration | null {
  const access = isRecord(settings) ? settings.accessImport : null;
  if (!isRecord(access) || !Array.isArray(access.report)) return null;
  const report = access.report.filter(
    (i): i is ImportItem => isRecord(i) && typeof i.kind === "string" && typeof i.name === "string",
  );
  return {
    source: typeof access.source === "string" ? access.source : "",
    format: (typeof access.format === "string" ? access.format : "template") as AccessFormat,
    report: report.map((i) => ({ ...i, notes: strings(i.notes) })),
    warnings: strings(access.warnings),
    importedAt: typeof access.importedAt === "string" ? access.importedAt : null,
    reviewed: strings(access.reviewed),
  };
}

export const itemKey = (item: Pick<ImportItem, "kind" | "name">) => `${item.kind}:${item.name}`;

/** Settings with `key` marked reviewed (or not); other settings are kept. */
export function withReviewed(settings: unknown, key: string, reviewed: boolean): unknown {
  const base = isRecord(settings) ? settings : {};
  const access = isRecord(base.accessImport) ? base.accessImport : {};
  const current = new Set(strings(access.reviewed));
  if (reviewed) current.add(key);
  else current.delete(key);
  return { ...base, accessImport: { ...access, reviewed: [...current].sort() } };
}

export type StatusFilter = "attention" | "all" | ItemStatus;

export interface MigrationFilter {
  status: StatusFilter;
  kind: string;
  search: string;
  hideReviewed: boolean;
}

const rank = (s: ItemStatus) => (s === "skipped" ? 0 : s === "partial" ? 1 : 2);
const kindRank = (kind: string) => {
  const i = KIND_ORDER.indexOf(kind);
  return i < 0 ? KIND_ORDER.length : i;
};

/** Items matching the filter, worst first, then by kind and name. */
export function filterItems(m: AccessMigration, f: MigrationFilter): ImportItem[] {
  const search = f.search.trim().toLowerCase();
  const reviewed = new Set(m.reviewed);
  return m.report
    .filter((i) =>
      f.status === "all"
        ? true
        : f.status === "attention"
          ? i.status !== "converted"
          : i.status === f.status,
    )
    .filter((i) => !f.kind || i.kind === f.kind)
    .filter((i) => !f.hideReviewed || !reviewed.has(itemKey(i)))
    .filter(
      (i) =>
        !search ||
        i.name.toLowerCase().includes(search) ||
        i.notes.some((n) => n.toLowerCase().includes(search)),
    )
    .sort(
      (a, b) =>
        rank(a.status) - rank(b.status) ||
        kindRank(a.kind) - kindRank(b.kind) ||
        a.name.localeCompare(b.name),
    );
}

/** Items still needing attention that nobody marked reviewed. */
export function openCount(m: AccessMigration): number {
  const reviewed = new Set(m.reviewed);
  return m.report.filter((i) => i.status !== "converted" && !reviewed.has(itemKey(i))).length;
}

const mdCell = (s: string) => s.replace(/\|/g, "\\|").replace(/\r?\n/g, " ");

/** The report as Markdown: a summary table, then every item that lost something. */
export function toMarkdown(m: AccessMigration, title: string): string {
  const reviewed = new Set(m.reviewed);
  const lines = [`# Access migration report: ${title}`, ""];
  lines.push(`Source: ${m.source || "unknown"} (${FORMAT_LABELS[m.format] ?? m.format})`);
  if (m.importedAt) lines.push(`Imported: ${m.importedAt}`);
  lines.push("", "| Objects | Converted | Partly converted | Not converted |", "|---|---|---|---|");
  const kinds = [...new Set(m.report.map((i) => i.kind))].sort((a, b) => kindRank(a) - kindRank(b));
  for (const kind of kinds) {
    const items = m.report.filter((i) => i.kind === kind);
    const n = (s: ItemStatus) => items.filter((i) => i.status === s).length;
    lines.push(`| ${kindLabel(kind)} | ${n("converted")} | ${n("partial")} | ${n("skipped")} |`);
  }
  const attention = filterItems(m, {
    status: "attention",
    kind: "",
    search: "",
    hideReviewed: false,
  });
  if (attention.length) {
    lines.push("", "## What did not convert fully", "");
    for (const item of attention) {
      const mark = reviewed.has(itemKey(item)) ? " (reviewed)" : "";
      lines.push(`### ${mdCell(item.name)} (${item.kind}): ${statusLabel(item.status)}${mark}`, "");
      for (const note of item.notes) lines.push(`- ${mdCell(note)}`);
      lines.push("");
    }
  }
  if (m.warnings.length) {
    lines.push("", "## Reading problems", "");
    for (const w of m.warnings) lines.push(`- ${mdCell(w)}`);
  }
  return `${lines.join("\n").trimEnd()}\n`;
}

const csvCell = (s: string) => (/[",\r\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s);

/** One row per object and note: kind, name, status, reviewed, note. */
export function toCsv(m: AccessMigration): string {
  const reviewed = new Set(m.reviewed);
  const rows = [["Kind", "Name", "Status", "Reviewed", "Note"]];
  const all = filterItems(m, { status: "all", kind: "", search: "", hideReviewed: false });
  for (const item of all) {
    const base = [
      item.kind,
      item.name,
      statusLabel(item.status),
      reviewed.has(itemKey(item)) ? "yes" : "no",
    ];
    if (item.notes.length === 0) rows.push([...base, ""]);
    for (const note of item.notes) rows.push([...base, note]);
  }
  return `${rows.map((r) => r.map(csvCell).join(",")).join("\r\n")}\r\n`;
}
