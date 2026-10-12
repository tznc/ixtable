import { exportSavedQuery, exportSqlQuery } from "../export/api";
import { ExportMenu } from "../export/ExportMenu";
import type { NamedValue } from "../lib/types";
import type { QueryParameter, SavedQuery } from "./types";

/**
 * Exports the full result of the query shown in the editor. An unmodified saved query
 * exports by id; a draft or edited query exports its current SQL.
 */
export function QueryExport({
  query,
  sql,
  exactSaved,
  params,
  parameters,
}: {
  query: SavedQuery;
  sql: string;
  exactSaved: boolean;
  params: NamedValue[];
  parameters: QueryParameter[];
}) {
  return (
    <div className="query-status">
      <ExportMenu
        name={query.name}
        onExport={(format, path) =>
          exactSaved
            ? exportSavedQuery(query.id, { params }, format, path)
            : exportSqlQuery(sql, { params, parameters }, format, path)
        }
      />
    </div>
  );
}
