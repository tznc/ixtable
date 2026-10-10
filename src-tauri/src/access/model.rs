//! The format-neutral model of an Access database that both readers produce.
//!
//! `accdt` (template packages) and `jet` (`.accdb` / `.mdb` files) fill the same
//! structures; `convert` turns them into an ixtable document. See
//! `docs/access-format.md` for how each field is found in each format.
use std::collections::BTreeMap;

/// Which kind of file the database came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceFormat {
    /// An Access template package (`.accdt`, OPC zip of text definitions).
    Template,
    /// Jet 3 (Access 97) `.mdb`.
    Jet3,
    /// Jet 4 (Access 2000–2003) `.mdb`.
    Jet4,
    /// ACE (Access 2007 and later) `.accdb`.
    Ace,
}

/// Access column data types (TDEF type codes, `od:jetType` names in templates).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColType {
    Boolean,
    Byte,
    Integer,
    Long,
    Currency,
    Single,
    Double,
    DateTime,
    Binary,
    Text,
    Ole,
    Memo,
    Guid,
    Numeric {
        precision: u8,
        scale: u8,
    },
    BigInt,
    ExtDateTime,
    /// A complex (attachment or multi-value) column; see [`Column::complex`].
    Complex,
}

/// What a complex column holds.
#[derive(Debug, Clone, PartialEq)]
pub enum Complex {
    Attachment,
    /// A multi-value field whose values have the given type.
    MultiValue(ColType),
}

/// One column of a table.
#[derive(Debug, Clone)]
pub struct Column {
    pub name: String,
    pub ty: ColType,
    /// Declared length in characters (text) or bytes (binary).
    pub size: u32,
    pub auto_number: bool,
    pub hyperlink: bool,
    pub complex: Option<Complex>,
    /// The expression of a calculated column.
    pub expression: Option<String>,
    /// Design properties by name (`docs/access-format.md` §2.2 lists the ones in use).
    pub props: BTreeMap<String, String>,
}

impl Column {
    pub fn new(name: impl Into<String>, ty: ColType) -> Self {
        Self {
            name: name.into(),
            ty,
            size: 0,
            auto_number: false,
            hyperlink: false,
            complex: None,
            expression: None,
            props: BTreeMap::new(),
        }
    }

    pub fn prop(&self, name: &str) -> Option<&str> {
        self.props
            .get(name)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    pub fn required(&self) -> bool {
        matches!(self.prop("Required"), Some("1" | "-1" | "True" | "true"))
    }
}

/// An index of a table.
#[derive(Debug, Clone, PartialEq)]
pub struct Index {
    pub name: String,
    /// Column names, each with `true` for ascending.
    pub columns: Vec<(String, bool)>,
    pub primary: bool,
    pub unique: bool,
    /// A foreign-key index Access maintains for an enforced relationship.
    pub foreign: bool,
}

/// One table with its schema. Rows are read separately through [`AccessFile::rows`].
#[derive(Debug, Clone)]
pub struct Table {
    pub name: String,
    pub columns: Vec<Column>,
    pub indexes: Vec<Index>,
    /// Table properties: ValidationRule, ValidationText, Description, OrderBy, Filter, ...
    pub props: BTreeMap<String, String>,
    /// Row count when the reader knows it without reading the rows.
    pub row_count: Option<u64>,
}

impl Table {
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn primary_key(&self) -> Option<&Index> {
        self.indexes.iter().find(|i| i.primary)
    }

    pub fn prop(&self, name: &str) -> Option<&str> {
        self.props
            .get(name)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }
}

/// A value read from a table row.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Double(f64),
    /// Currency and decimal values, as a plain decimal string.
    Decimal(String),
    Text(String),
    /// `YYYY-MM-DDTHH:MM:SS[.fff]`, local time as Access stores it.
    DateTime(String),
    Binary(Vec<u8>),
    Guid(String),
    Attachments(Vec<Attachment>),
    Multi(Vec<Value>),
}

/// One file of an attachment field, decoded from the Access wrapper.
#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    pub file_name: String,
    pub file_type: String,
    pub data: Vec<u8>,
}

