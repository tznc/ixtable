//! Row layout and value decoding (`docs/access-format.md` §4.5–4.7).
use super::page::{row_pointer, u16_at, u32_at, Pages};
use super::tdef::ColumnDef;
use super::text::decode_text;
use crate::access::model::Value;

/// Splits a row into one raw slot per column (None for null); booleans come
/// back as a one-byte slot holding 0 or 1 (they live in the null mask).
pub fn split(
    row: &[u8],
    columns: &[ColumnDef],
    jet3: bool,
) -> Result<Vec<Option<Vec<u8>>>, String> {
    let count_size = if jet3 { 1 } else { 2 };
    if row.len() < count_size {
        return Err("row too short".into());
    }
    let num_cols = if jet3 {
        row[0] as usize
    } else {
        u16_at(row, 0) as usize
    };
    let mask_len = num_cols.div_ceil(8);
    if mask_len > row.len() {
        return Err("null mask past the row".into());
    }
    let mask = &row[row.len() - mask_len..];
    let present = |n: u16| (n as usize) < num_cols && mask[n as usize / 8] & (1 << (n % 8)) != 0;
    let var_offsets = if columns.iter().any(|c| !c.fixed()) {
        if jet3 {
            jet3_var_offsets(row, mask_len)
        } else {
            jet4_var_offsets(row, mask_len)
        }
    } else {
        vec![]
    };
    let mut out = Vec::with_capacity(columns.len());
    for c in columns {
        if c.type_code == 0x01 && !c.calculated() {
            out.push(Some(vec![present(c.number) as u8]));
            continue;
        }
        if !present(c.number) {
            out.push(None);
            continue;
        }
        if c.fixed() {
            let start = count_size + c.fixed_offset as usize;
            let len = fixed_size(c);
            out.push(row.get(start..start + len).map(<[u8]>::to_vec));
        } else {
            let i = c.var_index as usize;
            match (var_offsets.get(i), var_offsets.get(i + 1)) {
                (Some(&s), Some(&e)) if s <= e && e <= row.len() => {
                    out.push(Some(row[s..e].to_vec()))
                }
                _ => out.push(None),
            }
        }
    }
    Ok(out)
}

fn fixed_size(c: &ColumnDef) -> usize {
    if c.calculated() {
        return c.length as usize;
    }
    match c.type_code {
        0x02 => 1,
        0x03 => 2,
        0x04 | 0x06 | 0x12 => 4,
        0x05 | 0x07 | 0x08 | 0x13 => 8,
        0x0F => 16,
        0x10 => 17,
        0x14 => 42,
        _ => c.length as usize,
    }
}

/// Jet 4: u16 offsets stored backwards before the variable column count.
fn jet4_var_offsets(row: &[u8], mask_len: usize) -> Vec<usize> {
    let count_at = row.len() - mask_len - 2;
    let count = u16_at(row, count_at) as usize;
    (0..=count)
        .map(|i| {
            count_at
                .checked_sub(2 * (i + 1))
                .map(|p| u16_at(row, p) as usize)
                .unwrap_or(0)
        })
        .collect()
}

/// Jet 3: one-byte offsets plus a jump table for rows longer than 255 bytes.
fn jet3_var_offsets(row: &[u8], mask_len: usize) -> Vec<usize> {
    let row_end = row.len() - 1;
    let num_var = row[row_end - mask_len] as usize;
    let mut num_jumps = (row.len() - 1) / 256;
    let col_offset = row_end - mask_len - num_jumps - 1;
    if (col_offset.saturating_sub(num_var)) / 256 < num_jumps {
        num_jumps -= 1;
    }
    let mut jumps_used = 0;
    let mut out = Vec::with_capacity(num_var + 1);
    for i in 0..=num_var {
        while jumps_used < num_jumps && i == row[row_end - mask_len - jumps_used - 1] as usize {
            jumps_used += 1;
        }
        let at = col_offset.saturating_sub(i);
        out.push(row[at] as usize + jumps_used * 256);
    }
    out
}

