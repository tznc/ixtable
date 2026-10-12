import { isInputKind, type DesignForm } from "../design/schema";
import { CommittedWriteError, insertRecord, updateRecord } from "../lib/records";
import type { DataValue, TableSchema } from "../lib/types";
import { compute, disabledColumns, type FormScope } from "./formState";
import { fromDataValue, namedValues, type RecordValues, sameValue } from "./values";

/**
 * Record writes for views that edit many records in place (continuous forms). They match
 * RecordView's save: computed bound inputs are written, disabled fields keep their stored
 * value, and updates carry the loaded values as `expected` (PRD §19).
 */

/** Values to write: the record plus computed bound inputs, minus disabled fields' edits. */
export function valuesToWrite(
  form: DesignForm,
  scope: FormScope,
  original: RecordValues,
): RecordValues {
  const values: RecordValues = { ...scope.record };
  for (const control of form.controls) {
    const column = control.binding?.column;
    if (column && control.computed && isInputKind(control.kind))
      values[column] = compute(control.computed, scope).value;
  }
  for (const column of disabledColumns(form, scope)) {
    if (column in original) values[column] = original[column];
    else delete values[column];
  }
  return values;
}

/** Original values for the optimistic concurrency check. */
export const expectedValues = (schema: TableSchema, original: RecordValues) =>
  namedValues(
    original,
    schema.columns.filter((c) => !c.generated && c.name in original),
  );

export type WriteOutcome = { record: RecordValues; problem?: string };

/** A write that committed but whose sync trigger failed still counts as saved. */
async function committed<T>(run: () => Promise<T>): Promise<{ result: T; problem?: string }> {
  try {
    return { result: await run() };
  } catch (e) {
    if (!(e instanceof CommittedWriteError)) throw e;
    return { result: e.results[0] as T, problem: `Saved. ${e.message}` };
  }
}

/** Inserts `values`; generated keys come back in the returned record. */
export async function createRow(
  table: string,
  schema: TableSchema,
  values: RecordValues,
): Promise<WriteOutcome> {
  const keys = new Set(
    schema.columns
      .filter((c) => c.primaryKeyPosition > 0 && values[c.name] == null)
      .map((c) => c.name),
  );
  const columns = schema.columns.filter(
    (c) => !keys.has(c.name) && values[c.name] != null && !c.generated,
  );
  const { result: id, problem } = await committed(() =>
    insertRecord(table, namedValues(values, columns)),
  );
  const record = { ...values };
  schema.columns
    .filter((c) => c.primaryKeyPosition > 0)
    .forEach((c, i) => {
      if (record[c.name] == null) record[c.name] = fromDataValue((id as DataValue[])[i]);
    });
  return { record, problem };
}

/** Updates the changed columns of the row `identity`; a no-op when nothing changed. */
export async function updateRow(
  table: string,
  schema: TableSchema,
  identity: DataValue[],
  values: RecordValues,
  original: RecordValues,
): Promise<WriteOutcome> {
  const changed = schema.columns.filter(
    (c) => !c.generated && c.name in values && !sameValue(values[c.name], original[c.name]),
  );
  if (!changed.length) return { record: { ...original, ...values } };
  const { problem } = await committed(() =>
    updateRecord(table, namedValues(values, changed), identity, {
      expected: expectedValues(schema, original),
      old: original,
    }),
  );
  return { record: { ...original, ...values }, problem };
}
