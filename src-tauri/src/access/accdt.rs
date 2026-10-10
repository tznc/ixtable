//! Reader for Access template packages (`.accdt`), `docs/access-format.md` §2.
//!
//! A template is an OPC zip. `template/database/objects/` holds one part per
//! object: `table<Name>.xsd` (schema), `sampleData/table<Name>.xml` (rows),
//! `query|form|report|macro<Name>.txt` (SaveAsText) and `module<Name>.txt`
//! (VBA). `properties/<part>_Metadata.xml` carries each object's real name, and
//! `relationships.xml` the rows of `MSysRelationships`.
use crate::access::model::*;
use crate::access::xml::{self, unescape_name, Element};
use crate::access::{blob, query_def, text_format};
use base64::Engine;
use std::collections::BTreeMap;
use std::io::{Read, Seek};

const OBJECTS: &str = "template/database/objects/";
const MAX_PART_BYTES: u64 = 1 << 30;

pub struct TemplatePackage {
    db: AccessDb,
    /// Sample data part per lower-case table name.
    data: BTreeMap<String, Vec<u8>>,
}

/// Reads every part of the package into memory.
fn read_parts<R: Read + Seek>(reader: R) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut zip =
        zip::ZipArchive::new(reader).map_err(|e| format!("not an Access template package: {e}"))?;
    let mut parts = BTreeMap::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        if f.is_dir() {
            continue;
        }
        total += f.size();
        if total > MAX_PART_BYTES {
            return Err("the template package is larger than 1 GB uncompressed".into());
        }
        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        parts.insert(f.name().replace('\\', "/"), buf);
    }
    Ok(parts)
}

impl TemplatePackage {
    pub fn open<R: Read + Seek>(reader: R) -> Result<Self, String> {
        let parts = read_parts(reader)?;
        if !parts.keys().any(|k| k.starts_with(OBJECTS)) {
            return Err(
                "the package has no template/database/objects part; it is not an Access template"
                    .into(),
            );
        }
        let mut db = AccessDb::new(SourceFormat::Template);
        let mut data = BTreeMap::new();
        let mut part_tables = BTreeMap::new();
        let names = object_names(&parts);
        for (path, bytes) in &parts {
            let Some(file) = path.strip_prefix(OBJECTS) else {
                continue;
            };
            if let Some(stem) = file
                .strip_prefix("sampleData/")
                .and_then(|f| f.strip_suffix(".xml"))
            {
                data.insert(stem.to_lowercase(), bytes.clone());
                continue;
            }
            if file.contains('/') {
                continue;
            }
            let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
            let name = names.get(stem).cloned();
            let before = db.tables.len();
            if let Err(e) = read_object(&mut db, file, stem, name, bytes) {
                db.warnings.push(format!("{file}: {e}"));
            }
            if let Some(t) = db.tables.get(before) {
                part_tables.insert(stem.to_lowercase(), t.name.to_lowercase());
            }
        }
        // Sample data parts share the stem of the table's schema part.
        let data = data
            .into_iter()
            .map(|(stem, bytes)| (part_tables.get(&stem).cloned().unwrap_or(stem), bytes))
            .collect();
        if let Some(rel) = parts.get("template/database/relationships.xml") {
            db.relationships = relationships(&xml::decode(rel)).unwrap_or_else(|e| {
                db.warnings.push(format!("relationships.xml: {e}"));
                vec![]
            });
        }
        if let Some(p) = parts.get("template/database/databaseProperties.xml") {
            db.props = database_properties(&xml::decode(p));
        }
        db.resources = resources(&parts);
        Ok(Self { db, data })
    }
}

/// `properties/<stem>_Metadata.xml` → `<Name>`: the object's real name.
fn object_names(parts: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let prefix = format!("{OBJECTS}properties/");
    for (path, bytes) in parts {
        let Some(stem) = path
            .strip_prefix(&prefix)
            .and_then(|p| p.strip_suffix("_Metadata.xml"))
        else {
            continue;
        };
        if let Ok(root) = xml::parse(&xml::decode(bytes)) {
            if let Some(name) = root.child("Name") {
                out.insert(stem.to_string(), name.text());
            }
        }
    }
    out
}

