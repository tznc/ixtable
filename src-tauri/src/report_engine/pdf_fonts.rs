//! Embedded fallback fonts for the PDF writer. `report_pdf` subsets the bundled Unicode
//! fonts to the characters a report uses; this module writes them as Type0 fonts with
//! Identity-H encoding, a CIDFontType2 descendant (CIDToGIDMap Identity), and a ToUnicode
//! CMap for copy and search.

use crate::report_pdf::PdfFontSubset;
use std::collections::HashMap;

/// Resource name of a fallback font in content streams.
pub fn fallback_font_name(index: usize) -> String {
    format!("/F{}", index + 3)
}

fn hex4(n: u32) -> String {
    format!("{n:04X}")
}

/// UTF-16BE hex of a code point (surrogate pair above the BMP).
fn utf16_hex(cp: u32) -> String {
    if cp < 0x10000 {
        return hex4(cp);
    }
    let v = cp - 0x10000;
    hex4(0xd800 + (v >> 10)) + &hex4(0xdc00 + (v & 0x3ff))
}

/// Code point to CID of a subset.
pub fn cid_map(font: &PdfFontSubset) -> HashMap<u32, u16> {
    font.glyphs.iter().map(|g| (g.code_point, g.cid)).collect()
}

/// Character codes of `text`: one 2-byte CID per character, `.notdef` when missing.
pub fn cid_string(cids: &HashMap<u32, u16>, text: &str) -> String {
    let mut out = String::from("<");
    for ch in text.chars() {
        out += &hex4(u32::from(cids.get(&(ch as u32)).copied().unwrap_or(0)));
    }
    out.push('>');
    out
}

fn to_unicode(font: &PdfFontSubset) -> String {
    let mut glyphs = font.glyphs.clone();
    glyphs.sort_by_key(|g| g.cid);
    let blocks: Vec<String> = glyphs
        .chunks(100)
        .map(|chunk| {
            let lines: Vec<String> = chunk
                .iter()
                .map(|g| format!("<{}> <{}>", hex4(u32::from(g.cid)), utf16_hex(g.code_point)))
                .collect();
            format!("{} beginbfchar\n{}\nendbfchar", chunk.len(), lines.join("\n"))
        })
        .collect();
    let mut parts: Vec<String> = vec![
        "/CIDInit /ProcSet findresource begin".into(),
        "12 dict begin".into(),
        "begincmap".into(),
        "/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def".into(),
        "/CMapName /Adobe-Identity-UCS def".into(),
        "/CMapType 2 def".into(),
        "1 begincodespacerange".into(),
        "<0000> <FFFF>".into(),
        "endcodespacerange".into(),
    ];
    parts.extend(blocks);
    parts.extend(
        ["endcmap", "CMapName currentdict /CMap defineresource pop", "end", "end"].map(String::from),
    );
    parts.join("\n")
}

/// One object of a font subset: a plain body or a stream.
pub enum FontObject {
    Body(String),
    Stream { dict: String, data: Vec<u8> },
}

/// Objects a font subset needs, in order, starting at object `first` (5 objects).
pub fn font_objects(font: &PdfFontSubset, first: usize, font_file: Vec<u8>) -> Vec<FontObject> {
    let (cid_font, descriptor, file, cmap) = (first + 1, first + 2, first + 3, first + 4);
    let mut glyphs = font.glyphs.clone();
    glyphs.sort_by_key(|g| g.cid);
    let widths: Vec<String> = glyphs.iter().map(|g| format!("{} [{}]", g.cid, g.width)).collect();
    let [x0, y0, x1, y1] = font.bbox;
    vec![
        FontObject::Body(format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /{} /Encoding /Identity-H /DescendantFonts [{cid_font} 0 R] /ToUnicode {cmap} 0 R >>",
            font.base_font
        )),
        FontObject::Body(format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {descriptor} 0 R /DW 1000 /W [{}] /CIDToGIDMap /Identity >>",
            font.base_font,
            widths.join(" ")
        )),
        FontObject::Body(format!(
            "<< /Type /FontDescriptor /FontName /{} /Flags 4 /FontBBox [{x0} {y0} {x1} {y1}] /ItalicAngle 0 /Ascent {} /Descent {} /CapHeight {} /StemV 80 /FontFile2 {file} 0 R >>",
            font.base_font, font.ascent, font.descent, font.cap_height
        )),
        FontObject::Stream {
            dict: format!("/Length1 {} /Filter /FlateDecode", font.length1),
            data: font_file,
        },
        FontObject::Stream {
            dict: String::new(),
            data: to_unicode(font).into_bytes(),
        },
    ]
}
