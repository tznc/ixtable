//! Ports of `report-pdf.test.ts` and `report-unicode.test.ts`.

use super::fixtures::{band, base_json, lay, orders, rows};
use super::pdf::{fallback_code_points, num, pdf_date, pdf_string, write_pdf, PdfAsset, PdfOptions};
use super::pdf_images::{decoded_image, jpeg_image, png_image};
use super::text::{fallback_glyph, measure_text, normalize_text, text_runs, wrap_text};
use super::{
    ImageItem, Item, LineItem, Page, RectItem, ReportDocument, TextItem, TextLine,
};
use crate::report_pdf::{PdfFontSubset, PdfGlyph, PdfPngImage};
use base64::Engine as _;
use serde_json::json;
use std::collections::HashMap;

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|b| *b as char).collect()
}

/// Like `latin1` but with high bytes shown as `~`, so string offsets are byte offsets.
fn ascii(bytes: &[u8]) -> String {
    bytes.iter().map(|b| if *b < 128 { *b as char } else { '~' }).collect()
}

fn b64(text: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(text)
}

fn check_structure(bytes: &[u8]) -> String {
    let text = ascii(bytes);
    assert!(text.ends_with("%%EOF\n"));
    let pos = text.rfind("startxref\n").expect("startxref");
    let tail = &text[pos + "startxref\n".len()..];
    let startxref: usize = tail.split('\n').next().unwrap().parse().unwrap();
    assert_eq!(&text[startxref..startxref + 5], "xref\n");
    let mut lines = text[startxref..].split('\n').skip(1);
    let header = lines.next().unwrap();
    let count: usize = header.strip_prefix("0 ").unwrap().parse().unwrap();
    let entries: Vec<&str> = lines.take(count).collect();
    assert_eq!(entries[0], "0000000000 65535 f ");
    for (i, entry) in entries.iter().enumerate().skip(1) {
        assert_eq!(entry.len(), "0000000000 00000 n ".len(), "{entry}");
        assert!(entry.ends_with(" 00000 n "));
        let offset: usize = entry[..10].parse().unwrap();
        let want = format!("{i} 0 obj");
        assert_eq!(&text[offset..offset + want.len()], want);
    }
    let mut from = 0;
    while let Some(found) = text[from..].find("\nstream\n") {
        let at = from + found;
        let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
        let line = &text[line_start..at];
        if let Some(l) = line.rfind("/Length ") {
            let digits: String = line[l + 8..].chars().take_while(char::is_ascii_digit).collect();
            let len: usize = digits.parse().unwrap();
            let start = at + "\nstream\n".len();
            assert_eq!(&text[start + len..start + len + 10], "\nendstream");
        }
        from = at + 8;
    }
    text
}

fn text_item(id: &str, text: &str, line_text: &str, bold: bool) -> Item {
    Item::Text(TextItem {
        component_id: id.into(),
        x: 10.0,
        y: 50.0,
        w: 100.0,
        h: 14.0,
        text: text.into(),
        lines: vec![TextLine { text: line_text.into(), x: 12.0, y: 58.56, width: 36.13 }],
        font_size: 10.0,
        bold,
        gray: 0.0,
    })
}

fn tiny() -> ReportDocument {
    ReportDocument {
        width: 200.0,
        height: 100.0,
        diagnostics: vec![],
        pages: vec![Page {
            number: 1,
            items: vec![
                Item::Rect(RectItem {
                    component_id: "r".into(),
                    x: 10.0,
                    y: 10.0,
                    w: 50.0,
                    h: 20.0,
                    line_width: 1.0,
                    gray: 0.0,
                    fill: Some(0.9),
                }),
                Item::Line(LineItem {
                    component_id: "l".into(),
                    x: 10.0,
                    y: 40.0,
                    w: 100.0,
                    h: 0.0,
                    line_width: 0.5,
                    gray: 0.25,
                }),
                text_item("t", "Café (1)", "Café (1)", true),
            ],
        }],
    }
}

