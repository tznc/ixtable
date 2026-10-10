import { call } from "../lib/api";
import type { CreateTableSpec, DataValue, Filter, SessionState } from "../lib/types";
import type { TotalSpec } from "./sheet/types";

export const createDatabaseTable = (spec: CreateTableSpec) =>
  call<SessionState>("create_database_table", { spec });

/** The datasheet totals row: one value per spec over every row `filters` select. */
export const readTableTotals = (table: string, filters: Filter[], totals: TotalSpec[]) =>
  call<DataValue[]>("read_table_totals", { table, filters, totals });
