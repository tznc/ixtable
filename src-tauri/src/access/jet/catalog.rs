//! System tables: `MSysObjects`, `MSysRelationships`, `MSysQueries`,
//! `MSysComplexColumns` (`docs/access-format.md` §4.8–4.11).
use super::props::{self, PropertyMaps};
use super::tdef::TableDef;
use super::{ComplexSource, JetFile};
use crate::access::accdt::merge_relationship_rows;
use crate::access::model::*;
use crate::access::query_def::{self, Join, OutputColumn, QueryDef};
use std::collections::BTreeMap;

/// The catalog's TDEF is always on page 2.
const CATALOG_PAGE: u32 = 2;

/// `MSysObjects.Type` values.
pub mod object_type {
    pub const TABLE: i64 = 1;
    pub const LINKED_ODBC: i64 = 4;
    pub const QUERY: i64 = 5;
    pub const LINKED: i64 = 6;
    pub const FORM: i64 = -32768;
    pub const REPORT: i64 = -32764;
    pub const MACRO: i64 = -32766;
    pub const MODULE: i64 = -32761;
}

/// One catalog row with the columns the importer uses.
#[derive(Debug, Clone)]
struct CatalogRow {
    id: i64,
    name: String,
    ty: i64,
    flags: i64,
    props: Vec<u8>,
}

/// Named access to a system table's rows.
struct SysTable {
    names: Vec<String>,
    rows: Vec<Vec<Value>>,
}

impl SysTable {
    fn read(f: &mut JetFile, def: &TableDef) -> Result<Self, String> {
        Ok(Self {
            names: def.columns.iter().map(|c| c.name.to_lowercase()).collect(),
            rows: f.read_all(def)?,
        })
    }

    fn col(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == &name.to_lowercase())
    }

    fn get<'a>(&self, row: &'a [Value], name: &str) -> &'a Value {
        self.col(name)
            .and_then(|i| row.get(i))
            .unwrap_or(&Value::Null)
    }

    fn text(&self, row: &[Value], name: &str) -> String {
        match self.get(row, name) {
            Value::Text(t) => t.clone(),
            _ => String::new(),
        }
    }

    fn int(&self, row: &[Value], name: &str) -> i64 {
        match self.get(row, name) {
            Value::Int(i) => *i,
            Value::Bool(b) => *b as i64,
            _ => 0,
        }
    }

    fn bytes(&self, row: &[Value], name: &str) -> Vec<u8> {
        match self.get(row, name) {
            Value::Binary(b) => b.clone(),
            _ => vec![],
        }
    }
}

fn system_table(
    f: &mut JetFile,
    catalog: &[CatalogRow],
    name: &str,
) -> Result<Option<SysTable>, String> {
    let Some(row) = catalog
        .iter()
        .find(|r| r.ty == object_type::TABLE && r.name.eq_ignore_ascii_case(name))
    else {
        return Ok(None);
    };
    let def = f.def_at(row.id as u32)?;
    Ok(Some(SysTable::read(f, &def)?))
}

fn is_system(r: &CatalogRow) -> bool {
    r.flags as u32 & 0x8000_0002 != 0 || r.name.starts_with("MSys") || r.name.starts_with('~')
}

