//! Images in rich text (`docs/decisions/field-formats.md`): web images, and files of the
//! record's attachment fields written `attachment:<file name>` (or the file's id). The editor
//! uploads a pasted or picked image to the form's attachment field and links it by name.

use super::field_values::AttachmentRef;
use super::richtext::{ATTACHMENT_SCHEME, Token, sanitize_rich_text, tokens};
use std::io::Read;
use std::time::Duration;

/// Largest web image the viewer downloads (decimal megabytes).
const MAX_WEB_IMAGE_BYTES: u64 = 10_000_000;

/// The `src` of every image in a rich-text value, in order, without repeats.
pub fn image_sources(html: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for token in tokens(&sanitize_rich_text(html)) {
        if let Token::Open { name, attrs, .. } = token {
            if let Some(src) = Token::attr(&attrs, "src").filter(|_| name == "img") {
                if !out.iter().any(|s| s == src) {
                    out.push(src.to_string());
                }
            }
        }
    }
    out
}

/// The image `src` for an attachment file name; spaces and brackets are percent-encoded so the
/// Markdown link needs no angle brackets.
pub fn attachment_src(name: &str) -> String {
    let mut out = String::from(ATTACHMENT_SCHEME);
    for c in name.chars() {
        match c {
            ' ' | '%' | '(' | ')' | '<' | '>' | '"' | '[' | ']' => out.push_str(&format!("%{:02X}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The record file an `attachment:` source names, by id or by file name.
pub fn attachment_for<'a>(src: &str, files: &'a [AttachmentRef]) -> Option<&'a AttachmentRef> {
    let reference = percent_decode(src.strip_prefix(ATTACHMENT_SCHEME)?);
    files
        .iter()
        .find(|f| f.id == reference)
        .or_else(|| files.iter().find(|f| f.name == reference))
        .or_else(|| files.iter().find(|f| f.name.eq_ignore_ascii_case(&reference)))
}

/// The image type of `bytes`, from their first bytes.
pub fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    let starts = |magic: &[u8]| bytes.starts_with(magic);
    if starts(b"\x89PNG") {
        Some("image/png")
    } else if starts(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if starts(b"GIF8") {
        Some("image/gif")
    } else if starts(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else if starts(b"BM") {
        Some("image/bmp")
    } else if starts(b"II*\0") || starts(b"MM\0*") {
        Some("image/tiff")
    } else {
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_ascii_lowercase();
        head.contains("<svg").then_some("image/svg+xml")
    }
}

/// The file extension for an image type.
pub fn image_extension(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        "image/svg+xml" => "svg",
        _ => "png",
    }
}

/// A file name for a pasted image that no file of the record already uses.
pub fn pasted_image_name(files: &[AttachmentRef], extension: &str, now: chrono::NaiveDateTime) -> String {
    let stem = format!("pasted-image-{}", now.format("%Y%m%d-%H%M%S"));
    let mut name = format!("{stem}.{extension}");
    let mut n = 2;
    while files.iter().any(|f| f.name == name) {
        name = format!("{stem}-{n}.{extension}");
        n += 1;
    }
    name
}

/// Downloads a web image for display, refusing anything over the size limit or not an image.
pub fn fetch_web_image(url: &str) -> Result<Vec<u8>, String> {
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return Err("Only web images can be downloaded".into());
    }
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("ixtable-desktop/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client.get(url).send().and_then(|r| r.error_for_status()).map_err(|e| e.to_string())?;
    if response.content_length().is_some_and(|n| n > MAX_WEB_IMAGE_BYTES) {
        return Err("The image is too large to show".into());
    }
    let mut bytes = vec![];
    response
        .take(MAX_WEB_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_WEB_IMAGE_BYTES {
        return Err("The image is too large to show".into());
    }
    if image_mime(&bytes).is_none() {
        return Err("The address is not an image".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: &str, name: &str) -> AttachmentRef {
        AttachmentRef { id: id.into(), name: name.into(), mime: "image/png".into(), size: 1, sha256: String::new() }
    }

    #[test]
    fn finds_attachment_images_by_name_or_id() {
        let files = [file("f1", "my photo.png"), file("f2", "Chart.PNG")];
        assert_eq!(attachment_src("my photo.png"), "attachment:my%20photo.png");
        assert_eq!(attachment_for("attachment:my%20photo.png", &files).map(|f| f.id.as_str()), Some("f1"));
        assert_eq!(attachment_for("attachment:f2", &files).map(|f| f.id.as_str()), Some("f2"));
        assert_eq!(attachment_for("attachment:chart.png", &files).map(|f| f.id.as_str()), Some("f2"));
        assert!(attachment_for("attachment:missing.png", &files).is_none());
        assert!(attachment_for("https://x.io/a.png", &files).is_none());
        assert_eq!(percent_decode("a%2"), "a%2");
    }

    #[test]
    fn lists_image_sources_once() {
        let html = "<p><img src=\"https://x.io/a.png\"><img src=\"attachment:b.png\"><img src=\"https://x.io/a.png\"><img src=\"file:///c\"></p>";
        assert_eq!(image_sources(html), ["https://x.io/a.png", "attachment:b.png"]);
    }

    #[test]
    fn names_pasted_images_uniquely() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 10, 20).unwrap().and_hms_opt(9, 30, 5).unwrap();
        assert_eq!(pasted_image_name(&[], "png", now), "pasted-image-20261020-093005.png");
        let taken = [file("a", "pasted-image-20261020-093005.png")];
        assert_eq!(pasted_image_name(&taken, "png", now), "pasted-image-20261020-093005-2.png");
    }

    #[test]
    fn recognises_image_bytes() {
        assert_eq!(image_mime(b"\x89PNG\r\n"), Some("image/png"));
        assert_eq!(image_mime(&[0xff, 0xd8, 0xff, 0xe0]), Some("image/jpeg"));
        assert_eq!(image_mime(b"RIFF\0\0\0\0WEBPVP8"), Some("image/webp"));
        assert_eq!(image_mime(b"<?xml?><svg/>"), Some("image/svg+xml"));
        assert_eq!(image_mime(b"hello"), None);
        assert_eq!(image_extension("image/jpeg"), "jpg");
        assert!(fetch_web_image("file:///etc/passwd").is_err());
    }
}
