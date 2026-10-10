//! Table definition (TDEF) pages, `docs/access-format.md` §4.4.
use super::page::{u16_at, u32_at, Pages, Version};
use super::text::decode_name;
use crate::access::model::ColType;

/// A column as the TDEF describes it.
#[derive(Debug, Clone)]
pub struct ColumnDef {
    pub name: String,
    pub type_code: u8,
    /// Position in the null mask (deleted columns keep their numbers).
    pub number: u16,
    /// Index into the variable-length offset table.
    pub var_index: u16,
    pub fixed_offset: u16,
    pub length: u16,
    pub flags: u8,
    pub ext_flags: u8,
    pub precision: u8,
    pub scale: u8,
    /// Complex column id (ACE type 0x12).
    pub complex_id: u32,
}

impl ColumnDef {
    pub fn fixed(&self) -> bool {
        self.flags & 0x01 != 0
    }

    pub fn auto_number(&self) -> bool {
        self.flags & 0x04 != 0 || self.flags & 0x40 != 0
    }

    pub fn hyperlink(&self) -> bool {
        self.flags & 0x80 != 0
    }

    pub fn calculated(&self) -> bool {
        self.ext_flags & 0xC0 == 0xC0
    }

    pub fn col_type(&self) -> ColType {
        match self.type_code {
            0x01 => ColType::Boolean,
            0x02 => ColType::Byte,
            0x03 => ColType::Integer,
            0x04 => ColType::Long,
            0x05 => ColType::Currency,
            0x06 => ColType::Single,
            0x07 => ColType::Double,
            0x08 => ColType::DateTime,
            0x09 | 0x11 => ColType::Binary,
            0x0A => ColType::Text,
            0x0B => ColType::Ole,
            0x0C => ColType::Memo,
            0x0F => ColType::Guid,
            0x10 => ColType::Numeric {
                precision: self.precision,
                scale: self.scale,
            },
            0x12 => ColType::Complex,
            0x13 => ColType::BigInt,
            0x14 => ColType::ExtDateTime,
            _ => ColType::Binary,
        }
    }
}

/// A physical index (column list) and the logical indexes that use it.
#[derive(Debug, Clone)]
pub struct IndexDef {
    pub name: String,
    /// (column number, ascending)
    pub columns: Vec<(u16, bool)>,
    pub primary: bool,
    pub unique: bool,
    pub foreign: bool,
}

#[derive(Debug, Clone)]
pub struct TableDef {
    pub page: u32,
    pub row_count: u32,
    pub system: bool,
    pub columns: Vec<ColumnDef>,
    pub indexes: Vec<IndexDef>,
    /// Usage map row pointer of the pages owned by the table.
    pub owned_pages: u32,
}

/// Reads the TDEF chain starting at `page` and concatenates the page bodies.
fn read_chain(pages: &mut Pages, page: u32) -> Result<Vec<u8>, String> {
    let mut buf = pages.read(page)?;
    if buf[0] != 0x02 {
        return Err(format!(
            "page {page} is not a table definition (type {})",
            buf[0]
        ));
    }
    let mut next = u32_at(&buf, 4);
    let mut guard = 0;
    while next != 0 {
        guard += 1;
        if guard > 10_000 {
            return Err("table definition chain does not end".into());
        }
        let more = pages.read(next)?;
        next = u32_at(&more, 4);
        buf.extend_from_slice(&more[8..]);
    }
    Ok(buf)
}

pub fn read(pages: &mut Pages, page: u32, code_page: u16) -> Result<TableDef, String> {
    let version = pages.header.version;
    let b = read_chain(pages, page)?;
    let jet3 = version.jet3();
    let (o_rows, o_type, o_ncols, o_nidx, o_nreal, o_owned, o_block) = if jet3 {
        (12, 20, 25, 27, 31, 35, 43)
    } else {
        (16, 40, 45, 47, 51, 55, 63)
    };
    let row_count = u32_at(&b, o_rows);
    let table_type = b[o_type];
    let num_cols = u16_at(&b, o_ncols) as usize;
    let num_idx = u32_at(&b, o_nidx) as usize;
    let num_real = u32_at(&b, o_nreal) as usize;
    if num_cols > 4096 || num_idx > 1024 || num_real > 1024 {
        return Err(format!("table definition on page {page} is corrupt"));
    }
    let mut pos = o_block + num_real * if jet3 { 8 } else { 12 };
    let col_size = if jet3 { 18 } else { 25 };
    let mut columns = Vec::with_capacity(num_cols);
    for _ in 0..num_cols {
        let c = b
            .get(pos..pos + col_size)
            .ok_or("column block past the end")?;
        columns.push(column(c, version));
        pos += col_size;
    }
    for col in columns.iter_mut() {
        let (name, used) = decode_name(&b[pos..], jet3, code_page)?;
        col.name = name;
        pos += used;
    }
    // Physical index column lists.
    let real_size = if jet3 { 39 } else { 52 };
    let mut real: Vec<(Vec<(u16, bool)>, u8)> = vec![];
    for _ in 0..num_real {
        let r = b
            .get(pos..pos + real_size)
            .ok_or("index block past the end")?;
        let cols_at = if jet3 { 0 } else { 4 };
        let mut cols = vec![];
        for i in 0..10 {
            let num = u16_at(r, cols_at + i * 3);
            if num != 0xFFFF {
                cols.push((num, r[cols_at + i * 3 + 2] == 0x01));
            }
        }
        let flags = if jet3 { r[38] } else { r[cols_at + 30 + 12] };
        real.push((cols, flags));
        pos += real_size;
    }
    let info_size = if jet3 { 20 } else { 28 };
    let mut logical: Vec<(usize, u8)> = vec![];
    for _ in 0..num_idx {
        let r = b
            .get(pos..pos + info_size)
            .ok_or("index info past the end")?;
        let at = if jet3 { 0 } else { 4 };
        let real_index = u32_at(r, at + 4) as usize;
        let index_type = r[at + 19];
        logical.push((real_index, index_type));
        pos += info_size;
    }
    let mut indexes = vec![];
    for (real_index, index_type) in logical {
        let (name, used) = decode_name(&b[pos..], jet3, code_page)?;
        pos += used;
        let Some((cols, flags)) = real.get(real_index) else {
            continue;
        };
        indexes.push(IndexDef {
            name,
            columns: cols.clone(),
            primary: index_type == 1,
            unique: index_type == 1 || flags & 0x01 != 0,
            foreign: index_type == 2,
        });
    }
    // Columns are listed in creation order; Access shows them by column number.
    columns.sort_by_key(|c| c.number);
    Ok(TableDef {
        page,
        row_count,
        system: table_type == 0x53,
        columns,
        indexes,
        owned_pages: u32_at(&b, o_owned),
    })
}

fn column(c: &[u8], version: Version) -> ColumnDef {
    if version.jet3() {
        ColumnDef {
            name: String::new(),
            type_code: c[0],
            number: u16_at(c, 1),
            var_index: u16_at(c, 3),
            fixed_offset: u16_at(c, 14),
            length: u16_at(c, 16),
            flags: c[13],
            ext_flags: 0,
            precision: c[11],
            scale: c[12],
            complex_id: 0,
        }
    } else {
        ColumnDef {
            name: String::new(),
            type_code: c[0],
            number: u16_at(c, 5),
            var_index: u16_at(c, 7),
            fixed_offset: u16_at(c, 21),
            length: u16_at(c, 23),
            flags: c[15],
            ext_flags: c[16],
            precision: c[11],
            scale: c[12],
            complex_id: u32_at(c, 11),
        }
    }
}