/// Reads the catalog and fills the database model.
pub fn load(f: &mut JetFile) -> Result<(), String> {
    let cat_def = f.def_at(CATALOG_PAGE)?;
    let cat = SysTable::read(f, &cat_def)?;
    let catalog: Vec<CatalogRow> = cat
        .rows
        .iter()
        .map(|r| CatalogRow {
            id: cat.int(r, "Id"),
            name: cat.text(r, "Name"),
            ty: cat.int(r, "Type") as i16 as i64,
            flags: cat.int(r, "Flags"),
            props: cat.bytes(r, "LvProp"),
        })
        .collect();
    let (jet3, cp) = (f.jet3(), f.code_page());
    let maps = |r: &CatalogRow| props::parse(&r.props, jet3, cp);
    if let Some(db) = catalog.iter().find(|r| r.name == "MSysDb") {
        f.db.props = maps(db).remove("").unwrap_or_default();
    }
    let complex = complex_columns(f, &catalog)?;
    for r in catalog.iter().filter(|r| !is_system(r)) {
        match r.ty {
            object_type::TABLE => {
                // Complex value and type tables are hidden support tables.
                if matches!(r.flags as u32 & 0x000F_0000, 0x000A_0000 | 0x0003_0000) {
                    continue;
                }
                match f.def_at(r.id as u32) {
                    Ok(mut def) => {
                        let props = maps(r);
                        // A calculated column stores its value as the type of its result.
                        for c in def.columns.iter_mut().filter(|c| c.calculated()) {
                            let rt = props
                                .get(&c.name)
                                .and_then(|p| p.get("ResultType"))
                                .and_then(|v| v.parse::<u8>().ok());
                            if let Some(rt) = rt.filter(|t| *t > 0) {
                                c.type_code = rt;
                            }
                        }
                        let table = model_table(r, &def, &maps(r), &complex, f);
                        f.defs.insert(table.name.to_lowercase(), def);
                        f.db.tables.push(table);
                    }
                    Err(e) => f.db.warnings.push(format!("table {}: {e}", r.name)),
                }
            }
            object_type::LINKED | object_type::LINKED_ODBC => {
                f.db.warnings.push(format!(
                    "{} is a linked table; link targets are not imported",
                    r.name
                ));
            }
            object_type::FORM => f.design_objects.push(("form".into(), r.name.clone())),
            object_type::REPORT => f.design_objects.push(("report".into(), r.name.clone())),
            object_type::MACRO => f.design_objects.push(("macro".into(), r.name.clone())),
            object_type::MODULE => f.design_objects.push(("module".into(), r.name.clone())),
            _ => {}
        }
    }
    if let Some(rel) = system_table(f, &catalog, "MSysRelationships")? {
        f.db.relationships = relationships(&rel);
    }
    if let Some(q) = system_table(f, &catalog, "MSysQueries")? {
        f.db.queries = queries(&q, &catalog);
    }
    Ok(())
}

fn model_table(
    r: &CatalogRow,
    def: &TableDef,
    maps: &PropertyMaps,
    complex: &BTreeMap<(i64, String), (u32, Complex)>,
    f: &mut JetFile,
) -> Table {
    let jet3 = f.jet3();
    let mut columns = vec![];
    for c in &def.columns {
        // A complex column with no values table (version history) has no model column.
        if c.type_code == 0x12 && !complex.contains_key(&(r.id, c.name.to_lowercase())) {
            continue;
        }
        let mut col = Column::new(c.name.clone(), c.col_type());
        col.auto_number = c.auto_number();
        col.hyperlink = c.hyperlink() && c.type_code == 0x0C;
        col.size = match c.type_code {
            0x0A if !jet3 => c.length as u32 / 2,
            _ => c.length as u32,
        };
        if let Some(p) = maps.get(&c.name) {
            col.props = p.clone();
        }
        if c.calculated() {
            col.expression = col.props.get("Expression").cloned();
        }
        if let Some((flat, kind)) = complex.get(&(r.id, c.name.to_lowercase())) {
            col.complex = Some(kind.clone());
            f.complex.insert(
                (r.name.to_lowercase(), c.name.to_lowercase()),
                ComplexSource {
                    flat_table: *flat,
                    kind: kind.clone(),
                },
            );
        }
        columns.push(col);
    }
    let name_of = |n: u16| {
        def.columns
            .iter()
            .find(|c| c.number == n)
            .map(|c| c.name.clone())
            .unwrap_or_default()
    };
    let mut indexes: Vec<Index> = vec![];
    for i in &def.indexes {
        let idx = Index {
            name: i.name.clone(),
            columns: i
                .columns
                .iter()
                .map(|(n, asc)| (name_of(*n), *asc))
                .collect(),
            primary: i.primary,
            unique: i.unique,
            foreign: i.foreign,
        };
        indexes.push(idx);
    }
    Table {
        name: r.name.clone(),
        columns,
        indexes,
        props: maps.get("").cloned().unwrap_or_default(),
        row_count: Some(def.row_count as u64),
    }
}

