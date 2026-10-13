//! Image XObjects for the PDF writer without decoding pixels: JPEG passes through as
//! DCTDecode; PNG IDAT data passes through as FlateDecode with the PNG predictor
//! (non-interlaced gray, RGB and palette images, at most 8 bits per sample, as PDF 1.4
//! allows). Other PNGs (alpha, tRNS transparency, interlacing, 16-bit samples) are decoded
//! by `report_pdf` into color samples plus a soft mask.

use crate::report_pdf::PdfPngImage;
use base64::Engine as _;

pub struct PdfImage {
    /// Image dictionary entries after /Type /XObject /Subtype /Image (without /Length).
    pub dict: String,
    pub data: Vec<u8>,
    /// 8-bit alpha samples (FlateDecode), written as the image's /SMask.
    pub smask: Option<(String, Vec<u8>)>,
}

fn unbase64(data: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .unwrap_or_default()
}

/// Image XObject of a decoded PNG, with its alpha as a soft mask.
pub fn decoded_image(png: &PdfPngImage) -> PdfImage {
    let size = format!("/Width {} /Height {}", png.width, png.height);
    let space = if png.colors == 1 { "/DeviceGray" } else { "/DeviceRGB" };
    PdfImage {
        dict: format!("{size} /ColorSpace {space} /BitsPerComponent 8 /Filter /FlateDecode"),
        data: unbase64(&png.color_base64),
        smask: png.alpha_base64.as_deref().map(|alpha| {
            (
                format!("{size} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode"),
                unbase64(alpha),
            )
        }),
    }
}

fn be16(b: &[u8], i: usize) -> usize {
    (usize::from(b[i]) << 8) | usize::from(b[i + 1])
}

fn be32(b: &[u8], i: usize) -> usize {
    (usize::from(b[i]) << 24)
        | (usize::from(b[i + 1]) << 16)
        | (usize::from(b[i + 2]) << 8)
        | usize::from(b[i + 3])
}

/// Reads JPEG dimensions and components from the first SOFn marker.
pub fn jpeg_image(data: &[u8]) -> Option<PdfImage> {
    if data.len() < 2 || data[0] != 0xff || data[1] != 0xd8 {
        return None;
    }
    let mut i = 2;
    while i + 9 < data.len() {
        if data[i] != 0xff {
            return None;
        }
        let marker = data[i + 1];
        if marker == 0xff {
            i += 1;
            continue;
        }
        let length = be16(data, i + 2);
        let is_sof =
            (0xc0..=0xcf).contains(&marker) && marker != 0xc4 && marker != 0xc8 && marker != 0xcc;
        if is_sof {
            let height = be16(data, i + 5);
            let width = be16(data, i + 7);
            let components = data[i + 9];
            let space = match components {
                1 => "/DeviceGray",
                4 => "/DeviceCMYK",
                _ => "/DeviceRGB",
            };
            let decode = if components == 4 { " /Decode [1 0 1 0 1 0 1 0]" } else { "" };
            if width == 0 || height == 0 {
                return None;
            }
            return Some(PdfImage {
                dict: format!(
                    "/Width {width} /Height {height} /ColorSpace {space} /BitsPerComponent 8{decode} /Filter /DCTDecode"
                ),
                data: data.to_vec(),
                smask: None,
            });
        }
        i += 2 + length;
    }
    None
}

const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

/// PNG passthrough for opaque non-interlaced 8-bit-or-less grayscale, RGB and palette images.
pub fn png_image(data: &[u8]) -> Option<PdfImage> {
    if data.len() < 8 || data[..8] != PNG_SIGNATURE {
        return None;
    }
    let mut i = 8;
    let (mut width, mut height, mut depth, mut color_type, mut interlace) =
        (0usize, 0usize, 0u8, -1i32, 0u8);
    let mut palette: Option<&[u8]> = None;
    let mut idat: Vec<&[u8]> = vec![];
    while i + 8 <= data.len() {
        let length = be32(data, i);
        let kind = &data[i + 4..i + 8];
        let end = (i + 8 + length).min(data.len());
        let body = &data[i + 8..end];
        match kind {
            b"IHDR" if body.len() >= 13 => {
                width = be32(body, 0);
                height = be32(body, 4);
                depth = body[8];
                color_type = i32::from(body[9]);
                interlace = body[12];
            }
            b"PLTE" => palette = Some(body),
            b"IDAT" => idat.push(body),
            b"tRNS" => return None,
            b"IEND" => break,
            _ => {}
        }
        i += 12 + length;
    }
    // 16-bit samples need PDF 1.5; `report_pdf` strips them to 8 bits instead.
    if width == 0 || height == 0 || interlace != 0 || depth > 8 || idat.is_empty() {
        return None;
    }
    let (colors, space) = match (color_type, palette) {
        (0, _) => (1, "/DeviceGray".to_string()),
        (2, _) if depth == 8 => (3, "/DeviceRGB".to_string()),
        (3, Some(p)) if p.len() >= 3 => {
            let hex: String = p.iter().map(|b| format!("{b:02x}")).collect();
            (1, format!("[/Indexed /DeviceRGB {} <{hex}>]", p.len() / 3 - 1))
        }
        _ => return None,
    };
    Some(PdfImage {
        dict: format!(
            "/Width {width} /Height {height} /ColorSpace {space} /BitsPerComponent {depth} /Filter /FlateDecode /DecodeParms << /Predictor 15 /Colors {colors} /BitsPerComponent {depth} /Columns {width} >>"
        ),
        data: idat.concat(),
        smask: None,
    })
}

pub fn pdf_image(media_type: &str, data: &[u8]) -> Option<PdfImage> {
    match media_type {
        "image/jpeg" => jpeg_image(data),
        "image/png" => png_image(data),
        _ => None,
    }
}