fn read_object(
    db: &mut AccessDb,
    file: &str,
    stem: &str,
    name: Option<String>,
    bytes: &[u8],
) -> Result<(), String> {
    let kinds = ["table", "query", "form", "report", "macro", "module"];
    let Some(kind) = kinds.iter().find(|k| stem.starts_with(*k)) else {
        return Ok(());
    };
    let name = name.unwrap_or_else(|| stem[kind.len()..].to_string());
    match *kind {
        "table" if file.ends_with(".xsd") => {
            for table in schema_tables(&xml::decode(bytes))? {
                if db.table(&table.name).is_some() {
                    db.warnings.push(format!(
                        "{file}: table {} is defined twice; the first definition is kept",
                        table.name
                    ));
                } else {
                    db.tables.push(table);
                }
            }
        }
        "query" => {
            let root =
                text_format::parse(&text_format::decode(bytes)).map_err(|e| e.to_string())?;
            let (kind, sql, parameters) = query_def::from_text(&root)?;
            db.queries.push(Query {
                name,
                kind,
                sql,
                parameters,
            });
        }
        "form" | "report" | "macro" => {
            let root =
                text_format::parse(&text_format::decode(bytes)).map_err(|e| e.to_string())?;
            let obj = DesignObject { name, root };
            match *kind {
                "form" => db.forms.push(obj),
                "report" => db.reports.push(obj),
                _ => db.macros.push(obj),
            }
        }
        "module" => db.modules.push(Module {
            name,
            source: text_format::decode(bytes),
        }),
        _ => {}
    }
    Ok(())
}

/// Tables defined in an XSD part (one table per part in practice).
pub fn schema_tables(text: &str) -> Result<Vec<Table>, String> {
    let root = xml::parse(text)?;
    let mut out = vec![];
    for el in root.children_named("xsd:element") {
        let Some(name) = el.attr("name") else {
            continue;
        };
        if name == "dataroot" {
            continue;
        }
        out.push(schema_table(el)?);
    }
    Ok(out)
}

fn appinfo(el: &Element) -> Option<&Element> {
    el.child("xsd:annotation")
        .and_then(|a| a.child("xsd:appinfo"))
}

fn schema_table(el: &Element) -> Result<Table, String> {
    let name = unescape_name(el.attr("name").unwrap_or_default());
    let mut table = Table {
        name,
        columns: vec![],
        indexes: vec![],
        props: BTreeMap::new(),
        row_count: None,
    };
    if let Some(info) = appinfo(el) {
        for p in info.children_named("od:tableProperty") {
            let key = p.attr("name").unwrap_or_default();
            if !matches!(key, "NameMap" | "GUID" | "DOL") {
                table.props.insert(
                    key.to_string(),
                    p.attr("value").unwrap_or_default().to_string(),
                );
            }
        }
        for i in info.children_named("od:index") {
            let columns = i
                .attr("index-key")
                .unwrap_or_default()
                .split_whitespace()
                .map(|k| match k.strip_prefix('-') {
                    Some(desc) => (unescape_name(desc), false),
                    None => (unescape_name(k), true),
                })
                .collect();
            table.indexes.push(Index {
                name: i.attr("index-name").unwrap_or_default().to_string(),
                columns,
                primary: i.attr("primary") == Some("yes"),
                unique: i.attr("unique") == Some("yes"),
                foreign: false,
            });
        }
    }
    let seq = el
        .child("xsd:complexType")
        .and_then(|c| c.child("xsd:sequence"));
    for c in seq
        .into_iter()
        .flat_map(|s| s.children_named("xsd:element"))
    {
        table.columns.push(schema_column(c));
    }
    Ok(table)
}

fn restriction(el: &Element) -> Option<&Element> {
    el.child("xsd:simpleType")
        .and_then(|s| s.child("xsd:restriction"))
}

fn facet(el: &Element, name: &str) -> Option<u32> {
    restriction(el)?.child(name)?.attr("value")?.parse().ok()
}

/// `od:jetType` names → column types.
pub fn jet_type(name: &str) -> ColType {
    match name {
        "yesno" => ColType::Boolean,
        "byte" => ColType::Byte,
        "integer" => ColType::Integer,
        "longinteger" | "autonumber" => ColType::Long,
        "currency" => ColType::Currency,
        "single" => ColType::Single,
        "double" => ColType::Double,
        "datetime" => ColType::DateTime,
        "datetimeextended" => ColType::ExtDateTime,
        "binary" => ColType::Binary,
        "oleobject" => ColType::Ole,
        "memo" | "hyperlink" => ColType::Memo,
        "replicationid" | "guid" => ColType::Guid,
        "bigint" | "largenumber" => ColType::BigInt,
        "complex" => ColType::Complex,
        "decimal" => ColType::Numeric {
            precision: 18,
            scale: 0,
        },
        _ => ColType::Text,
    }
}