/// `MSysComplexColumns`: (conceptual table id, column) → (flat table page, kind).
fn complex_columns(
    f: &mut JetFile,
    catalog: &[CatalogRow],
) -> Result<BTreeMap<(i64, String), (u32, Complex)>, String> {
    let mut out = BTreeMap::new();
    let Some(t) = system_table(f, catalog, "MSysComplexColumns")? else {
        return Ok(out);
    };
    for r in &t.rows {
        let type_name = catalog
            .iter()
            .find(|c| c.id == t.int(r, "ComplexTypeObjectID"))
            .map(|c| c.name.clone())
            .unwrap_or_default();
        let kind = if type_name == "MSysComplexType_Attachment" {
            Complex::Attachment
        } else if type_name.starts_with("MSysComplexTypeVH_") {
            // Version history of an append-only memo: the memo keeps the latest text.
            continue;
        } else {
            let inner = match type_name.trim_start_matches("MSysComplexType_") {
                "UnsignedByte" => ColType::Byte,
                "Short" => ColType::Integer,
                "Long" => ColType::Long,
                "IEEESingle" => ColType::Single,
                "IEEEDouble" => ColType::Double,
                "GUID" => ColType::Guid,
                "Decimal" => ColType::Numeric {
                    precision: 18,
                    scale: 0,
                },
                _ => ColType::Text,
            };
            Complex::MultiValue(inner)
        };
        out.insert(
            (
                t.int(r, "ConceptualTableID"),
                t.text(r, "ColumnName").to_lowercase(),
            ),
            (t.int(r, "FlatTableID") as u32, kind),
        );
    }
    Ok(out)
}

fn relationships(t: &SysTable) -> Vec<Relationship> {
    let rows = t
        .rows
        .iter()
        .map(|r| {
            let name = t.text(r, "szRelationship");
            (
                name.clone(),
                t.int(r, "icolumn") as u32,
                Relationship {
                    name,
                    table: t.text(r, "szObject"),
                    columns: vec![t.text(r, "szColumn")],
                    ref_table: t.text(r, "szReferencedObject"),
                    ref_columns: vec![t.text(r, "szReferencedColumn")],
                    flags: t.int(r, "grbit") as u32,
                },
            )
        })
        .filter(|(_, _, r)| !r.table.starts_with("MSys"))
        .collect();
    merge_relationship_rows(rows)
}

/// One `MSysQueries` row.
struct QRow {
    attribute: i64,
    expression: Option<String>,
    flag: i64,
    name1: Option<String>,
    name2: Option<String>,
    order: Vec<u8>,
}

fn queries(t: &SysTable, catalog: &[CatalogRow]) -> Vec<Query> {
    let mut by_object: BTreeMap<i64, Vec<QRow>> = BTreeMap::new();
    for r in &t.rows {
        let opt = |n: &str| Some(t.text(r, n)).filter(|s| !s.is_empty());
        by_object
            .entry(t.int(r, "ObjectId"))
            .or_default()
            .push(QRow {
                attribute: t.int(r, "Attribute"),
                expression: opt("Expression"),
                flag: t.int(r, "Flag"),
                name1: opt("Name1"),
                name2: opt("Name2"),
                order: t.bytes(r, "Order"),
            });
    }
    let mut out = vec![];
    for q in catalog
        .iter()
        .filter(|c| c.ty == object_type::QUERY && !c.name.starts_with('~'))
    {
        let Some(mut rows) = by_object.remove(&q.id) else {
            continue;
        };
        rows.sort_by(|a, b| a.order.cmp(&b.order));
        let (kind, sql, parameters) = query_sql(&rows);
        out.push(Query {
            name: q.name.clone(),
            kind,
            sql,
            parameters,
        });
    }
    out
}