fn opts(title: Option<&str>, date: &str) -> PdfOptions {
    PdfOptions { title: title.map(String::from), creation_date: date.into(), ..PdfOptions::default() }
}

#[test]
fn formats_numbers_strings_and_dates() {
    let got: Vec<String> = [1.0, -0.001, 12.345, 0.1 + 0.2, 100.0].iter().map(|n| num(*n)).collect();
    assert_eq!(got, ["1", "0", "12.35", "0.3", "100"]);
    assert_eq!(pdf_string("a(b)\\c é €—✓"), "(a\\(b\\)\\\\c \\351 \\200\\227?)");
    assert_eq!(pdf_date("2026-01-05T09:08:07.000Z"), "D:20260105090807Z");
}

#[test]
fn writes_a_byte_for_byte_golden_pdf_1_4_file() {
    let bytes = write_pdf(&tiny(), &opts(Some("Tiny"), "2026-01-05T00:00:00Z"));
    let content = [
        "q",
        "0.9 g",
        "0 G 1 w",
        "10 70 50 20 re B",
        "Q",
        "q 0.25 G 0.5 w 10 60 m 110 60 l S Q",
        "BT",
        "/F2 10 Tf 0 g",
        "1 0 0 1 12 41.44 Tm (Caf\\351 \\(1\\)) Tj",
        "ET",
    ]
    .join("\n");
    let objects = [
        "%PDF-1.4\n%\u{e2}\u{e3}\u{cf}\u{d3}\n".to_string(),
        "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".to_string(),
        "2 0 obj\n<< /Type /Pages /Kids [6 0 R] /Count 1 >>\nendobj\n".to_string(),
        "3 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>\nendobj\n".to_string(),
        "4 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>\nendobj\n".to_string(),
        "5 0 obj\n<< /Title (Tiny) /Producer (ixtable report engine) /CreationDate (D:20260105000000Z) >>\nendobj\n".to_string(),
        "6 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents 7 0 R >>\nendobj\n".to_string(),
        format!("7 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n", content.len()),
    ];
    let offsets: Vec<usize> = (0..objects.len()).map(|i| objects[..i].concat().chars().count()).collect();
    let xref = objects.concat().chars().count();
    let mut expected = objects.concat();
    expected += "xref\n0 8\n0000000000 65535 f \n";
    for o in &offsets[1..] {
        expected += &format!("{o:010} 00000 n \n");
    }
    expected += &format!("trailer\n<< /Size 8 /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n");
    assert_eq!(latin1(&bytes), expected);
    check_structure(&bytes);
}

#[test]
fn is_deterministic_for_a_multi_page_report_and_keeps_xref_offsets_valid() {
    let report = base_json(vec![
        (
            "detail",
            band(
                300.0,
                vec![json!({ "id": "n", "kind": "field", "expression": "record.customer & ' (€)'",
                    "x": 0, "y": 0, "w": 100, "h": 14 })],
                false,
            ),
        ),
        (
            "pageFooter",
            band(
                20.0,
                vec![json!({ "id": "p", "kind": "calculated", "expression": "page & '/' & pages",
                    "x": 0, "y": 0, "w": 100, "h": 14 })],
                false,
            ),
        ),
    ]);
    let doc = lay(&report, &orders());
    let options = opts(Some("Sales"), "2026-02-01T10:00:00Z");
    let a = write_pdf(&doc, &options);
    let b = write_pdf(&lay(&report, &orders()), &options);
    assert_eq!(a, b);
    let text = check_structure(&a);
    assert!(text.contains("/Count 3"));
    assert!(text.contains("(Elm \\(\\200\\)) Tj"));
    assert!(text.contains("(3/3) Tj"));
    let later = write_pdf(&doc, &opts(Some("Sales"), "2026-02-02T10:00:00Z"));
    assert_eq!(ascii(&later).replace("D:20260202", "D:20260201"), text);
}