/// Reads a long value (memo, OLE): a 12 byte header in the row, then inline
/// data or one or more LVAL rows.
pub fn long_value(pages: &mut Pages, slot: &[u8]) -> Result<Vec<u8>, String> {
    if slot.len() < 12 {
        return Ok(vec![]);
    }
    let word = u32_at(slot, 0);
    let len = (word & 0x3FFF_FFFF) as usize;
    match word >> 30 {
        2 => Ok(slot[12..].iter().copied().take(len).collect()),
        1 => {
            let (page, row) = row_pointer(u32_at(slot, 4));
            let mut data = pages.row(page, row)?.data;
            data.truncate(len);
            Ok(data)
        }
        _ => {
            let mut out = Vec::with_capacity(len);
            let mut ptr = u32_at(slot, 4);
            let mut guard = 0;
            while ptr != 0 && out.len() < len {
                guard += 1;
                if guard > 1_000_000 {
                    return Err("long value chain does not end".into());
                }
                let (page, row) = row_pointer(ptr);
                let data = pages.row(page, row)?.data;
                ptr = u32_at(&data, 0);
                out.extend_from_slice(data.get(4..).unwrap_or_default());
            }
            out.truncate(len);
            Ok(out)
        }
    }
}

/// Days since 1899-12-30 → `YYYY-MM-DDTHH:MM:SS[.fff]`.
pub fn ole_date(v: f64) -> String {
    if !v.is_finite() {
        return String::new();
    }
    let days = v.trunc();
    let frac = (v - days).abs();
    let base = chrono::NaiveDate::from_ymd_opt(1899, 12, 30).expect("valid date");
    let date = base + chrono::Duration::days(days as i64);
    let millis = (frac * 86_400_000.0).round() as i64;
    let time = chrono::NaiveTime::from_hms_opt(0, 0, 0).expect("valid time")
        + chrono::Duration::milliseconds(millis.min(86_399_999));
    let dt = date.and_time(time);
    if millis % 1000 == 0 {
        dt.format("%Y-%m-%dT%H:%M:%S").to_string()
    } else {
        dt.format("%Y-%m-%dT%H:%M:%S%.3f").to_string()
    }
}

/// A scaled 128 bit integer: sign byte then four u32 words, most significant first.
pub fn numeric(b: &[u8], scale: u8) -> String {
    if b.len() < 17 {
        return String::new();
    }
    let mut v: u128 = 0;
    for w in 0..4 {
        v = (v << 32) | u32_at(b, 1 + w * 4) as u128;
    }
    let digits = v.to_string();
    let scale = scale as usize;
    let mut s = if scale == 0 {
        digits
    } else {
        let padded = format!("{:0>width$}", digits, width = scale + 1);
        let (i, f) = padded.split_at(padded.len() - scale);
        format!("{i}.{f}")
    };
    if b[0] & 0x80 != 0 && v != 0 {
        s.insert(0, '-');
    }
    s
}

/// A calculated decimal is an OLE `DECIMAL`: u16 14 (VT_DECIMAL), scale, sign
/// (0x80 negative), u32 high 32 bits, u64 low 64 bits, both little-endian.
fn calc_numeric(b: &[u8]) -> String {
    if b.len() < 16 {
        return String::new();
    }
    let mut words = [0u8; 17];
    words[0] = b[3];
    // `numeric` reads four words most significant first.
    words[5..9].copy_from_slice(&b[4..8]);
    words[9..13].copy_from_slice(&b[12..16]);
    words[13..17].copy_from_slice(&b[8..12]);
    numeric(&words, b[2])
}

/// Currency: i64 in ten-thousandths.
pub fn currency(b: &[u8]) -> String {
    let v = i64::from_le_bytes(b[..8].try_into().unwrap_or([0; 8]));
    let sign = if v < 0 { "-" } else { "" };
    let a = v.unsigned_abs();
    let frac = a % 10_000;
    let s = format!("{sign}{}.{:04}", a / 10_000, frac);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub fn guid(b: &[u8]) -> String {
    if b.len() < 16 {
        return String::new();
    }
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{}",
        u32_at(b, 0),
        u16_at(b, 4),
        u16_at(b, 6),
        b[8],
        b[9],
        b[10..16]
            .iter()
            .map(|x| format!("{x:02x}"))
            .collect::<String>()
    )
}