/// Rebuilds the Access SQL of a query from its `MSysQueries` rows.
fn query_sql(rows: &[QRow]) -> (QueryKind, String, Vec<(String, String)>) {
    let type_row = rows.iter().find(|r| r.attribute == 1);
    let kind = query_def::kind_from_operation(type_row.map(|r| r.flag).unwrap_or(1));
    let text = |r: Option<&QRow>| r.and_then(|r| r.expression.clone()).unwrap_or_default();
    match kind {
        QueryKind::DataDefinition | QueryKind::PassThrough => {
            return (kind, text(type_row), vec![])
        }
        QueryKind::Union => {
            let part = |id: &str| {
                text(
                    rows.iter()
                        .find(|r| r.attribute == 5 && r.name2.as_deref() == Some(id)),
                )
            };
            let flag_row = rows.iter().find(|r| r.attribute == 3);
            let all = if flag_row.is_some_and(|r| r.flag & 0x02 != 0) {
                ""
            } else {
                "ALL "
            };
            let mut sql = format!(
                "{}\nUNION {all}{}",
                part("X7YZ_____1").trim(),
                part("X7YZ_____2").trim()
            );
            let order: Vec<String> = rows
                .iter()
                .filter(|r| r.attribute == 11)
                .map(order_term)
                .collect();
            if !order.is_empty() {
                sql.push_str(&format!("\nORDER BY {}", order.join(", ")));
            }
            return (kind, sql, vec![]);
        }
        _ => {}
    }
    let mut def = QueryDef::new(kind);
    for r in rows {
        match r.attribute {
            1 => def.target = r.name1.clone(),
            2 => def.parameters.push((
                r.name1.clone().unwrap_or_default(),
                query_def::parameter_type(&r.flag.to_string()),
            )),
            3 => {
                def.flags = r.flag as u32;
                def.top = r.name1.clone();
            }
            5 => def
                .inputs
                .push((r.name1.clone().unwrap_or_default(), r.name2.clone())),
            6 => {
                let col = OutputColumn {
                    expression: r.expression.clone().unwrap_or_default(),
                    alias: if kind == QueryKind::Update {
                        None
                    } else {
                        r.name1.clone()
                    },
                    name: r.name2.clone(),
                };
                if kind == QueryKind::Crosstab && r.flag & 0x01 != 0 {
                    def.pivot = Some(col.expression);
                } else if kind == QueryKind::Crosstab && r.flag & 0x03 == 0 {
                    def.transform = Some(match &col.alias {
                        Some(a) => format!("{} AS {}", col.expression, query_def::quote_name(a)),
                        None => col.expression,
                    });
                } else if !(kind == QueryKind::Append && r.flag & 0x8000 != 0) {
                    def.columns.push(col);
                }
            }
            7 => def.joins.push(Join {
                left: r.name1.clone().unwrap_or_default(),
                right: r.name2.clone().unwrap_or_default(),
                expression: r.expression.clone().unwrap_or_default(),
                kind: r.flag,
            }),
            8 => def.where_clause = r.expression.clone(),
            9 => {
                if kind != QueryKind::Crosstab || r.flag & 0x02 != 0 {
                    def.groups.push(r.expression.clone().unwrap_or_default());
                }
            }
            10 => def.having = r.expression.clone(),
            11 => def.order.push((
                r.expression.clone().unwrap_or_default(),
                r.name1
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case("D")),
            )),
            _ => {}
        }
    }
    let params = def.parameters.clone();
    (kind, query_def::to_access_sql(&def), params)
}

fn order_term(r: &QRow) -> String {
    let e = r.expression.clone().unwrap_or_default();
    if r.name1
        .as_deref()
        .is_some_and(|n| n.eq_ignore_ascii_case("D"))
    {
        format!("{e} DESC")
    } else {
        e
    }
}
