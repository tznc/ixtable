//! PDF resources the TS report writer can't build itself
//! (docs/decisions/report-engine.md): subsets of the bundled Unicode fonts for
//! characters outside WinAnsi, and PNG images decoded into a color stream plus
//! an alpha soft mask. Every output is a pure function of its input, so the
//! same report exports the same bytes on every OS.

use crate::manager::AppError;
use base64::Engine as _;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Write as _;
use std::sync::OnceLock;

/// Fallback fonts in lookup order. PDF font n + 3 is `FONTS[n]` (F1 and F2 are Helvetica).
const FONTS: [(&str, &[u8]); 2] = [
    ("DejaVuSans", include_bytes!("../fonts/DejaVuSans.ttf")),
    (
        "DroidSansFallback",
        include_bytes!("../fonts/DroidSansFallbackFull.ttf"),
    ),
];

/// Unicode code points outside Latin-1 that WinAnsiEncoding (Helvetica) covers.
const WIN_ANSI_HIGH: [u32; 27] = [
    0x20ac, 0x201a, 0x0192, 0x201e, 0x2026, 0x2020, 0x2021, 0x02c6, 0x2030, 0x0160, 0x2039, 0x0152,
    0x017d, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014, 0x02dc, 0x2122, 0x0161, 0x203a,
    0x0153, 0x017e, 0x0178,
];

fn faces() -> &'static [ttf_parser::Face<'static>] {
    static FACES: OnceLock<Vec<ttf_parser::Face<'static>>> = OnceLock::new();
    FACES.get_or_init(|| {
        FONTS
            .iter()
            .map(|(_, data)| ttf_parser::Face::parse(data, 0).expect("bundled report font"))
            .collect()
    })
}

/// True for characters Helvetica prints, controls, and scripts that need
/// shaping or right-to-left order (Hebrew, Arabic, Syriac, Thaana, N'Ko and
/// their presentation forms), which the engine can't lay out.
fn excluded(cp: u32) -> bool {
    cp <= 0xff
        || WIN_ANSI_HIGH.contains(&cp)
        || (0x0590..=0x08ff).contains(&cp)
        || (0xfb1d..=0xfdff).contains(&cp)
        || (0xfe70..=0xfeff).contains(&cp)
        || (0xd800..=0xdfff).contains(&cp)
}

/// Fallback font index and glyph id for a character Helvetica can't print.
pub fn glyph_for(cp: u32) -> Option<(usize, u16)> {
    if excluded(cp) {
        return None;
    }
    let ch = char::from_u32(cp)?;
    faces()
        .iter()
        .enumerate()
        .find_map(|(i, face)| face.glyph_index(ch).filter(|g| g.0 != 0).map(|g| (i, g.0)))
}

/// Scales font units to 1/1000 em, rounding half away from zero.
fn per_mille(units: i32, upem: u16) -> i32 {
    let scaled = f64::from(units) * 1000.0 / f64::from(upem);
    scaled.round() as i32
}

/// Advance width of a glyph in 1/1000 em (what the layout engine measures with).
pub fn glyph_width(font: usize, gid: u16) -> i32 {
    let face = &faces()[font];
    let advance = face
        .glyph_hor_advance(ttf_parser::GlyphId(gid))
        .unwrap_or(0);
    per_mille(i32::from(advance), face.units_per_em())
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PdfGlyph {
    pub code_point: u32,
    /// Glyph id in the subset, used as the CID (Identity-H, CIDToGIDMap Identity).
    pub cid: u16,
    /// Advance in 1/1000 em.
    pub width: i32,
}

/// One embedded font subset, everything the writer needs for a Type0/CIDFontType2 font.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PdfFontSubset {
    /// Index into the fallback fonts; the writer names it `/F{index + 3}`.
    pub index: usize,
    /// Subset-tagged PostScript name, e.g. `ABCDEF+DejaVuSans`.
    pub base_font: String,
    /// TrueType subset, zlib-compressed (`/Filter /FlateDecode`).
    pub data_base64: String,
    /// Uncompressed length (`/Length1`).
    pub length1: usize,
    pub bbox: [i32; 4],
    pub ascent: i32,
    pub descent: i32,
    pub cap_height: i32,
    pub glyphs: Vec<PdfGlyph>,
}