fn sized_image(id: &str, x: f64, asset: &str, media: &str) -> Item {
    Item::Image(ImageItem {
        component_id: id.into(),
        x,
        y: 0.0,
        w: 30.0,
        h: 20.0,
        asset_id: asset.into(),
        media_type: media.into(),
    })
}

#[test]
fn passes_jpeg_through_as_dct_decode_and_draws_placeholders_for_unsupported_images() {
    let jpeg: Vec<u8> = vec![
        0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x02, 0x00, 0x03, 0x03, 1, 0x11, 0, 2, 0x11, 0, 3,
        0x11, 0, 0xff, 0xd9,
    ];
    assert_eq!(
        jpeg_image(&jpeg).unwrap().dict,
        "/Width 3 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode"
    );
    let mut doc = tiny();
    doc.pages[0].items = vec![
        sized_image("a", 0.0, "jpg", "image/jpeg"),
        sized_image("b", 40.0, "bad", "image/png"),
        sized_image("c", 80.0, "jpg", "image/jpeg"),
    ];
    let mut assets = HashMap::new();
    assets.insert("jpg".to_string(), PdfAsset { media_type: "image/jpeg".into(), data: jpeg });
    assets.insert("bad".to_string(), PdfAsset { media_type: "image/png".into(), data: vec![1, 2, 3] });
    let options = PdfOptions { assets, ..opts(None, "2026-01-01T00:00:00Z") };
    let bytes = write_pdf(&doc, &options);
    let text = check_structure(&bytes);
    assert!(text.contains("6 0 obj\n<< /Type /XObject /Subtype /Image /Width 3 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length 23 >>\nstream\n"));
    assert!(text.contains("/XObject << /Im1 6 0 R >>"));
    assert!(text.contains("q 30 0 0 20 0 80 cm /Im1 Do Q"));
    assert!(text.contains("q 30 0 0 20 80 80 cm /Im1 Do Q"));
    assert!(text.contains("q 0.5 G 0.5 w 40 80 30 20 re S 40 80 m 70 100 l 40 100 m 70 80 l S Q"));
}

fn chunk(kind: &str, body: &[u8]) -> Vec<u8> {
    let mut out = vec![0, 0, 0, body.len() as u8];
    out.extend(kind.bytes());
    out.extend(body);
    out.extend([0, 0, 0, 0]);
    out
}

const SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

#[test]
fn passes_non_alpha_png_data_through_with_the_png_predictor_and_rejects_alpha_pngs() {
    let png = |color_type: u8| {
        let mut out = SIGNATURE.to_vec();
        out.extend(chunk("IHDR", &[0, 0, 0, 4, 0, 0, 0, 5, 8, color_type, 0, 0, 0]));
        out.extend(chunk("IDAT", &[1, 2]));
        out.extend(chunk("IDAT", &[3]));
        out.extend(chunk("IEND", &[]));
        out
    };
    let rgb = png_image(&png(2)).unwrap();
    assert_eq!(
        rgb.dict,
        "/Width 4 /Height 5 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /DecodeParms << /Predictor 15 /Colors 3 /BitsPerComponent 8 /Columns 4 >>"
    );
    assert_eq!(rgb.data, [1, 2, 3]);
    assert!(png_image(&png(6)).is_none());
}

// Fallback fonts and alpha images (report-unicode).

fn options() -> PdfOptions {
    opts(None, "2026-01-01T00:00:00Z")
}

#[test]
fn keeps_greek_cyrillic_cjk_and_symbols_and_replaces_the_rest_with_a_question_mark() {
    assert_eq!(normalize_text("Ωμέγα Жук 漢字 → ✓"), "Ωμέγα Жук 漢字 → ✓");
    assert_eq!(normalize_text("שלום سلام"), "???? ????");
    assert_eq!(fallback_glyph('A'), None);
    assert_eq!(fallback_glyph('€'), None);
    assert_eq!(fallback_glyph('Ω').map(|g| g.0), Some(0));
    assert_eq!(fallback_glyph('漢'), Some((1, 1000)));
}