/// Decodes one column slot into a value. Long values need the page reader.
pub fn value(
    pages: &mut Pages,
    c: &ColumnDef,
    slot: Option<Vec<u8>>,
    code_page: u16,
) -> Result<Value, String> {
    let jet3 = pages.header.version.jet3();
    let Some(b) = slot else {
        return Ok(Value::Null);
    };
    // A calculated column wraps its last value: length at 16, data from 20.
    let unwrap = |b: Vec<u8>| -> Vec<u8> {
        if b.len() < 20 {
            return b;
        }
        let len = u32_at(&b, 16) as usize;
        b[20..(20 + len).min(b.len())].to_vec()
    };
    if c.calculated() && c.type_code == 0x0C {
        return Ok(Value::Text(decode_text(
            &unwrap(long_value(pages, &b)?),
            jet3,
            code_page,
        )));
    }
    let b = if c.calculated() { unwrap(b) } else { b };
    if c.calculated() && b.is_empty() {
        return Ok(Value::Null);
    }
    if c.calculated() && c.type_code == 0x10 {
        return Ok(Value::Decimal(calc_numeric(&b)));
    }
    // A calculated yes/no result is one byte whatever the declared type.
    if c.calculated() && b.len() == 1 && matches!(c.type_code, 0x01..=0x04) {
        return Ok(Value::Bool(b[0] != 0));
    }
    let int = |n: usize| -> i64 {
        match n {
            1 => b[0] as i64,
            2 => i16::from_le_bytes([b[0], b[1]]) as i64,
            4 => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as i64,
            _ => i64::from_le_bytes(b[..8].try_into().unwrap_or([0; 8])),
        }
    };
    let need = |n: usize| {
        if b.len() < n {
            Err(format!("value of {} is too short", c.name))
        } else {
            Ok(())
        }
    };
    Ok(match c.type_code {
        0x01 => Value::Bool(b.first().is_some_and(|v| *v != 0)),
        0x02 => {
            need(1)?;
            Value::Int(int(1))
        }
        0x03 => {
            need(2)?;
            Value::Int(int(2))
        }
        0x04 | 0x12 => {
            need(4)?;
            Value::Int(int(4))
        }
        0x13 => {
            need(8)?;
            Value::Int(int(8))
        }
        0x05 => {
            need(8)?;
            Value::Decimal(currency(&b))
        }
        0x06 => {
            need(4)?;
            Value::Double(f32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64)
        }
        0x07 => {
            need(8)?;
            Value::Double(f64::from_le_bytes(b[..8].try_into().unwrap_or([0; 8])))
        }
        0x08 => {
            need(8)?;
            Value::DateTime(ole_date(f64::from_le_bytes(
                b[..8].try_into().unwrap_or([0; 8]),
            )))
        }
        0x0F => Value::Guid(guid(&b)),
        0x10 => Value::Decimal(numeric(&b, c.scale)),
        0x14 => Value::DateTime(ext_date(&b)),
        0x0A => Value::Text(decode_text(&b, jet3, code_page)),
        0x0C => Value::Text(decode_text(&long_value(pages, &b)?, jet3, code_page)),
        0x0B => Value::Binary(long_value(pages, &b)?),
        _ => Value::Binary(b),
    })
}

/// Date/Time Extended: ASCII `days:seconds+fraction:7`.
fn ext_date(b: &[u8]) -> String {
    let s = String::from_utf8_lossy(b);
    let mut parts = s.trim_end_matches('\0').split(':');
    let days: i64 = parts
        .next()
        .and_then(|d| d.trim().parse().ok())
        .unwrap_or(0);
    let rest = parts.next().unwrap_or("0");
    let (secs, frac) = rest.split_at(rest.len().saturating_sub(7));
    let secs: i64 = secs.trim().parse().unwrap_or(0);
    let base = chrono::NaiveDate::from_ymd_opt(1, 1, 1).expect("valid date");
    let date = base + chrono::Duration::days(days);
    let time = chrono::NaiveTime::from_hms_opt(0, 0, 0).expect("valid time")
        + chrono::Duration::seconds(secs);
    let frac = frac.trim_end_matches('0');
    let head = date.and_time(time).format("%Y-%m-%dT%H:%M:%S").to_string();
    if frac.is_empty() {
        head
    } else {
        format!("{head}.{frac}")
    }
}