fn schema_column(el: &Element) -> Column {
    let jt = el.attr("od:jetType").unwrap_or("text");
    let mut col = Column::new(
        unescape_name(el.attr("name").unwrap_or_default()),
        jet_type(jt),
    );
    col.auto_number = jt == "autonumber" || el.attr("od:autoUnique") == Some("yes");
    col.hyperlink = jt == "hyperlink" || el.attr("od:hyperlink").is_some();
    col.expression = el.attr("od:expression").map(str::to_string);
    if let ColType::Numeric { .. } = col.ty {
        col.ty = ColType::Numeric {
            precision: facet(el, "xsd:totalDigits").unwrap_or(18) as u8,
            scale: facet(el, "xsd:fractionDigits").unwrap_or(0) as u8,
        };
    }
    col.size = facet(el, "xsd:maxLength").unwrap_or(if col.ty == ColType::Text { 255 } else { 0 });
    if let Some(info) = appinfo(el) {
        for p in info.children_named("od:fieldProperty") {
            let key = p.attr("name").unwrap_or_default();
            if key != "GUID" {
                col.props.insert(
                    key.to_string(),
                    p.attr("value").unwrap_or_default().to_string(),
                );
            }
        }
    }
    if el.attr("od:nonNullable") == Some("yes") && !col.auto_number {
        col.props
            .entry("Required".into())
            .or_insert_with(|| "1".into());
    }
    if col.ty == ColType::Complex {
        col.complex = Some(match el.attr("od:jetComplexType") {
            Some("MSysComplexType_Attachment") => Complex::Attachment,
            _ => {
                let inner = el
                    .child("xsd:complexType")
                    .and_then(|c| c.child("xsd:sequence"))
                    .and_then(|s| s.child("xsd:element"))
                    .and_then(|v| v.attr("od:jetType"))
                    .map(jet_type)
                    .unwrap_or(ColType::Text);
                Complex::MultiValue(inner)
            }
        });
    }
    col
}

/// `relationships.xml`: rows of `MSysRelationships`, one per related column.
pub fn relationships(text: &str) -> Result<Vec<Relationship>, String> {
    let root = xml::parse(text)?;
    let mut rows: Vec<(String, u32, Relationship)> = vec![];
    for r in root.children_named("MSysRelationships") {
        let get = |k: &str| r.child(k).map(|e| e.text()).unwrap_or_default();
        let name = get("szRelationship");
        let icolumn: u32 = get("icolumn").trim().parse().unwrap_or(0);
        rows.push((
            name.clone(),
            icolumn,
            Relationship {
                name,
                table: get("szObject"),
                columns: vec![get("szColumn")],
                ref_table: get("szReferencedObject"),
                ref_columns: vec![get("szReferencedColumn")],
                flags: get("grbit").trim().parse::<i64>().unwrap_or(0) as u32,
            },
        ));
    }
    Ok(merge_relationship_rows(rows))
}

/// Combines the per-column rows of multi-column relationships, ordered by `icolumn`.
pub fn merge_relationship_rows(mut rows: Vec<(String, u32, Relationship)>) -> Vec<Relationship> {
    rows.sort_by(|a, b| (a.0.to_lowercase(), a.1).cmp(&(b.0.to_lowercase(), b.1)));
    let mut out: Vec<Relationship> = vec![];
    for (name, _, r) in rows {
        match out.last_mut() {
            Some(last) if last.name.eq_ignore_ascii_case(&name) => {
                last.columns.extend(r.columns);
                last.ref_columns.extend(r.ref_columns);
            }
            _ => out.push(r),
        }
    }
    out
}

fn database_properties(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Ok(root) = xml::parse(text) {
        let mut props = vec![];
        root.descendants("Property", &mut props);
        for p in props {
            if let Some(name) = p.attr("Name") {
                out.insert(name.to_string(), p.text());
            }
        }
    }
    out
}

/// Shared images: `resources/ResN.<ext>` with the name in `ResN-name.txt` (UTF-16LE).
fn resources(parts: &BTreeMap<String, Vec<u8>>) -> Vec<Resource> {
    let prefix = "template/database/resources/";
    let mut out = vec![];
    for (path, bytes) in parts {
        let Some(file) = path.strip_prefix(prefix) else {
            continue;
        };
        if file.contains('/') || file.ends_with("-name.txt") {
            continue;
        }
        let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
        let name = parts
            .get(&format!("{prefix}{stem}-name.txt"))
            .map(|n| utf16_or_utf8(n))
            .unwrap_or_else(|| stem.to_string());
        out.push(Resource {
            name,
            file_name: file.to_string(),
            data: bytes.clone(),
        });
    }
    out
}