/// A relationship between two tables (one row of `MSysRelationships` per column).
#[derive(Debug, Clone, PartialEq)]
pub struct Relationship {
    pub name: String,
    /// The many side (`szObject`).
    pub table: String,
    pub columns: Vec<String>,
    /// The one side (`szReferencedObject`).
    pub ref_table: String,
    pub ref_columns: Vec<String>,
    /// `grbit`: see [`rel_flags`].
    pub flags: u32,
}

/// `MSysRelationships.grbit` bits.
pub mod rel_flags {
    pub const ONE_TO_ONE: u32 = 0x1;
    pub const NOT_ENFORCED: u32 = 0x2;
    pub const CASCADE_UPDATES: u32 = 0x100;
    pub const CASCADE_DELETES: u32 = 0x1000;
    pub const CASCADE_NULL: u32 = 0x2000;
    pub const LEFT_JOIN: u32 = 0x0100_0000;
    pub const RIGHT_JOIN: u32 = 0x0200_0000;
}

impl Relationship {
    pub fn enforced(&self) -> bool {
        self.flags & rel_flags::NOT_ENFORCED == 0
    }
}

/// The kind of a saved query (`MSysObjects.Flags & 0xF0`, or the text `Operation`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum QueryKind {
    Select,
    Crosstab,
    Delete,
    Update,
    Append,
    MakeTable,
    DataDefinition,
    PassThrough,
    Union,
}

/// A saved query as Access SQL.
#[derive(Debug, Clone)]
pub struct Query {
    pub name: String,
    pub kind: QueryKind,
    pub sql: String,
    /// Declared `PARAMETERS` (name, Access type name).
    pub parameters: Vec<(String, String)>,
}

/// A form, report or macro in the SaveAsText format, parsed into a tree.
#[derive(Debug, Clone)]
pub struct DesignObject {
    pub name: String,
    pub root: crate::access::text_format::Node,
}

/// A VBA module (standard or class) kept as source text.
#[derive(Debug, Clone)]
pub struct Module {
    pub name: String,
    pub source: String,
}

/// An image shared by forms and reports (template `resources/`).
#[derive(Debug, Clone)]
pub struct Resource {
    pub name: String,
    pub file_name: String,
    pub data: Vec<u8>,
}

/// Everything the importer reads from an Access file except the rows.
#[derive(Debug, Clone)]
pub struct AccessDb {
    pub format: SourceFormat,
    pub tables: Vec<Table>,
    pub relationships: Vec<Relationship>,
    pub queries: Vec<Query>,
    pub forms: Vec<DesignObject>,
    pub reports: Vec<DesignObject>,
    pub macros: Vec<DesignObject>,
    pub modules: Vec<Module>,
    pub resources: Vec<Resource>,
    /// Database properties (AppTitle, StartupForm, ...).
    pub props: BTreeMap<String, String>,
    /// Problems met while reading that did not stop the read.
    pub warnings: Vec<String>,
}

impl AccessDb {
    pub fn new(format: SourceFormat) -> Self {
        Self {
            format,
            tables: vec![],
            relationships: vec![],
            queries: vec![],
            forms: vec![],
            reports: vec![],
            macros: vec![],
            modules: vec![],
            resources: vec![],
            props: BTreeMap::new(),
            warnings: vec![],
        }
    }

    pub fn table(&self, name: &str) -> Option<&Table> {
        self.tables
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
    }

    pub fn query(&self, name: &str) -> Option<&Query> {
        self.queries
            .iter()
            .find(|q| q.name.eq_ignore_ascii_case(name))
    }

    pub fn form(&self, name: &str) -> Option<&DesignObject> {
        self.forms
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(name))
    }
}

/// An opened Access file: its definitions plus a way to stream each table's rows.
pub trait AccessFile: Send {
    fn db(&self) -> &AccessDb;
    /// Calls `f` with each row of `table`, values in the order of its columns.
    fn rows(
        &mut self,
        table: &str,
        f: &mut dyn FnMut(Vec<Value>) -> Result<(), String>,
    ) -> Result<(), String>;
    /// Objects stored compiled, which the reader can name but not read: (kind, name).
    fn compiled_objects(&self) -> Vec<(String, String)> {
        vec![]
    }
}
