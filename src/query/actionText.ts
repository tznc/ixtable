import type { ActionQueryKind } from "./types";

/** Query mode's names for the query types. */
export const KIND_LABELS: Record<"read" | ActionQueryKind, string> = {
  read: "Read rows",
  insert: "Insert rows",
  update: "Update rows",
  delete: "Delete rows",
  replace: "Replace all rows",
};

/** What a run did, in words. */
export function describeRun(
  run: { changed: number; removed: number; dryRun: boolean; table: string },
  kind: ActionQueryKind,
) {
  const rows = (n: number) => `${n.toLocaleString()} ${n === 1 ? "row" : "rows"}`;
  const verb = { insert: "insert", update: "update", delete: "delete", replace: "insert" }[kind];
  const done = { insert: "Inserted", update: "Updated", delete: "Deleted", replace: "Inserted" }[
    kind
  ];
  const removed = kind === "replace" ? ` after removing ${rows(run.removed)}` : "";
  return run.dryRun
    ? `Would ${verb} ${rows(run.changed)} in ${run.table}${removed}. Nothing was changed.`
    : `${done} ${rows(run.changed)} in ${run.table}${removed}.`;
}
