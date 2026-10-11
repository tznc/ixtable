import { Search, X } from "lucide-react";
import { useState } from "react";
import { asTauriError, readTablePage } from "../../lib/api";
import { type RecordWrite, updateRecord, writeRecordBatch } from "../../lib/records";
import type { DataValue, DbPage, Filter, Sort } from "../../lib/types";
import { logicalOf, valueFromText } from "../../schema/logical";
import {
  type CellAt,
  cellMatches,
  cellText,
  type FindOptions,
  nextMatch,
  replaceText,
} from "./find";
import { originalValues } from "./rows";

/** Largest page the backend serves; replace-all reads the table in pages of this size. */
const SCAN_PAGE = 1000;

/**
 * Access's Find and Replace, as a non-modal panel above the datasheet. Find next
 * walks the whole filtered, sorted table page by page (wrapping once) and moves
 * the datasheet to the match; Replace all rewrites every match in one batch.
 */
export function FindReplace({
  table,
  page,
  pageSize,
  readOnly,
  read,
  visible,
  editable,
  active,
  onFound,
  onWritten,
  onClose,
}: {
  table: string;
  page: DbPage;
  pageSize: number;
  readOnly: boolean;
  /** The sorts and filters the datasheet reads with. */
  read: { sorts: Sort[]; filters: Filter[] };
  visible: number[];
  editable: (column: number) => boolean;
  active: CellAt | null;
  onFound: (offset: number, cell: CellAt) => void;
  onWritten: () => void;
  onClose: () => void;
}) {
  const [find, setFind] = useState("");
  const [replacement, setReplacement] = useState("");
  const [scope, setScope] = useState<"all" | "column">("all");
  const [matchCase, setMatchCase] = useState(false);
  const [wholeField, setWholeField] = useState(false);
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  const options: FindOptions = { find, matchCase, wholeField };
  const columns = scope === "column" && active ? [active.column] : visible;
  const readPage = (offset: number, limit: number) =>
    readTablePage(table, { offset, limit, ...read });

  const run = async (work: () => Promise<void>) => {
    setBusy(true);
    setStatus("");
    try {
      await work();
    } catch (e) {
      setStatus(asTauriError(e).message);
    } finally {
      setBusy(false);
    }
  };

  const locate = async () => {
    const here = nextMatch(page.rows, columns, options, active);
    if (here) return onFound(page.offset, here);
    const pages = Math.ceil(page.total / pageSize);
    const current = Math.floor(page.offset / pageSize);
    // Later pages, then wrap around to the first page through the current one.
    for (let k = 1; k <= pages; k++) {
      const offset = ((current + k) % pages) * pageSize;
      const rows = offset === page.offset ? page.rows : (await readPage(offset, pageSize)).rows;
      const hit = nextMatch(rows, columns, options, null);
      if (hit) return onFound(offset, hit);
    }
    setStatus(`No matches for “${find}”.`);
  };
  const findNext = () => run(locate);

  const write = (row: DataValue[], identity: DataValue[], column: number, text: string) => {
    const meta = page.columns[column];
    return {
      column: meta.name,
      value: valueFromText(replaceText(text, replacement, options), meta.name, logicalOf(meta)),
      identity,
      expected: originalValues(page.columns, row),
    };
  };

  const replace = () =>
    run(async () => {
      const value = active && page.rows[active.row]?.[active.column];
      if (!active || !value || !editable(active.column) || !cellMatches(value, options)) {
        await locate();
        return;
      }
      const row = page.rows[active.row];
      const change = write(row, page.identities[active.row], active.column, cellText(value) ?? "");
      await updateRecord(table, [{ column: change.column, value: change.value }], change.identity, {
        expected: change.expected,
      });
      onWritten();
      const next = nextMatch(page.rows, columns, options, active);
      if (next) onFound(page.offset, next);
    });

  const replaceAll = () =>
    run(async () => {
      const targets = columns.filter(editable);
      const writes: RecordWrite[] = [];
      let cells = 0;
      for (let offset = 0; offset < page.total; offset += SCAN_PAGE) {
        const chunk = await readPage(offset, SCAN_PAGE);
        chunk.rows.forEach((row, i) => {
          const values = targets
            .filter((j) => cellMatches(row[j], options))
            .map((j) => write(row, chunk.identities[i], j, cellText(row[j]) ?? ""));
          if (!values.length) return;
          cells += values.length;
          writes.push({
            operation: "update",
            table,
            identity: chunk.identities[i],
            values: values.map(({ column, value }) => ({ column, value })),
            meta: { expected: values[0].expected },
          });
        });
      }
      if (!writes.length) return setStatus(`No matches for “${find}”.`);
      const records = writes.length === 1 ? "1 record" : `${writes.length} records`;
      if (!window.confirm(`Replace ${cells} matches in ${records}? This cannot be undone.`)) return;
      await writeRecordBatch(writes);
      onWritten();
      setStatus(`Replaced ${cells} matches in ${records}.`);
    });

  return (
    <div className="find-replace" role="dialog" aria-label="Find and replace">
      <label>
        Find
        <input
          autoFocus
          value={find}
          onChange={(e) => setFind(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && find) void findNext();
            if (e.key === "Escape") onClose();
          }}
        />
      </label>
      {!readOnly && (
        <label>
          Replace with
          <input value={replacement} onChange={(e) => setReplacement(e.target.value)} />
        </label>
      )}
      <label>
        Look in
        <select value={scope} onChange={(e) => setScope(e.target.value as "all" | "column")}>
          <option value="all">All columns</option>
          <option value="column" disabled={!active}>
            Current column
          </option>
        </select>
      </label>
      <label className="check">
        <input
          type="checkbox"
          checked={wholeField}
          onChange={(e) => setWholeField(e.target.checked)}
        />
        Whole field
      </label>
      <label className="check">
        <input
          type="checkbox"
          checked={matchCase}
          onChange={(e) => setMatchCase(e.target.checked)}
        />
        Match case
      </label>
      <button type="button" disabled={!find || busy} onClick={() => void findNext()}>
        <Search aria-hidden />
        Find next
      </button>
      {!readOnly && (
        <>
          <button type="button" disabled={!find || busy} onClick={() => void replace()}>
            Replace
          </button>
          <button type="button" disabled={!find || busy} onClick={() => void replaceAll()}>
            Replace all
          </button>
        </>
      )}
      <button type="button" aria-label="Close find and replace" onClick={onClose}>
        <X aria-hidden />
      </button>
      {status && <span role="status">{status}</span>}
    </div>
  );
}
