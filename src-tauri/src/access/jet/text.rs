//! Text in Jet/ACE files: UCS-2 with Access' "compressed unicode", and the
//! single-byte code page of Jet 3 (`docs/access-format.md` §4.6).

/// Decodes a text value. Jet 4+ text is UTF-16LE, or compressed when it starts
/// with FF FE: runs of one-byte characters and runs of UTF-16 code units,
/// switched by a 0x00 byte, starting with one-byte characters.
pub fn decode_text(data: &[u8], jet3: bool, code_page: u16) -> String {
    if jet3 {
        return decode_code_page(data, code_page);
    }
    if data.len() > 1 && data[0] == 0xFF && data[1] == 0xFE {
        let mut out = String::with_capacity(data.len());
        let mut compressed = true;
        let mut start = 2;
        for i in 2..=data.len() {
            if i == data.len() || data[i] == 0 {
                push_segment(&data[start..i], compressed, &mut out);
                compressed = !compressed;
                start = i + 1;
            }
        }
        return out;
    }
    utf16(data)
}

fn push_segment(seg: &[u8], compressed: bool, out: &mut String) {
    if compressed {
        out.extend(seg.iter().map(|b| *b as char));
    } else {
        out.push_str(&utf16(seg));
    }
}

pub fn utf16(data: &[u8]) -> String {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

pub fn decode_code_page(data: &[u8], code_page: u16) -> String {
    let enc = codepage::to_encoding(code_page).unwrap_or(encoding_rs::WINDOWS_1252);
    enc.decode_without_bom_handling(data).0.into_owned()
}

/// Reads a length-prefixed name from a TDEF: Jet 3 has a 1 byte length and
/// code-page bytes, Jet 4+ a 2 byte length in bytes and UTF-16LE.
/// Returns the name and the bytes consumed.
pub fn decode_name(b: &[u8], jet3: bool, code_page: u16) -> Result<(String, usize), String> {
    if jet3 {
        let len = *b.first().ok_or("name past the end")? as usize;
        let bytes = b.get(1..1 + len).ok_or("name past the end")?;
        Ok((decode_code_page(bytes, code_page), 1 + len))
    } else {
        let len = u16::from_le_bytes([
            *b.first().ok_or("name past the end")?,
            *b.get(1).ok_or("name past the end")?,
        ]) as usize;
        let bytes = b.get(2..2 + len).ok_or("name past the end")?;
        Ok((utf16(bytes), 2 + len))
    }
}