#[test]
fn measures_and_wraps_cjk_text_with_the_fallback_widths() {
    assert_eq!(measure_text("漢字", 10.0, false), 20.0);
    assert_eq!(measure_text("漢字", 10.0, true), 20.0);
    assert_eq!(wrap_text("漢字漢字漢字", 35.0, 10.0, false), ["漢字漢", "字漢字"]);
}

#[test]
fn splits_lines_into_font_runs_with_offsets() {
    let runs = text_runs("Ab Ω漢", 10.0, false);
    let got: Vec<(&str, i32)> = runs.iter().map(|r| (r.text.as_str(), r.font)).collect();
    assert_eq!(got, [("Ab ", -1), ("Ω", 0), ("漢", 1)]);
    assert!((runs[1].dx - measure_text("Ab ", 10.0, false)).abs() < 1e-6);
    assert!((runs[2].dx - measure_text("Ab Ω", 10.0, false)).abs() < 1e-6);
}

fn fonts() -> Vec<PdfFontSubset> {
    vec![
        PdfFontSubset {
            index: 0,
            base_font: "AAAAAA+DejaVuSans".into(),
            data_base64: b64("FONT0"),
            length1: 100,
            bbox: [-1021, -463, 1793, 1232],
            ascent: 928,
            descent: -236,
            cap_height: 729,
            glyphs: vec![PdfGlyph { code_point: 0x3a9, cid: 1, width: 765 }],
        },
        PdfFontSubset {
            index: 1,
            base_font: "BBBBBB+DroidSansFallback".into(),
            data_base64: b64("FONT1"),
            length1: 200,
            bbox: [0, -100, 1000, 900],
            ascent: 1050,
            descent: -270,
            cap_height: 1050,
            glyphs: vec![
                PdfGlyph { code_point: 0x6f22, cid: 2, width: 1000 },
                PdfGlyph { code_point: 0x20bb7, cid: 1, width: 1000 },
            ],
        },
    ]
}

fn doc(text: &str, bold: bool) -> ReportDocument {
    lay(
        &base_json(vec![(
            "detail",
            band(
                20.0,
                vec![json!({ "id": "t", "kind": "staticText", "text": text, "x": 0, "y": 0, "w": 300,
                    "h": 14, "style": { "bold": bold } })],
                false,
            ),
        )]),
        &rows(json!([{}])),
    )
}

#[test]
fn embeds_type0_fonts_and_draws_fallback_runs_as_cids() {
    let layout = doc("Ab Ω漢", false);
    assert_eq!(fallback_code_points(&layout), [0x3a9, 0x6f22]);
    let with_fonts = PdfOptions { fonts: fonts(), ..options() };
    let text = latin1(&write_pdf(&layout, &with_fonts));
    for want in [
        "6 0 obj\n<< /Type /Font /Subtype /Type0 /BaseFont /AAAAAA+DejaVuSans /Encoding /Identity-H /DescendantFonts [7 0 R] /ToUnicode 10 0 R >>",
        "/CIDToGIDMap /Identity",
        "/W [1 [765]]",
        "/W [1 [1000] 2 [1000]]",
        "<< /Length1 100 /Filter /FlateDecode /Length 5 >>\nstream\nFONT0",
        "/FontFile2 9 0 R",
        "<0001> <03A9>",
        "<0001> <D842DFB7>",
        "/Font << /F1 3 0 R /F2 4 0 R /F3 6 0 R /F4 11 0 R >>",
    ] {
        assert!(text.contains(want), "missing: {want}");
    }
    let runs = text_runs("Ab Ω漢", 10.0, false);
    let marker = "1 0 0 1 38 ";
    let at = text.find(marker).expect("first run") + marker.len();
    let y: String = text[at..].chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let want = format!(
        "BT\n/F1 10 Tf 0 g\n1 0 0 1 38 {y} Tm (Ab ) Tj\n/F3 10 Tf\n1 0 0 1 {} {y} Tm <0001> Tj\n/F4 10 Tf\n1 0 0 1 {} {y} Tm <0002> Tj\nET",
        num(38.0 + runs[1].dx),
        num(38.0 + runs[2].dx)
    );
    assert!(text.contains(&want), "missing text block: {want}");
    assert_eq!(latin1(&write_pdf(&layout, &with_fonts)), text);
}