fn utf16_or_utf8(b: &[u8]) -> String {
    if b.len() >= 2 && b.len() % 2 == 0 && b.iter().skip(1).step_by(2).all(|x| *x == 0) {
        let units: Vec<u16> = b
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units)
            .trim_start_matches('\u{feff}')
            .to_string();
    }
    text_format::decode(b)
}

/// Converts one sample-data value to a model value for a column.
pub fn sample_value(col: &Column, el: &Element) -> Value {
    let text = el.text();
    match (&col.ty, &col.complex) {
        (_, Some(Complex::Attachment)) => {
            let get = |k: &str| el.child(k).map(|e| e.text()).unwrap_or_default();
            let content = decode_base64(&get("FileData"));
            let (ext, data) = blob::split_attachment(&content);
            let file_type = if get("FileType").is_empty() {
                ext
            } else {
                get("FileType")
            };
            Value::Attachments(vec![Attachment {
                file_name: get("FileName"),
                file_type,
                data,
            }])
        }
        (_, Some(Complex::MultiValue(ty))) => {
            let inner = el.child("Value").map(|v| v.text()).unwrap_or(text);
            Value::Multi(vec![scalar(*ty, inner.trim())])
        }
        (ColType::Ole | ColType::Binary, _) => Value::Binary(decode_base64(&text)),
        (ty, _) => scalar(*ty, &text),
    }
}

fn scalar(ty: ColType, text: &str) -> Value {
    let t = text.trim();
    match ty {
        ColType::Boolean => Value::Bool(matches!(t, "1" | "-1" | "true" | "True")),
        ColType::Byte | ColType::Integer | ColType::Long | ColType::BigInt => t
            .parse::<i64>()
            .map(Value::Int)
            .unwrap_or_else(|_| Value::Text(text.to_string())),
        ColType::Single | ColType::Double => t
            .parse::<f64>()
            .map(Value::Double)
            .unwrap_or_else(|_| Value::Text(text.to_string())),
        ColType::Currency | ColType::Numeric { .. } => Value::Decimal(t.to_string()),
        ColType::DateTime | ColType::ExtDateTime => Value::DateTime(t.to_string()),
        ColType::Guid => Value::Guid(t.trim_matches(|c| c == '{' || c == '}').to_lowercase()),
        _ => Value::Text(text.to_string()),
    }
}

fn decode_base64(s: &str) -> Vec<u8> {
    let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(clean)
        .unwrap_or_default()
}

impl AccessFile for TemplatePackage {
    fn db(&self) -> &AccessDb {
        &self.db
    }

    fn rows(
        &mut self,
        table: &str,
        f: &mut dyn FnMut(Vec<Value>) -> Result<(), String>,
    ) -> Result<(), String> {
        let Some(t) = self.db.table(table) else {
            return Err(format!("no table {table}"));
        };
        let Some(bytes) = self.data.get(&t.name.to_lowercase()) else {
            return Ok(());
        };
        let root = xml::parse(&xml::decode(bytes))?;
        // The data root follows the inline schema.
        let Some(data_root) = root
            .children_named("dataroot")
            .last()
            .or((root.name == "dataroot").then_some(&root))
        else {
            return Ok(());
        };
        let index: BTreeMap<String, usize> = t
            .columns
            .iter()
            .enumerate()
            .map(|(i, c)| (c.name.to_lowercase(), i))
            .collect();
        for row in data_root.elements() {
            if !unescape_name(&row.name).eq_ignore_ascii_case(&t.name) {
                continue;
            }
            let mut values = vec![Value::Null; t.columns.len()];
            for cell in row.elements() {
                let Some(&i) = index.get(&unescape_name(&cell.name).to_lowercase()) else {
                    continue;
                };
                let v = sample_value(&t.columns[i], cell);
                // Complex values repeat the element once per value.
                values[i] = match (std::mem::replace(&mut values[i], Value::Null), v) {
                    (Value::Attachments(mut a), Value::Attachments(b)) => {
                        a.extend(b);
                        Value::Attachments(a)
                    }
                    (Value::Multi(mut a), Value::Multi(b)) => {
                        a.extend(b);
                        Value::Multi(a)
                    }
                    (_, v) => v,
                };
            }
            f(values)?;
        }
        Ok(())
    }
}
