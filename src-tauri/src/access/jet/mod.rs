//! Reader for Jet 3/4 `.mdb` and ACE `.accdb` files (`docs/access-format.md` §4).
//!
//! Only reading is supported, and only what an import needs: the catalog
//! (`MSysObjects`), table definitions, rows (including memo/OLE long values and
//! complex attachment and multi-value columns), property maps, relationships
//! (`MSysRelationships`) and saved queries (`MSysQueries`). Forms, reports,
//! macros and modules are stored as compiled binary streams in
//! `MSysAccessStorage`; the reader lists them by name only.
pub mod catalog;
pub mod page;
pub mod props;
pub mod row;
pub mod tdef;
pub mod text;

use crate::access::model::*;
use page::{row_pointer, u32_at, Pages, Version};
use std::collections::BTreeMap;
use tdef::TableDef;

/// How a complex column of a user table finds its values.
#[derive(Debug, Clone)]
pub struct ComplexSource {
    pub flat_table: u32,
    pub kind: Complex,
}

pub struct JetFile {
    pages: Pages,
    db: AccessDb,
    /// TDEF per lower-case user table name.
    defs: BTreeMap<String, TableDef>,
    /// Complex columns per (table, column), lower case.
    complex: BTreeMap<(String, String), ComplexSource>,
    /// Names of forms, reports, macros and modules (not decoded).
    pub design_objects: Vec<(String, String)>,
}

impl JetFile {
    pub fn open(file: std::fs::File) -> Result<Self, String> {
        let pages = Pages::open(file)?;
        let format = match pages.header.version {
            Version::Jet3 => SourceFormat::Jet3,
            Version::Jet4 => SourceFormat::Jet4,
            Version::Ace(_) => SourceFormat::Ace,
        };
        let mut f = Self {
            pages,
            db: AccessDb::new(format),
            defs: BTreeMap::new(),
            complex: BTreeMap::new(),
            design_objects: vec![],
        };
        catalog::load(&mut f)?;
        Ok(f)
    }

    pub fn code_page(&self) -> u16 {
        match self.pages.header.code_page {
            0 => 1252,
            c => c,
        }
    }

    pub fn jet3(&self) -> bool {
        self.pages.header.version.jet3()
    }

    /// Calls `f` with the raw column slots of every live row of a table.
    fn scan(
        pages: &mut Pages,
        def: &TableDef,
        f: &mut dyn FnMut(&mut Pages, Vec<Option<Vec<u8>>>) -> Result<(), String>,
    ) -> Result<(), String> {
        let version = pages.header.version;
        let owned = pages.usage_map(def.owned_pages)?;
        for p in owned {
            if p == 0 || p >= pages.page_count {
                continue;
            }
            let buf = pages.read(p)?;
            if buf[0] != 0x01 || u32_at(&buf, 4) != def.page {
                continue;
            }
            for r in 0..page::rows_on_page(&buf, version) {
                let Some(mut raw) = page::row_on_page(&buf, r, version) else {
                    continue;
                };
                if raw.deleted {
                    continue;
                }
                let mut hops = 0;
                while raw.overflow {
                    hops += 1;
                    if hops > 16 || raw.data.len() < 4 {
                        return Err(format!("row {r} on page {p} has a broken overflow pointer"));
                    }
                    let (np, nr) = row_pointer(u32_at(&raw.data, 0));
                    raw = pages.row(np, nr)?;
                }
                let slots = row::split(&raw.data, &def.columns, version.jet3())
                    .map_err(|e| format!("row {r} on page {p}: {e}"))?;
                f(pages, slots)?;
            }
        }
        Ok(())
    }

    /// Every row of a table as decoded values, in TDEF column order.
    pub fn read_all(&mut self, def: &TableDef) -> Result<Vec<Vec<Value>>, String> {
        let code_page = self.code_page();
        let mut out = vec![];
        Self::scan(&mut self.pages, def, &mut |pages, slots| {
            let mut values = Vec::with_capacity(slots.len());
            for (c, slot) in def.columns.iter().zip(slots) {
                values.push(row::value(pages, c, slot, code_page)?);
            }
            out.push(values);
            Ok(())
        })?;
        Ok(out)
    }