/// A decoded PNG: 8-bit color samples and an optional alpha mask, both zlib-compressed.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PdfPngImage {
    pub width: u32,
    pub height: u32,
    /// 1 (DeviceGray) or 3 (DeviceRGB).
    pub colors: u8,
    pub color_base64: String,
    /// Absent when every pixel is opaque.
    pub alpha_base64: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReportPdfResources {
    pub fonts: Vec<PdfFontSubset>,
    /// One entry per input PNG, in order; null when it can't be decoded.
    pub images: Vec<Option<PdfPngImage>>,
    /// Why images print as placeholders (one line per undecodable PNG, by input position).
    pub warnings: Vec<String>,
}

fn deflate(data: &[u8]) -> Result<Vec<u8>, AppError> {
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(data)
        .map_err(|e| AppError::new("IO_ERROR", e))?;
    enc.finish().map_err(|e| AppError::new("IO_ERROR", e))
}

fn b64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

/// Subset tag: six capitals derived from the font and its glyphs, so the same text names the same subset.
fn subset_tag(name: &str, gids: &[u16]) -> String {
    let mut hash = Sha256::new();
    hash.update(name.as_bytes());
    for g in gids {
        hash.update(g.to_be_bytes());
    }
    hash.finalize()[..6]
        .iter()
        .map(|b| char::from(b'A' + b % 26))
        .collect()
}

/// Subsets of the fallback fonts covering `code_points`; characters no font covers are skipped.
pub fn font_subsets(code_points: &[u32]) -> Result<Vec<PdfFontSubset>, AppError> {
    let mut cps = code_points.to_vec();
    cps.sort_unstable();
    cps.dedup();
    let mut out = vec![];
    for (index, (name, data)) in FONTS.iter().enumerate() {
        let used: Vec<(u32, u16)> = cps
            .iter()
            .filter_map(|&cp| match glyph_for(cp) {
                Some((font, gid)) if font == index => Some((cp, gid)),
                _ => None,
            })
            .collect();
        if used.is_empty() {
            continue;
        }
        let gids: Vec<u16> = used.iter().map(|(_, g)| *g).collect();
        let remapper = subsetter::GlyphRemapper::new_from_glyphs_sorted(&gids);
        let font = subsetter::subset(data, 0, &remapper)
            .map_err(|e| AppError::new("INTERNAL_ERROR", format!("font subset failed: {e}")))?;
        let face = &faces()[index];
        let upem = face.units_per_em();
        let s = |v: i16| per_mille(i32::from(v), upem);
        let bb = face.global_bounding_box();
        let mut sorted_gids = gids.clone();
        sorted_gids.sort_unstable();
        sorted_gids.dedup();
        out.push(PdfFontSubset {
            index,
            base_font: format!("{}+{name}", subset_tag(name, &sorted_gids)),
            data_base64: b64(&deflate(&font)?),
            length1: font.len(),
            bbox: [s(bb.x_min), s(bb.y_min), s(bb.x_max), s(bb.y_max)],
            ascent: s(face.ascender()),
            descent: s(face.descender()),
            cap_height: s(face.capital_height().unwrap_or(face.ascender())),
            glyphs: used
                .iter()
                .map(|&(code_point, gid)| PdfGlyph {
                    code_point,
                    cid: remapper.get(gid).unwrap_or(0),
                    width: glyph_width(index, gid),
                })
                .collect(),
        });
    }
    Ok(out)
}

/// Largest PNG the writer decodes. A header can declare any size, so the
/// output buffer is never allocated before this check.
pub const MAX_PNG_PIXELS: u64 = 50_000_000;
/// Allocation budget for the PNG decoder's own buffers.
const PNG_DECODER_BYTES: usize = 64 << 20;

