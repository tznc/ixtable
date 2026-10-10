import type { DesignForm } from "../design/schema";
import type { Filter, Sort } from "../lib/types";
import { loadPage, type RecordPage, recordIdFor, sourceParams, tableSchema } from "./data";
import type { RecordValues } from "./values";

/**
 * A record's place in the ordered records of a collection view (list, continuous, split):
 * the form that lists them, its sort and search, its page parameters, and the index.
 * The record navigation bar moves through these positions.
 */
export type RecordCursor = {
  // Form whose source, filter and sort order define the sequence.
  formId: string;
  index: number;
  sorts: Sort[];
  filters: Filter[];
  params?: Record<string, unknown>;
};

export type Positioned = { recordId: unknown; total: number } | null;

/** The id of the record at `index` of the page's row set (`rows[index]` of `page`). */
export async function idForRow(
  form: DesignForm,
  page: Pick<RecordPage, "rows" | "identities">,
  index: number,
): Promise<unknown> {
  const record: RecordValues | undefined = page.rows[index];
  if (!record) return null;
  const table = form.source?.kind === "table" ? form.source.table : null;
  const identity = page.identities?.[index];
  // Query sources (and keyless reads) open by the row itself, as list mode does.
  if (!table || !identity) return record;
  return recordIdFor(await tableSchema(table), record, identity);
}

/**
 * Reads the record id at `index` of the cursor's sequence through the same DuckDB page
 * reader as list mode, so the order matches what the list showed. Null past the end.
 */
export async function recordAt(
  form: DesignForm,
  cursor: Omit<RecordCursor, "index" | "formId">,
  index: number,
  app: Record<string, unknown>,
): Promise<Positioned> {
  const params = cursor.params ?? {};
  const scope = { app, params };
  const page = await loadPage(
    form,
    { offset: Math.max(0, index), limit: 1, sorts: cursor.sorts, filters: cursor.filters },
    scope,
    sourceParams(form, scope),
  );
  if (!page.rows.length) return { recordId: null, total: page.total };
  return { recordId: await idForRow(form, page, 0), total: page.total };
}
