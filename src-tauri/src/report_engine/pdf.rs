//! Minimal deterministic PDF 1.4 writer for laid-out reports (no dependencies).
//!
//! - Fonts: standard Helvetica / Helvetica-Bold, WinAnsiEncoding (not embedded), plus
//!   embedded subsets of the bundled fallback fonts for other characters (`pdf_fonts`).
//! - Graphics: text, lines, rectangles in gray levels; chart paths in RGB; JPEG and opaque
//!   PNG images passed through, other PNGs decoded by `report_pdf` with an alpha soft mask
//!   (`pdf_images`).
//! - Determinism: fixed object order, uncompressed content streams, no random file id, and
//!   the CreationDate comes from the options. Same input, same bytes.
//!
//! `report_pdf_bytes` is the whole export: it asks `report_pdf` for font subsets and PNG
//! decoding only when the report needs them.

use super::pdf_fonts::{cid_map, cid_string, fallback_font_name, font_objects, FontObject};
use super::pdf_images::{decoded_image, pdf_image, PdfImage};
use super::text::{fallback_glyph, text_runs, win_ansi_code};
use super::{js_round, Item, Mark, PathItem, PathOp, ReportDocument, TextItem};
use crate::manager::AppError;
use crate::report_pdf::{PdfFontSubset, PdfPngImage};
use base64::Engine as _;
use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone)]
pub struct PdfAsset {
    pub media_type: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct PdfOptions {
    pub title: Option<String>,
    /// ISO timestamp written as /CreationDate. Required so output is reproducible.
    pub creation_date: String,
    pub assets: HashMap<String, PdfAsset>,
    /// Fallback font subsets from `report_pdf`; without them those characters print as `?`.
    pub fonts: Vec<PdfFontSubset>,
    /// PNG assets decoded by `report_pdf`, by asset id.
    pub decoded_images: HashMap<String, PdfPngImage>,
}

/// Formats a number for content streams: at most 2 decimals, no exponent, no -0.
pub fn num(n: f64) -> String {
    let r = js_round(n * 100.0) / 100.0;
    if !r.is_finite() || r == 0.0 {
        return "0".into();
    }
    let s = format!("{r:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A PDF literal string in WinAnsi with `\`, `(`, `)` and non-ASCII bytes escaped.
pub fn pdf_string(text: &str) -> String {
    let mut out = String::from("(");
    for ch in text.chars() {
        let code = win_ansi_code(ch);
        if code == 0x5c || code == 0x28 || code == 0x29 {
            out.push('\\');
            out.push(char::from_u32(code).unwrap_or('?'));
        } else if !(32..=126).contains(&code) {
            out += &format!("\\{code:03o}");
        } else {
            out.push(char::from_u32(code).unwrap_or('?'));
        }
    }
    out.push(')');
    out
}

/// `D:YYYYMMDDHHmmSSZ` from an ISO timestamp (UTC).
pub fn pdf_date(iso: &str) -> String {
    use chrono::{DateTime, Utc};
    let parsed = DateTime::parse_from_rfc3339(iso)
        .map(|d| d.with_timezone(&Utc))
        .or_else(|_| {
            chrono::NaiveDate::parse_from_str(iso, "%Y-%m-%d")
                .map(|d| d.and_hms_opt(0, 0, 0).unwrap_or_default().and_utc())
        });
    match parsed {
        Ok(d) => d.format("D:%Y%m%d%H%M%SZ").to_string(),
        Err(_) => "D:19700101000000Z".into(),
    }
}

fn ascii(s: &str) -> Vec<u8> {
    s.chars().map(|c| (c as u32 & 0xff) as u8).collect()
}

type Fonts = HashMap<usize, HashMap<u32, u16>>;

/// Text operators of one line: Helvetica runs as WinAnsi strings, fallback runs as CIDs.
fn line_ops(
    item: &TextItem,
    line: &super::TextLine,
    y: &str,
    fonts: &Fonts,
    current: &mut String,
) -> String {
    let base = if item.bold { "/F2" } else { "/F1" };
    let size = num(item.font_size);
    let mut ops: Vec<String> = vec![];
    for run in text_runs(&line.text, item.font_size, item.bold) {
        let cids = (run.font >= 0).then(|| fonts.get(&(run.font as usize))).flatten();
        let font = match cids {
            Some(_) => fallback_font_name(run.font as usize),
            None => base.to_string(),
        };
        if font != *current {
            ops.push(format!("{font} {size} Tf"));
        }
        *current = font;
        let text = match cids {
            Some(cids) => cid_string(cids, &run.text),
            None => pdf_string(&run.text),
        };
        let show = format!("1 0 0 1 {} {y} Tm {text} Tj", num(line.x + run.dx));
        // Fallback fonts have no bold face: stroke the outlines (render mode 2) instead.
        ops.push(if cids.is_some() && item.bold {
            format!("{} G {} w 2 Tr {show} 0 Tr", num(item.gray), num(item.font_size * 0.03))
        } else {
            show
        });
    }
    ops.join("\n")
}

/// `r g b` of a `#rrggbb` color, each 0-1.
pub fn rgb(color: &str) -> String {
    (0..3)
        .map(|i| {
            let v = color
                .get(1 + i * 2..3 + i * 2)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .map_or(0.0, |v| f64::from(v) / 255.0);
            num(v)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A chart path: filled, stroked, or both, with round joins like the preview.
fn path_ops(item: &PathItem, y: &dyn Fn(f64) -> String) -> String {
    let stroke = item.stroke.is_some() && item.line_width > 0.0;
    let fill = item.fill.is_some();
    if (!stroke && !fill) || item.ops.is_empty() {
        return String::new();
    }
    let segments: Vec<String> = item
        .ops
        .iter()
        .map(|op| match op {
            PathOp::M(a, b) => format!("{} {} m", num(*a), y(*b)),
            PathOp::L(a, b) => format!("{} {} l", num(*a), y(*b)),
            PathOp::C(a, b, c, d, e, f) => {
                format!("{} {} {} {} {} {} c", num(*a), y(*b), num(*c), y(*d), num(*e), y(*f))
            }
            PathOp::Z => "h".into(),
        })
        .collect();
    let paint = if stroke && fill {
        "B"
    } else if stroke {
        "S"
    } else {
        "f"
    };
    [
        "q".to_string(),
        item.fill.as_deref().map(|f| format!("{} rg", rgb(f))).unwrap_or_default(),
        if stroke {
            format!(
                "{} RG {} w 1 j",
                rgb(item.stroke.as_deref().unwrap_or("#000000")),
                num(item.line_width)
            )
        } else {
            String::new()
        },
        segments.join(" "),
        paint.to_string(),
        "Q".to_string(),
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join("\n")
}

fn text_ops(item: &TextItem, y: &dyn Fn(f64) -> String, fonts: &Fonts) -> String {
    if item.lines.is_empty() {
        return String::new();
    }
    let font = if item.bold { "/F2" } else { "/F1" };
    let mut current = font.to_string();
    let lines: Vec<String> = item
        .lines
        .iter()
        .map(|line| line_ops(item, line, &y(line.y), fonts, &mut current))
        .collect();
    let mut out = vec![
        "BT".to_string(),
        format!("{font} {} Tf {} g", num(item.font_size), num(item.gray)),
    ];
    out.extend(lines);
    out.push("ET".into());
    out.join("\n")
}

fn item_ops(
    item: &Item,
    page_height: f64,
    image: &dyn Fn(&str) -> Option<String>,
    fonts: &Fonts,
) -> String {
    let y = |v: f64| num(page_height - v);
    match item {
        Item::Rect(r) => {
            let stroke = r.line_width > 0.0;
            let fill = r.fill.is_some();
            if !stroke && !fill {
                return String::new();
            }
            let op = if stroke && fill {
                "B"
            } else if stroke {
                "S"
            } else {
                "f"
            };
            [
                "q".to_string(),
                r.fill.map(|f| format!("{} g", num(f))).unwrap_or_default(),
                if stroke {
                    format!("{} G {} w", num(r.gray), num(r.line_width))
                } else {
                    String::new()
                },
                format!("{} {} {} {} re {op}", num(r.x), y(r.y + r.h), num(r.w), num(r.h)),
                "Q".to_string(),
            ]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
        }
        Item::Line(l) => format!(
            "q {} G {} w {} {} m {} {} l S Q",
            num(l.gray),
            num(l.line_width),
            num(l.x),
            y(l.y),
            num(l.x + l.w),
            y(l.y + l.h)
        ),
        Item::Text(t) => text_ops(t, &y, fonts),
        Item::Chart(c) => c
            .marks
            .iter()
            .map(|mark| match mark {
                Mark::Path(p) => path_ops(p, &y),
                Mark::Text(t) => text_ops(t, &y, fonts),
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Item::Image(i) => {
            if let Some(name) = image(&i.asset_id) {
                return format!(
                    "q {} 0 0 {} {} {} cm {name} Do Q",
                    num(i.w),
                    num(i.h),
                    num(i.x),
                    y(i.y + i.h)
                );
            }
            // Unsupported or missing image data: a crossed placeholder box.
            let (x0, x1) = (num(i.x), num(i.x + i.w));
            let (y0, y1) = (y(i.y + i.h), y(i.y));
            format!(
                "q 0.5 G 0.5 w {x0} {y0} {} {} re S {x0} {y0} m {x1} {y1} l {x0} {y1} m {x1} {y0} l S Q",
                num(i.w),
                num(i.h)
            )
        }
    }
}

struct Out {
    bytes: Vec<u8>,
    offsets: HashMap<usize, usize>,
}

impl Out {
    fn write(&mut self, part: &[u8]) {
        self.bytes.extend_from_slice(part);
    }
    fn object(&mut self, n: usize, body: &str) {
        self.offsets.insert(n, self.bytes.len());
        self.write(&ascii(&format!("{n} 0 obj\n{body}\nendobj\n")));
    }
    fn stream(&mut self, n: usize, dict: &str, data: &[u8]) {
        self.offsets.insert(n, self.bytes.len());
        let dict = if dict.is_empty() { String::new() } else { format!("{dict} ") };
        self.write(&ascii(&format!(
            "{n} 0 obj\n<< {dict}/Length {} >>\nstream\n",
            data.len()
        )));
        self.write(data);
        self.write(b"\nendstream\nendobj\n");
    }
}

/// Serializes a laid-out report to PDF 1.4 bytes.
pub fn write_pdf(doc: &ReportDocument, options: &PdfOptions) -> Vec<u8> {
    // Objects: 1 catalog, 2 pages, 3-4 fonts, 5 info, images (+ soft mask), fallback fonts
    // (5 each), pages.
    struct Placed {
        name: String,
        image: PdfImage,
        reference: usize,
    }
    let mut images: Vec<Placed> = vec![];
    let mut image_names: HashMap<String, Option<String>> = HashMap::new();
    let mut next = 6;
    for page in &doc.pages {
        for item in &page.items {
            let Item::Image(item) = item else { continue };
            if image_names.contains_key(&item.asset_id) {
                continue;
            }
            let image = match options.decoded_images.get(&item.asset_id) {
                Some(decoded) => Some(decoded_image(decoded)),
                None => options
                    .assets
                    .get(&item.asset_id)
                    .and_then(|a| pdf_image(&a.media_type, &a.data)),
            };
            let Some(image) = image else {
                image_names.insert(item.asset_id.clone(), None);
                continue;
            };
            let name = format!("/Im{}", images.len() + 1);
            image_names.insert(item.asset_id.clone(), Some(name.clone()));
            let reference = next;
            next += if image.smask.is_some() { 2 } else { 1 };
            images.push(Placed { name, image, reference });
        }
    }
    let fonts: Vec<(&PdfFontSubset, usize)> = options
        .fonts
        .iter()
        .enumerate()
        .map(|(i, f)| (f, next + i * 5))
        .collect();
    next += fonts.len() * 5;
    let font_cids: Fonts = fonts.iter().map(|(f, _)| (f.index, cid_map(f))).collect();
    let first_page = next;
    let page_ref = |i: usize| first_page + i * 2;

    let mut out = Out {
        bytes: vec![],
        offsets: HashMap::new(),
    };
    out.write(&ascii("%PDF-1.4\n%\u{e2}\u{e3}\u{cf}\u{d3}\n"));
    out.object(1, "<< /Type /Catalog /Pages 2 0 R >>");
    let kids: Vec<String> = (0..doc.pages.len()).map(|i| format!("{} 0 R", page_ref(i))).collect();
    out.object(
        2,
        &format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), doc.pages.len()),
    );
    out.object(3, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>");
    out.object(
        4,
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>",
    );
    let title = options
        .title
        .as_deref()
        .filter(|t| !t.is_empty())
        .map(|t| format!(" /Title {}", pdf_string(t)))
        .unwrap_or_default();
    out.object(
        5,
        &format!(
            "<<{title} /Producer (ixtable report engine) /CreationDate ({}) >>",
            pdf_date(&options.creation_date)
        ),
    );
    for placed in &images {
        let smask = if placed.image.smask.is_some() {
            format!(" /SMask {} 0 R", placed.reference + 1)
        } else {
            String::new()
        };
        out.stream(
            placed.reference,
            &format!("/Type /XObject /Subtype /Image {}{smask}", placed.image.dict),
            &placed.image.data,
        );
        if let Some((dict, data)) = &placed.image.smask {
            out.stream(
                placed.reference + 1,
                &format!("/Type /XObject /Subtype /Image {dict}"),
                data,
            );
        }
    }
    for (font, reference) in &fonts {
        let file = base64::engine::general_purpose::STANDARD
            .decode(font.data_base64.as_bytes())
            .unwrap_or_default();
        for (i, part) in font_objects(font, *reference, file).into_iter().enumerate() {
            match part {
                FontObject::Stream { dict, data } => out.stream(reference + i, &dict, &data),
                FontObject::Body(body) => out.object(reference + i, &body),
            }
        }
    }
    let xobjects = if images.is_empty() {
        String::new()
    } else {
        let list: Vec<String> = images
            .iter()
            .map(|img| format!("{} {} 0 R", img.name, img.reference))
            .collect();
        format!(" /XObject << {} >>", list.join(" "))
    };
    let font_refs: String = fonts
        .iter()
        .map(|(f, reference)| format!(" {} {reference} 0 R", fallback_font_name(f.index)))
        .collect();
    let image_of = |id: &str| image_names.get(id).cloned().flatten();
    for (i, page) in doc.pages.iter().enumerate() {
        let n = page_ref(i);
        out.object(
            n,
            &format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 3 0 R /F2 4 0 R{font_refs} >>{xobjects} >> /Contents {} 0 R >>",
                num(doc.width),
                num(doc.height),
                n + 1
            ),
        );
        let content: Vec<String> = page
            .items
            .iter()
            .map(|item| item_ops(item, doc.height, &image_of, &font_cids))
            .filter(|s| !s.is_empty())
            .collect();
        out.stream(n + 1, "", &ascii(&content.join("\n")));
    }

    let size = first_page + doc.pages.len() * 2;
    let xref = out.bytes.len();
    let mut table = format!("xref\n0 {size}\n0000000000 65535 f \n");
    for n in 1..size {
        table += &format!("{:010} 00000 n \n", out.offsets.get(&n).copied().unwrap_or(0));
    }
    table += &format!("trailer\n<< /Size {size} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref}\n%%EOF\n");
    out.write(&ascii(&table));
    out.bytes
}

/// Characters of a laid-out report that print in a fallback font, as sorted code points.
pub fn fallback_code_points(doc: &ReportDocument) -> Vec<u32> {
    let mut cps = BTreeSet::new();
    let mut scan = |item: &TextItem| {
        for line in &item.lines {
            for ch in line.text.chars() {
                if fallback_glyph(ch).is_some() {
                    cps.insert(ch as u32);
                }
            }
        }
    };
    for page in &doc.pages {
        for item in &page.items {
            match item {
                Item::Text(t) => scan(t),
                Item::Chart(c) => {
                    for mark in &c.marks {
                        if let Mark::Text(t) = mark {
                            scan(t);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    cps.into_iter().collect()
}

/// PDF bytes of a laid-out report. Asks `report_pdf` for subsets of the bundled fonts and
/// for PNGs the writer can't pass through (alpha, tRNS, interlacing, 16-bit) only when the
/// report needs them. The warnings say which images print as placeholders and why.
pub fn report_pdf_bytes(
    doc: &ReportDocument,
    assets: &HashMap<String, PdfAsset>,
    mut options: PdfOptions,
) -> Result<(Vec<u8>, Vec<String>), AppError> {
    let used: BTreeSet<&str> = doc
        .pages
        .iter()
        .flat_map(|p| p.items.iter())
        .filter_map(|i| match i {
            Item::Image(image) => Some(image.asset_id.as_str()),
            _ => None,
        })
        .collect();
    let mut ids: Vec<&String> = assets.keys().collect();
    ids.sort();
    let pngs: Vec<&String> = ids
        .into_iter()
        .filter(|id| {
            let a = &assets[*id];
            used.contains(id.as_str()) && a.media_type == "image/png" && pdf_image("image/png", &a.data).is_none()
        })
        .collect();
    let code_points = fallback_code_points(doc);
    let (mut fonts, mut images, mut warnings) = (vec![], vec![], vec![]);
    if !code_points.is_empty() || !pngs.is_empty() {
        let encoded: Vec<String> = pngs
            .iter()
            .map(|id| base64::engine::general_purpose::STANDARD.encode(&assets[*id].data))
            .collect();
        let prepared = crate::report_pdf::pdf_resources(&code_points, &encoded)?;
        fonts = prepared.fonts;
        images = prepared.images;
        warnings = prepared.warnings;
    }
    options.assets = assets.clone();
    options.fonts = fonts;
    options.decoded_images = pngs
        .iter()
        .zip(images)
        .filter_map(|(id, image)| image.map(|i| ((*id).clone(), i)))
        .collect();
    Ok((write_pdf(doc, &options), warnings))
}