/// Decodes any PNG (palette, low bit depth, 16-bit, interlaced, tRNS) to
/// 8-bit gray or RGB samples plus an alpha mask when any pixel is translucent.
/// Errors say why the image prints as a placeholder instead.
pub fn decode_png(bytes: &[u8]) -> Result<PdfPngImage, String> {
    let mut decoder = png::Decoder::new_with_limits(
        std::io::Cursor::new(bytes),
        png::Limits {
            bytes: PNG_DECODER_BYTES,
        },
    );
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("unreadable PNG: {e}"))?;
    let (width, height) = (reader.info().width, reader.info().height);
    if u64::from(width) * u64::from(height) > MAX_PNG_PIXELS {
        return Err(format!(
            "PNG is {width}×{height}, larger than the {} megapixel limit",
            MAX_PNG_PIXELS / 1_000_000
        ));
    }
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| "PNG is too large to decode".to_string())?;
    let mut buf = vec![0; size];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("unreadable PNG: {e}"))?;
    let (colors, has_alpha) = match info.color_type {
        png::ColorType::Grayscale => (1, false),
        png::ColorType::GrayscaleAlpha => (1, true),
        png::ColorType::Rgb => (3, false),
        png::ColorType::Rgba => (3, true),
        png::ColorType::Indexed => return Err("unsupported PNG color type".into()),
    };
    let channels = colors + usize::from(has_alpha);
    let pixels = info.width as usize * info.height as usize;
    let samples = &buf[..pixels * channels];
    let (color, alpha) = if has_alpha {
        let mut color = Vec::with_capacity(pixels * colors);
        let mut alpha = Vec::with_capacity(pixels);
        for px in samples.chunks_exact(channels) {
            color.extend_from_slice(&px[..colors]);
            alpha.push(px[colors]);
        }
        let translucent = alpha.iter().any(|&a| a != 255);
        (color, translucent.then_some(alpha))
    } else {
        (samples.to_vec(), None)
    };
    let compress = |data: &[u8]| deflate(data).map(|d| b64(&d)).map_err(|e| e.message);
    Ok(PdfPngImage {
        width: info.width,
        height: info.height,
        colors: colors as u8,
        color_base64: compress(&color)?,
        alpha_base64: alpha.as_deref().map(compress).transpose()?,
    })
}

/// Font subsets and decoded PNGs for one export (see `prepare_report_pdf`).
pub fn pdf_resources(
    code_points: &[u32],
    pngs_base64: &[String],
) -> Result<ReportPdfResources, AppError> {
    let mut warnings = vec![];
    let images = pngs_base64
        .iter()
        .enumerate()
        .map(|(i, data)| {
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(data.as_bytes())
                .map_err(|e| format!("invalid image data: {e}"))
                .and_then(|bytes| decode_png(&bytes));
            decoded
                .map_err(|reason| {
                    crate::logging::warn("reports", &format!("PDF image {}: {reason}", i + 1));
                    warnings.push(format!("Image {}: {reason}", i + 1));
                })
                .ok()
        })
        .collect();
    Ok(ReportPdfResources {
        fonts: font_subsets(code_points)?,
        images,
        warnings,
    })
}

/// Font subsets for the characters Helvetica can't print and decoded PNGs
/// (base64) that the writer can't pass through (alpha, tRNS, interlacing,
/// 16-bit). Runs on a worker thread: subsetting and deflating take a while.
pub fn prepare_report_pdf(
    window_label: String,
    code_points: Vec<u32>,
    pngs_base64: Vec<String>,
) -> Result<ReportPdfResources, AppError> {
    crate::manager()?.state(&window_label)?;
    pdf_resources(&code_points, &pngs_base64)
}

#[cfg(test)]
mod tests {
    include!("report_pdf_tests.rs");
}
