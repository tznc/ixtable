//! RecordStore configuration, errors, write operations, and plans.
use crate::data::{DataValue, NamedValue};
use crate::manager::AppError;
use serde::{Deserialize, Serialize};

/// The application's record store (PRD §9). Credentials are never stored
/// here: `password_ref` names an entry in the local secret store.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DatasourceConfig {
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub database: String,
    #[serde(default)]
    pub user: String,
    /// libpq sslmode: disable, prefer, require, verify-ca, verify-full.
    #[serde(default = "default_sslmode")]
    pub sslmode: String,
    #[serde(default = "default_schema")]
    pub schema: String,
    /// `shared` (one application credential) or `perUser`.
    #[serde(default = "default_credential_mode")]
    pub credential_mode: String,
    #[serde(default)]
    pub password_ref: Option<String>,
    /// Explicit developer confirmation of a non-TLS connection (PRD §21.4).
    #[serde(default)]
    pub insecure_transport_confirmed: bool,
    #[serde(default)]
    pub insecure_transport_confirmed_at: Option<String>,
    /// Manual runtime install (bundle id) whose entered login applies; never serialized.
    #[serde(skip)]
    pub installation: Option<String>,
    /// Cloud session (window label) whose key grants apply; never serialized.
    #[serde(skip)]
    pub grant_scope: Option<String>,
}
fn default_kind() -> String {
    "sqlite".into()
}
fn default_port() -> u16 {
    5432
}
fn default_sslmode() -> String {
    "require".into()
}
fn default_schema() -> String {
    "public".into()
}
fn default_credential_mode() -> String {
    "shared".into()
}
impl Default for DatasourceConfig {
    fn default() -> Self {
        Self {
            kind: default_kind(),
            id: String::new(),
            host: String::new(),
            port: default_port(),
            database: String::new(),
            user: String::new(),
            sslmode: default_sslmode(),
            schema: default_schema(),
            credential_mode: default_credential_mode(),
            password_ref: None,
            insecure_transport_confirmed: false,
            insecure_transport_confirmed_at: None,
            installation: None,
            grant_scope: None,
        }
    }
}
impl DatasourceConfig {
    pub fn is_postgres(&self) -> bool {
        self.kind == "postgres"
    }
    /// True when the connection may travel without TLS.
    pub fn allows_plaintext(&self) -> bool {
        matches!(self.sslmode.as_str(), "disable" | "allow" | "prefer")
    }
    /// True only for `verify-full`, which checks the server's certificate and name.
    pub fn verifies_server(&self) -> bool {
        self.sslmode == "verify-full"
    }
}

/// Per-entity (table) settings: the record-conflict policy (PRD §19).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EntitySettings {
    pub id: String,
    pub table: String,
    /// `optimistic` (default), `lastWriteWins`, or `customAction`.
    #[serde(default = "default_policy")]
    pub concurrency: String,
    #[serde(default)]
    pub action_id: Option<String>,
    /// Per-column field settings (rich text, attachments, multi-select, input masks).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<FieldSettings>,
}

/// How a column is presented and entered, layered on its logical type
/// (`docs/decisions/field-formats.md`). Keyed by column name: table renames and
/// drops of the column follow it (`commands::alter_table`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FieldSettings {
    pub id: String,
    pub column: String,
    /// One of `FIELD_FORMATS`; none is a plain field of the column's logical type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    /// Access-style input mask (`src/fields/mask.ts`); applies to entry only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_mask: Option<String>,
    /// Choices of a multi-select field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}
/// `richText` holds sanitized HTML in a text column; `attachment` and `multiSelect`
/// hold a JSON array in a text or json column.
pub const FIELD_FORMATS: &[&str] = &["richText", "attachment", "multiSelect"];
fn default_policy() -> String {
    "optimistic".into()
}
impl Default for EntitySettings {
    fn default() -> Self {
        Self {
            id: String::new(),
            table: String::new(),
            concurrency: default_policy(),
            action_id: None,
            fields: vec![],
        }
    }
}
pub const POLICIES: &[&str] = &["optimistic", "lastWriteWins", "customAction"];

/// A backend failure mapped to a stable code (see `StoreCapabilities::error_codes`).
#[derive(Debug, Clone, PartialEq)]
pub struct StoreError {
    pub code: &'static str,
    /// For CONSTRAINT_VIOLATION: not_null, unique, primary_key, foreign_key, check.
    pub constraint: Option<&'static str>,
    pub message: String,
}
impl StoreError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            constraint: None,
            message: message.into(),
        }
    }
    pub fn validation(message: impl Into<String>) -> Self {
        Self::new("VALIDATION_ERROR", message)
    }
    pub fn constraint(kind: &'static str, detail: impl Into<String>) -> Self {
        let label = match kind {
            "not_null" => "Not null constraint failed",
            "unique" => "Unique constraint failed",
            "primary_key" => "Primary key constraint failed",
            "foreign_key" => "Foreign key constraint failed",
            "check" => "Check constraint failed",
            _ => "Constraint failed",
        };
        let detail = detail.into();
        Self {
            code: "CONSTRAINT_VIOLATION",
            constraint: Some(kind),
            message: if detail.is_empty() {
                label.into()
            } else {
                format!("{label}: {detail}")
            },
        }
    }
}
impl From<StoreError> for AppError {
    fn from(e: StoreError) -> Self {
        AppError::new(e.code, e.message)
    }
}
impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

/// One record write inside `execute_write_batch` (all or nothing).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum WriteOp {
    Insert {
        table: String,
        values: Vec<NamedValue>,
    },
    Update {
        table: String,
        values: Vec<NamedValue>,
        identity: Vec<DataValue>,
        #[serde(default)]
        expected: Option<Vec<NamedValue>>,
    },
    Delete {
        table: String,
        identity: Vec<DataValue>,
        #[serde(default)]
        expected: Option<Vec<NamedValue>>,
    },
}
impl WriteOp {
    pub fn table(&self) -> &str {
        match self {
            Self::Insert { table, .. }
            | Self::Update { table, .. }
            | Self::Delete { table, .. } => table,
        }
    }
}
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WriteOutcome {
    pub changed: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<Vec<DataValue>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ChangeMode {
    InPlace,
    Rebuild,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlannedOperation {
    pub summary: String,
    pub mode: ChangeMode,
    pub destructive: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Preview of a schema change: per-operation mode, the generated SQL, and
/// what it affects. Destructive plans must be confirmed by the user.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChangePlan {
    pub table: String,
    pub operations: Vec<PlannedOperation>,
    pub rebuild: bool,
    pub destructive: bool,
    pub statements: Vec<String>,
    pub warnings: Vec<String>,
    pub impact: Option<TableImpact>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InboundForeignKey {
    pub table: String,
    pub columns: Vec<String>,
    pub target_columns: Vec<String>,
    pub on_delete: String,
    pub rows: u64,
}
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Dependent {
    pub kind: String,
    pub id: String,
    pub name: String,
}
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TableImpact {
    pub table: String,
    pub rows: u64,
    pub inbound_foreign_keys: Vec<InboundForeignKey>,
    pub indexes: Vec<String>,
    pub dependents: Vec<Dependent>,
    #[serde(default)]
    pub statements: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreateIndex {
    pub name: String,
    pub table: String,
    pub columns: Vec<String>,
    #[serde(default)]
    pub unique: bool,
}
