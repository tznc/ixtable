// Report PDF resource tests (included from report_pdf.rs).
use super::*;
use std::io::Read as _;

fn inflate(data_base64: &str) -> Vec<u8> {
    let data = base64::engine::general_purpose::STANDARD
        .decode(data_base64)
        .unwrap();
    let mut out = vec![];
    flate2::read::ZlibDecoder::new(&data[..])
        .read_to_end(&mut out)
        .unwrap();
    out
}

fn cps(text: &str) -> Vec<u32> {
    text.chars().map(u32::from).collect()
}

#[test]
fn fallback_fonts_cover_greek_cyrillic_cjk_and_symbols() {
    for ch in ['Ω', 'λ', 'Ж', 'я', '→', '✓', '∑', '≤', 'ő', 'Ł'] {
        assert_eq!(
            glyph_for(ch.into()).map(|g| g.0),
            Some(0),
            "{ch} in DejaVu Sans"
        );
    }
    for ch in ['漢', '字', 'あ', 'カ', '中'] {
        assert_eq!(
            glyph_for(ch.into()).map(|g| g.0),
            Some(1),
            "{ch} in Droid Sans Fallback"
        );
    }
    // Helvetica prints WinAnsi; right-to-left scripts and controls are not drawn.
    for ch in ['A', 'é', '€', '—', '\u{1}', 'א', 'ع'] {
        assert_eq!(glyph_for(ch.into()), None, "{ch:?} has no fallback glyph");
    }
}

#[test]
fn font_subsets_embed_only_used_glyphs_deterministically() {
    let text = cps("Ωμέγα Жук 漢字 → Ω");
    let fonts = font_subsets(&text).unwrap();
    assert_eq!(
        fonts,
        font_subsets(&text).unwrap(),
        "same input, same bytes"
    );
    assert_eq!(
        fonts.iter().map(|f| f.index).collect::<Vec<_>>(),
        vec![0, 1]
    );
    let dejavu = &fonts[0];
    assert!(dejavu.base_font.ends_with("+DejaVuSans"));
    assert_eq!(dejavu.base_font.find('+'), Some(6));
    // Ω appears twice in the input but once in the subset.
    let used: Vec<u32> = dejavu.glyphs.iter().map(|g| g.code_point).collect();
    assert_eq!(used, cps("ΩέαγμЖку→"));
    let cids: Vec<u16> = dejavu.glyphs.iter().map(|g| g.cid).collect();
    assert!(cids.iter().all(|&c| c > 0), "no glyph maps to .notdef");
    let program = inflate(&dejavu.data_base64);
    assert_eq!(program.len(), dejavu.length1);
    let face = ttf_parser::Face::parse(&program, 0).unwrap();
    // .notdef, nine glyphs, and the components of composite glyphs (έ).
    assert!((10..20).contains(&face.number_of_glyphs()));
    assert!(program.len() < 20_000, "subset, not the whole font");
    let cjk = &fonts[1];
    assert_eq!(
        cjk.glyphs.iter().map(|g| g.width).collect::<Vec<_>>(),
        vec![1000, 1000]
    );
    assert!(font_subsets(&cps("plain ASCII")).unwrap().is_empty());
}

#[test]
fn glyph_widths_use_the_font_advance_in_thousandths() {
    let (font, gid) = glyph_for('Ω'.into()).unwrap();
    let face = &faces()[font];
    let advance = face.glyph_hor_advance(ttf_parser::GlyphId(gid)).unwrap();
    assert_eq!(
        glyph_width(font, gid),
        (f64::from(advance) * 1000.0 / 2048.0).round() as i32
    );
    assert_eq!(per_mille(1, 2048), 0);
    assert_eq!(per_mille(-205, 256), -801);
}

fn encode_png(color: png::ColorType, depth: png::BitDepth, w: u32, h: u32, data: &[u8]) -> Vec<u8> {
    encode_png_with(color, depth, w, h, data, |_| {})
}

fn encode_png_with(
    color: png::ColorType,
    depth: png::BitDepth,
    w: u32,
    h: u32,
    data: &[u8],
    setup: impl FnOnce(&mut png::Encoder<&mut Vec<u8>>),
) -> Vec<u8> {
    let mut out = vec![];
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(color);
        enc.set_depth(depth);
        setup(&mut enc);
        let mut writer = enc.write_header().unwrap();
        writer.write_image_data(data).unwrap();
    }
    out
}

#[test]
fn rgba_png_splits_into_color_and_soft_mask() {
    let pixels = [255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 9, 9, 9, 255];
    let bytes = encode_png(png::ColorType::Rgba, png::BitDepth::Eight, 2, 2, &pixels);
    let img = decode_png(&bytes).unwrap();
    assert_eq!((img.width, img.height, img.colors), (2, 2, 3));
    assert_eq!(
        inflate(&img.color_base64),
        vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 9, 9, 9]
    );
    assert_eq!(
        inflate(img.alpha_base64.as_deref().unwrap()),
        vec![255, 128, 0, 255]
    );
    assert_eq!(decode_png(&bytes), Ok(img), "deterministic");
}

