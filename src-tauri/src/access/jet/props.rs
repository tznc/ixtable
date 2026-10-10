//! Property maps (`MSysObjects.LvProp`), `docs/access-format.md` §4.9.
//!
//! `MR2\0` (Jet 4+) or `KKD\0` (Jet 3), then blocks of u32 length, u16 type.
//! Type 0x80 lists the property names; the other blocks hold values for the
//! object itself (named "") or for one of its columns (named after it).
use super::page::{u16_at, u32_at};
use super::text::{decode_code_page, decode_text, utf16};
use std::collections::BTreeMap;

/// Property values by map name ("" for the object) then property name.
pub type PropertyMaps = BTreeMap<String, BTreeMap<String, String>>;

pub fn parse(b: &[u8], jet3: bool, code_page: u16) -> PropertyMaps {
    let mut maps = PropertyMaps::new();
    if b.len() < 4 || (&b[..4] != b"MR2\0" && &b[..4] != b"KKD\0") {
        return maps;
    }
    let name = |s: &[u8]| {
        if jet3 {
            decode_code_page(s, code_page)
        } else {
            utf16(s)
        }
    };
    let mut names: Vec<String> = vec![];
    let mut pos = 4;
    while pos + 6 <= b.len() {
        let len = u32_at(b, pos) as usize;
        let ty = u16_at(b, pos + 4);
        let end = (pos + len).min(b.len());
        if len < 6 {
            break;
        }
        let block = &b[pos + 6..end];
        if ty == 0x80 {
            let mut p = 0;
            while p + 2 <= block.len() {
                let l = u16_at(block, p) as usize;
                names.push(name(block.get(p + 2..p + 2 + l).unwrap_or_default()));
                p += 2 + l;
            }
        } else {
            let mut p = 0;
            let mut map_name = String::new();
            if block.len() >= 4 {
                let name_block = u32_at(block, 0) as usize;
                if name_block > 6 {
                    let l = u16_at(block, 4) as usize;
                    map_name = name(block.get(6..6 + l).unwrap_or_default());
                }
                p = name_block.max(4);
            }
            let map = maps.entry(map_name).or_default();
            while p + 8 <= block.len() {
                let vlen = u16_at(block, p) as usize;
                if vlen < 8 {
                    break;
                }
                let dtype = block[p + 3];
                let idx = u16_at(block, p + 4) as usize;
                let dlen = u16_at(block, p + 6) as usize;
                let data = block.get(p + 8..p + 8 + dlen).unwrap_or_default();
                if let Some(n) = names.get(idx) {
                    map.insert(n.clone(), value(dtype, data, jet3, code_page));
                }
                p += vlen;
            }
        }
        pos = end;
    }
    maps
}

/// Renders a property value as the text SaveAsText would show.
fn value(dtype: u8, d: &[u8], jet3: bool, code_page: u16) -> String {
    match dtype {
        0x01 => if d.first().copied().unwrap_or(0) != 0 {
            "1"
        } else {
            "0"
        }
        .into(),
        0x02 => d.first().copied().unwrap_or(0).to_string(),
        0x03 => (u16_at(d, 0) as i16).to_string(),
        0x04 => (u32_at(d, 0) as i32).to_string(),
        0x06 if d.len() >= 4 => f32::from_le_bytes([d[0], d[1], d[2], d[3]]).to_string(),
        0x07 if d.len() >= 8 => f64::from_le_bytes(d[..8].try_into().unwrap_or([0; 8])).to_string(),
        0x0A | 0x0C => decode_text(d, jet3, code_page),
        0x0F => super::row::guid(d),
        _ => String::new(),
    }
}