    pub fn def_at(&mut self, page: u32) -> Result<TableDef, String> {
        let code_page = self.code_page();
        tdef::read(&mut self.pages, page, code_page)
    }

    /// Values of a complex column grouped by the owning row's complex id.
    fn complex_values(&mut self, src: &ComplexSource) -> Result<BTreeMap<i64, Vec<Value>>, String> {
        let def = self.def_at(src.flat_table)?;
        let rows = self.read_all(&def)?;
        let pos = |name: &str| {
            def.columns
                .iter()
                .position(|c| c.name.eq_ignore_ascii_case(name))
        };
        let fk = def
            .columns
            .iter()
            .position(|c| c.ext_flags & 0x08 != 0)
            .or_else(|| def.columns.iter().position(|c| c.name.starts_with('_')))
            .ok_or("complex value table has no owner column")?;
        let mut out: BTreeMap<i64, Vec<Value>> = BTreeMap::new();
        for r in rows {
            let Value::Int(owner) = r[fk] else { continue };
            let v = match &src.kind {
                Complex::Attachment => {
                    let get = |n: &str| pos(n).map(|i| r[i].clone()).unwrap_or(Value::Null);
                    let text = |v: Value| {
                        if let Value::Text(t) = v {
                            t
                        } else {
                            String::new()
                        }
                    };
                    let data = match get("FileData") {
                        Value::Binary(b) => {
                            let (_, data) = crate::access::blob::split_attachment(
                                &crate::access::blob::unwrap_attachment(&b)?,
                            );
                            data
                        }
                        _ => vec![],
                    };
                    Value::Attachments(vec![Attachment {
                        file_name: text(get("FileName")),
                        file_type: text(get("FileType")),
                        data,
                    }])
                }
                Complex::MultiValue(_) => pos("Value").map(|i| r[i].clone()).unwrap_or(Value::Null),
            };
            out.entry(owner).or_default().push(v);
        }
        Ok(out)
    }
}

impl AccessFile for JetFile {
    fn db(&self) -> &AccessDb {
        &self.db
    }

    fn compiled_objects(&self) -> Vec<(String, String)> {
        self.design_objects.clone()
    }

    fn rows(
        &mut self,
        table: &str,
        f: &mut dyn FnMut(Vec<Value>) -> Result<(), String>,
    ) -> Result<(), String> {
        let key = table.to_lowercase();
        let def = self
            .defs
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("no table {table}"))?;
        let mut complex = vec![];
        for (i, c) in def.columns.iter().enumerate() {
            if let Some(src) = self
                .complex
                .get(&(key.clone(), c.name.to_lowercase()))
                .cloned()
            {
                complex.push((i, self.complex_values(&src)?, src.kind));
            }
        }
        let code_page = self.code_page();
        Self::scan(&mut self.pages, &def, &mut |pages, slots| {
            let mut values = Vec::with_capacity(slots.len());
            for (c, slot) in def.columns.iter().zip(slots) {
                values.push(row::value(pages, c, slot, code_page)?);
            }
            for (i, map, kind) in &complex {
                let id = if let Value::Int(id) = values[*i] {
                    Some(id)
                } else {
                    None
                };
                let found = id.and_then(|id| map.get(&id)).cloned().unwrap_or_default();
                values[*i] = match kind {
                    Complex::Attachment => Value::Attachments(
                        found
                            .into_iter()
                            .flat_map(|v| {
                                if let Value::Attachments(a) = v {
                                    a
                                } else {
                                    vec![]
                                }
                            })
                            .collect(),
                    ),
                    Complex::MultiValue(_) => Value::Multi(found),
                };
            }
            let kept = def
                .columns
                .iter()
                .zip(values)
                .filter(|(c, _)| {
                    c.type_code != 0x12
                        || complex
                            .iter()
                            .any(|(i, _, _)| def.columns[*i].number == c.number)
                })
                .map(|(_, v)| v)
                .collect();
            f(kept)
        })
    }
}