#[test]
fn opaque_and_gray_alpha_pngs() {
    let opaque = encode_png(
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        1,
        1,
        &[1, 2, 3, 255],
    );
    let img = decode_png(&opaque).unwrap();
    assert_eq!(img.alpha_base64, None, "no mask when every pixel is opaque");
    assert_eq!(inflate(&img.color_base64), vec![1, 2, 3]);

    let gray = encode_png(
        png::ColorType::GrayscaleAlpha,
        png::BitDepth::Sixteen,
        1,
        1,
        &[0x80, 0x01, 0x40, 0x02],
    );
    let img = decode_png(&gray).unwrap();
    assert_eq!(img.colors, 1);
    assert_eq!(
        inflate(&img.color_base64),
        vec![0x80],
        "16-bit samples keep the high byte"
    );
    assert_eq!(inflate(img.alpha_base64.as_deref().unwrap()), vec![0x40]);
}

#[test]
fn palette_png_with_transparency_expands_to_rgb_and_mask() {
    let bytes = encode_png_with(
        png::ColorType::Indexed,
        png::BitDepth::Eight,
        2,
        1,
        &[0, 1],
        |enc| {
            enc.set_palette(vec![10, 20, 30, 40, 50, 60]);
            enc.set_trns(vec![0]);
        },
    );
    let img = decode_png(&bytes).unwrap();
    assert_eq!(img.colors, 3);
    assert_eq!(inflate(&img.color_base64), vec![10, 20, 30, 40, 50, 60]);
    assert_eq!(inflate(img.alpha_base64.as_deref().unwrap()), vec![0, 255]);
    assert!(decode_png(b"not a png")
        .unwrap_err()
        .starts_with("unreadable PNG"));
}

/// A PNG written chunk by chunk, for headers the encoder won't produce.
fn raw_png(ihdr: [u8; 13], scanlines: &[u8]) -> Vec<u8> {
    let chunk = |out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut crc = flate2::Crc::new();
        crc.update(kind);
        crc.update(data);
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        out.extend_from_slice(&crc.sum().to_be_bytes());
    };
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &deflate(scanlines).unwrap());
    chunk(&mut out, b"IEND", &[]);
    out
}

fn ihdr(width: u32, height: u32, depth: u8, color: u8, interlace: u8) -> [u8; 13] {
    let mut h = [0; 13];
    h[..4].copy_from_slice(&width.to_be_bytes());
    h[4..8].copy_from_slice(&height.to_be_bytes());
    h[8] = depth;
    h[9] = color;
    h[12] = interlace;
    h
}

#[test]
fn huge_declared_png_is_rejected_before_allocating() {
    // 40000×40000 RGBA would need 6.4 GB; the file itself is tiny.
    let bytes = raw_png(ihdr(40_000, 40_000, 8, 6, 0), &[0]);
    assert!(bytes.len() < 100);
    let err = decode_png(&bytes).unwrap_err();
    assert!(err.contains("40000×40000"), "{err}");
    assert!(err.contains("50 megapixel"), "{err}");
    let resources = pdf_resources(&[], &[b64(&bytes)]).unwrap();
    assert_eq!(resources.images, vec![None]);
    assert_eq!(resources.warnings.len(), 1);
    assert!(resources.warnings[0].starts_with("Image 1: PNG is 40000×40000"));
}

#[test]
fn truncated_png_is_rejected() {
    let pixels = [7u8; 4 * 4 * 3];
    let bytes = encode_png(png::ColorType::Rgb, png::BitDepth::Eight, 4, 4, &pixels);
    let err = decode_png(&bytes[..bytes.len() / 2]).unwrap_err();
    assert!(err.starts_with("unreadable PNG"), "{err}");
    let resources = pdf_resources(&[], &["@@not base64".into()]).unwrap();
    assert_eq!(resources.images, vec![None]);
    assert!(resources.warnings[0].starts_with("Image 1: invalid image data"));
}

#[test]
fn sixteen_bit_rgb_png_is_stripped_to_eight_bits() {
    let pixels = [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc];
    let bytes = encode_png(png::ColorType::Rgb, png::BitDepth::Sixteen, 1, 1, &pixels);
    let img = decode_png(&bytes).unwrap();
    assert_eq!((img.colors, img.alpha_base64.clone()), (3, None));
    assert_eq!(inflate(&img.color_base64), vec![0x12, 0x56, 0x9a]);
}

#[test]
fn interlaced_png_is_deinterlaced() {
    // Adam7 on 2×2 RGB: pass 1 holds (0,0), pass 6 holds (1,0), pass 7 holds row 1.
    let scanlines = [
        0, 1, 2, 3, // pass 1
        0, 4, 5, 6, // pass 6
        0, 7, 8, 9, 10, 11, 12, // pass 7
    ];
    let bytes = raw_png(ihdr(2, 2, 8, 2, 1), &scanlines);
    let img = decode_png(&bytes).unwrap();
    assert_eq!((img.width, img.height, img.colors), (2, 2, 3));
    assert_eq!(
        inflate(&img.color_base64),
        vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]
    );
}
