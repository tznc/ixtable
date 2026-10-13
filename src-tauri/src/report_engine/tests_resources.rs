//! Port of `tests/integration/report-pdf-resources.test.tsx`: Unicode font subsets, PNG
//! alpha as an SMask, and the oversized-PNG placeholder, through the whole export.

use super::fixtures::{base_json, band, lay_with, rows};
use super::pdf::{report_pdf_bytes, PdfAsset, PdfOptions};
use super::{AssetInfo, LayoutOptions};
use flate2::{write::ZlibEncoder, Compression};
use serde_json::json;
use std::{collections::HashMap, io::Write};

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|b| *b as char).collect()
}

/// Whether the text names a `/BaseFont /ABCDEF+<name>` subset with Identity-H encoding.
fn subset_font(text: &str, name: &str) -> bool {
    let needle = format!("+{name} /Encoding /Identity-H");
    text.match_indices(&needle).any(|(at, _)| {
        at >= 17
            && text[at - 6..at].chars().all(|c| c.is_ascii_uppercase())
            && text[..at - 6].ends_with("/BaseFont /")
    })
}

fn crc(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for b in data {
        c ^= u32::from(*b);
        for _ in 0..8 {
            c = (c >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(c & 1));
        }
    }
    !c
}

fn chunk(kind: &str, body: &[u8]) -> Vec<u8> {
    let mut out = (body.len() as u32).to_be_bytes().to_vec();
    let mut tail = kind.as_bytes().to_vec();
    tail.extend(body);
    out.extend(&tail);
    out.extend(crc(&tail).to_be_bytes());
    out
}

fn png(ihdr: &[u8], pixels: &[u8]) -> Vec<u8> {
    let mut z = ZlibEncoder::new(vec![], Compression::default());
    z.write_all(pixels).unwrap();
    let mut out = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
    out.extend(chunk("IHDR", ihdr));
    out.extend(chunk("IDAT", &z.finish().unwrap()));
    out.extend(chunk("IEND", &[]));
    out
}

fn options(title: Option<&str>) -> PdfOptions {
    PdfOptions {
        title: title.map(String::from),
        creation_date: "2026-01-01T00:00:00Z".into(),
        ..Default::default()
    }
}

#[test]
fn exports_greek_cyrillic_and_cjk_text_with_embedded_font_subsets_and_png_alpha_as_an_smask() {
    let report = base_json(vec![(
        "detail",
        band(
            40.0,
            vec![
                json!({"id": "t", "kind": "staticText", "text": "Ωμέγα Жук 漢字 ✓", "x": 0, "y": 0, "w": 300, "h": 16}),
                json!({"id": "i", "kind": "image", "assetId": "logo", "x": 0, "y": 20, "w": 40, "h": 20}),
            ],
            false,
        ),
    )]);
    let rgba = png(&[0, 0, 0, 2, 0, 0, 0, 1, 8, 6, 0, 0, 0], &[0, 255, 0, 0, 255, 0, 255, 0, 128]);
    let mut known = HashMap::new();
    known.insert("logo".to_string(), AssetInfo { media_type: "image/png".into() });
    let layout = LayoutOptions { assets: known, ..Default::default() };
    let doc = lay_with(&report, &rows(json!([{}])), &layout);
    let mut assets = HashMap::new();
    assets.insert("logo".to_string(), PdfAsset { media_type: "image/png".into(), data: rgba });
    let (bytes, warnings) = report_pdf_bytes(&doc, &assets, options(Some("Unicode"))).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let text = latin1(&bytes);
    assert!(subset_font(&text, "DejaVuSans"));
    assert!(subset_font(&text, "DroidSansFallback"));
    assert_eq!(text.matches("/FontFile2 ").count(), 2);
    assert!(text.contains("<03A9>"));
    assert!(text.contains("<6F22>"));
    assert!(!text.contains("(?"));
    let mask = "/Width 2 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /SMask ";
    let after = text.split(mask).nth(1).expect("an image with an SMask");
    assert!(after.starts_with(|c: char| c.is_ascii_digit()));
    assert!(text.contains("/Im1 Do"));
    let (again, _) = report_pdf_bytes(&doc, &assets, options(Some("Unicode"))).unwrap();
    assert_eq!(latin1(&again), text);
}

#[test]
fn prints_an_oversized_png_as_a_placeholder_with_a_warning_instead_of_allocating_it() {
    let report = base_json(vec![(
        "detail",
        band(
            20.0,
            vec![json!({"id": "i", "kind": "image", "assetId": "huge", "x": 0, "y": 0, "w": 40, "h": 20})],
            false,
        ),
    )]);
    let huge = png(&[0, 0, 0x9c, 0x40, 0, 0, 0x9c, 0x40, 8, 6, 0, 0, 0], &[0]);
    let mut known = HashMap::new();
    known.insert("huge".to_string(), AssetInfo { media_type: "image/png".into() });
    let layout = LayoutOptions { assets: known, ..Default::default() };
    let doc = lay_with(&report, &rows(json!([{}])), &layout);
    let mut assets = HashMap::new();
    assets.insert("huge".to_string(), PdfAsset { media_type: "image/png".into(), data: huge });
    let (bytes, warnings) = report_pdf_bytes(&doc, &assets, options(None)).unwrap();
    assert_eq!(warnings, ["Image 1: PNG is 40000×40000, larger than the 50 megapixel limit"]);
    let text = latin1(&bytes);
    assert!(!text.contains("/Subtype /Image"));
    let placeholder = text.split(" re S ").nth(1).expect("a placeholder box");
    assert!(placeholder.contains(" l S Q"));
}