#[test]
fn strokes_bold_fallback_runs_and_prints_a_question_mark_without_a_subset() {
    let bold = latin1(&write_pdf(&doc("Ω", true), &PdfOptions { fonts: fonts(), ..options() }));
    let marker = "\n0 G 0.3 w 2 Tr 1 0 0 1 38 ";
    let at = bold.find(marker).expect("stroked run");
    let rest = &bold[at + marker.len()..];
    let y: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    assert!(!y.is_empty());
    assert!(rest[y.len()..].starts_with(" Tm <0001> Tj 0 Tr\n"));
    let plain = latin1(&write_pdf(&doc("Ω", false), &options()));
    assert!(plain.contains("(?) Tj"));
    assert!(!plain.contains("Type0"));
}

#[test]
fn leaves_win_ansi_only_reports_byte_identical() {
    let layout = doc("Café", false);
    assert!(fallback_code_points(&layout).is_empty());
    assert_eq!(
        latin1(&write_pdf(&layout, &PdfOptions { fonts: vec![], ..options() })),
        latin1(&write_pdf(&layout, &options()))
    );
}

fn alpha_image() -> ReportDocument {
    ReportDocument {
        width: 200.0,
        height: 100.0,
        diagnostics: vec![],
        pages: vec![Page { number: 1, items: vec![sized_image("i", 0.0, "logo", "image/png")] }],
    }
}

#[test]
fn writes_decoded_pngs_with_an_smask() {
    let decoded = PdfPngImage {
        width: 2,
        height: 1,
        colors: 3,
        color_base64: b64("RGB"),
        alpha_base64: Some(b64("A")),
    };
    let mut images = HashMap::new();
    images.insert("logo".to_string(), decoded.clone());
    let text = latin1(&write_pdf(&alpha_image(), &PdfOptions { decoded_images: images, ..options() }));
    for want in [
        "6 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /SMask 7 0 R /Length 3 >>\nstream\nRGB",
        "7 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length 1 >>\nstream\nA",
        "/XObject << /Im1 6 0 R >>",
        "/Count 1",
        "8 0 obj\n<< /Type /Page",
        "q 30 0 0 20 0 80 cm /Im1 Do Q",
    ] {
        assert!(text.contains(want), "missing: {want}");
    }
    let opaque = decoded_image(&PdfPngImage { colors: 1, alpha_base64: None, ..decoded });
    assert!(opaque.smask.is_none());
    assert!(opaque.dict.contains("/DeviceGray"));
}

#[test]
fn sends_pngs_with_trns_transparency_or_16_bit_samples_to_the_fallback_instead_of_passing_them_through() {
    let palette = |extra: Vec<u8>| {
        let mut out = SIGNATURE.to_vec();
        out.extend(chunk("IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 3, 0, 0, 0]));
        out.extend(chunk("PLTE", &[1, 2, 3]));
        out.extend(extra);
        out.extend(chunk("IDAT", &[1]));
        out.extend(chunk("IEND", &[]));
        out
    };
    assert!(png_image(&palette(vec![])).is_some());
    assert!(png_image(&palette(chunk("tRNS", &[0]))).is_none());
    let rgb = |depth: u8| {
        let mut out = SIGNATURE.to_vec();
        out.extend(chunk("IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, depth, 2, 0, 0, 0]));
        out.extend(chunk("IDAT", &[1]));
        out.extend(chunk("IEND", &[]));
        out
    };
    assert!(png_image(&rgb(8)).unwrap().dict.contains("/BitsPerComponent 8"));
    assert!(png_image(&rgb(16)).is_none());
    let mut gray16 = rgb(16);
    gray16[8 + 8 + 9] = 0;
    assert!(png_image(&gray16).is_none());
}
