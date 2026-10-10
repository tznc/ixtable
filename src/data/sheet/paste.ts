import type { RecordWrite } from "../../lib/records";
import type { DbPage } from "../../lib/types";
import { originalValues, typedValues } from "./rows";

/**
 * Parses spreadsheet clipboard text (Excel, Numbers, Google Sheets: tab-separated
 * rows, CRLF or LF line ends, fields with tabs, newlines or quotes wrapped in
 * double quotes) into rows of cells. A trailing line end adds no empty row.
 */
export function parseClipboardGrid(text: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let cell = "";
  let i = 0;
  let quoted = false;
  while (i < text.length) {
    const ch = text[i];
    if (quoted) {
      if (ch === '"' && text[i + 1] === '"') {
        cell += '"';
        i += 2;
        continue;
      }
      if (ch === '"') quoted = false;
      else cell += ch;
      i++;
      continue;
    }
    if (ch === '"' && cell === "") {
      quoted = true;
    } else if (ch === "\t") {
      row.push(cell);
      cell = "";
    } else if (ch === "\n" || ch === "\r") {
      row.push(cell);
      rows.push(row);
      row = [];
      cell = "";
      if (ch === "\r" && text[i + 1] === "\n") i++;
    } else {
      cell += ch;
    }
    i++;
  }
  if (cell !== "" || row.length) {
    row.push(cell);
    rows.push(row);
  }
  return rows;
}

/** True when pasted text spans more than one cell, so the datasheet should take it over. */
export const isMultiCell = (text: string) => /[\t\n\r]/.test(text.replace(/(\r?\n)+$/, ""));

export interface PastePlan {
  /** Existing rows (page row index) and the column-to-text cells pasted over them. */
  updates: { row: number; cells: Map<number, string> }[];
  /** New rows, each a column-to-text map. */
  inserts: Map<number, string>[];
}

/**
 * Maps a parsed grid onto the datasheet: cell (0, 0) lands on `start`, later
 * columns follow `columns` (the visible, editable column order) and cells past
 * the last one are dropped. Rows past `rowCount` become new records.
 * `start.row` equal to `rowCount` means the paste began in the new-record row.
 */
export function planPaste(
  grid: string[][],
  columns: number[],
  start: { row: number; column: number },
  rowCount: number,
): PastePlan {
  const first = columns.indexOf(start.column);
  const plan: PastePlan = { updates: [], inserts: [] };
  if (first < 0) return plan;
  grid.forEach((values, offset) => {
    const cells = new Map<number, string>();
    values.forEach((text, k) => {
      const column = columns[first + k];
      if (column !== undefined) cells.set(column, text);
    });
    const row = start.row + offset;
    if (row < rowCount) plan.updates.push({ row, cells });
    else plan.inserts.push(cells);
  });
  return plan;
}

/**
 * The record writes for `plan`: updates carry the row as read (`expected`), new
 * rows leave empty cells to their defaults. A cell that does not parse for its
 * column throws, naming the pasted row.
 */
export function pasteWrites(table: string, page: DbPage, plan: PastePlan): RecordWrite[] {
  const at = (k: number, build: () => RecordWrite) => {
    try {
      return build();
    } catch (e) {
      const reason = e instanceof Error ? e.message : String(e);
      throw new Error(`Pasted row ${k + 1}: ${reason}.`, { cause: e });
    }
  };
  return [
    ...plan.updates.map(({ row, cells }, k) =>
      at(k, () => ({
        operation: "update" as const,
        table,
        identity: page.identities[row],
        values: typedValues(page.columns, cells, true),
        meta: { expected: originalValues(page.columns, page.rows[row]) },
      })),
    ),
    ...plan.inserts.map((cells, k) =>
      at(plan.updates.length + k, () => ({
        operation: "insert" as const,
        table,
        identity: null,
        values: typedValues(page.columns, cells, false),
      })),
    ),
  ];
}

const records = (n: number) => (n === 1 ? "1 record" : `${n} records`);

/** Summary shown after a paste, e.g. "Pasted into 2 records and added 1 record." */
export function pastedMessage(updated: number, added: number): string {
  if (!added) return `Pasted into ${records(updated)}.`;
  if (!updated) return `Added ${records(added)}.`;
  return `Pasted into ${records(updated)} and added ${records(added)}.`;
}
