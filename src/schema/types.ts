import type { FieldSettings } from "../fields/types";

/** The application's record store (PRD §9). The password lives in the local secret store; `passwordRef` names it. */
export interface DatasourceConfig {
  kind: "sqlite" | "postgres" | (string & {});
  id?: string;
  host?: string;
  port?: number;
  database?: string;
  user?: string;
  // libpq sslmode: disable, allow, prefer, require, verify-ca, verify-full.
  sslmode?: string;
  schema?: string;
  credentialMode?: "shared" | "perUser" | (string & {});
  passwordRef?: string | null;
  insecureTransportConfirmed?: boolean;
  insecureTransportConfirmedAt?: string | null;
}

export type ConcurrencyPolicy = "optimistic" | "lastWriteWins" | "customAction";

/** Per-table settings; identity is the stable `id`, `table` is the current table name. */
export interface EntitySettings {
  id: string;
  table: string;
  concurrency?: ConcurrencyPolicy | (string & {});
  actionId?: string | null;
  /** Per-column field settings (rich text, attachments, multi-select, input masks). */
  fields?: FieldSettings[];
}

export type ChangeMode = "inPlace" | "rebuild" | "unsupported";

export interface StoreCapabilities {
  store: "sqlite" | "postgres" | (string & {});
  logicalTypes: Array<{
    logicalType: string;
    physicalType: string;
    enforcement: string;
    maxPrecision?: number;
  }>;
  ddl: Array<{ operation: string; mode: ChangeMode; notes: string }>;
  constraints: string[];
  foreignKeyActions: string[];
  indexes: { unique: boolean; multiColumn: boolean; partial: boolean; expression: boolean };
  transactions: {
    atomicBatches: boolean;
    transactionalDdl: boolean;
    savepoints: boolean;
    isolation: string;
  };
  parameterStyle: string;
  generatedValues: string[];
  migrations: { transactionalDdl: boolean; dryRun: string; healthChecks: string[]; notes: string };
  errorCodes: Array<{ code: string; constraint?: string; native: string; description: string }>;
  concurrency: {
    policies: string[];
    optimisticCheck: string;
    rowLocking: boolean;
    multiUser: boolean;
    notes: string;
  };
}

export interface InboundForeignKey {
  table: string;
  columns: string[];
  targetColumns: string[];
  onDelete: string;
  rows: number;
}

export interface TableImpact {
  table: string;
  rows: number;
  inboundForeignKeys: InboundForeignKey[];
  indexes: string[];
  dependents: Array<{ kind: string; id: string; name: string }>;
  statements: string[];
}

export interface PlannedOperation {
  summary: string;
  mode: ChangeMode;
  destructive: boolean;
  reason?: string;
}

export interface ChangePlan {
  table: string;
  operations: PlannedOperation[];
  rebuild: boolean;
  destructive: boolean;
  statements: string[];
  warnings: string[];
  impact: TableImpact | null;
}

export interface ConnectionReport {
  ok: boolean;
  store: string;
  serverVersion: string | null;
  encrypted: boolean;
  message: string;
}

export interface DatasourceStatus {
  kind: string;
  attached: boolean;
  error: string | null;
  hasPassword: boolean;
}

export interface IndexDefinition {
  name: string;
  table: string;
  columns: string[];
  unique: boolean;
  sql?: string | null;
}
