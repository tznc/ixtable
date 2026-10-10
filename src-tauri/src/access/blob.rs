//! Attachment and OLE payload decoding shared by both readers.
use std::io::Read;

/// Splits attachment content into (extension, file bytes).
///
/// The content starts with a header: u32 header length (including itself),
/// u32 constant 1, u32 extension length in characters (with the terminator),
/// then the extension as UTF-16LE with a null terminator. Template sample data
/// carries this content directly (base64); binary files wrap it, see
/// [`unwrap_attachment`].
pub fn split_attachment(content: &[u8]) -> (String, Vec<u8>) {
    let header = read_u32(content, 0) as usize;
    if content.len() < 12 || header < 12 || header > content.len() {
        return (String::new(), content.to_vec());
    }
    let chars = read_u32(content, 8) as usize;
    let ext_end = (12 + chars * 2).min(header);
    let units: Vec<u16> = content[12..ext_end]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|u| *u != 0)
        .collect();
    (String::from_utf16_lossy(&units), content[header..].to_vec())
}

/// Removes the 8 byte wrapper of an attachment stored in an ACE file:
/// u32 flag (0 raw, 1 deflated with a zlib header), u32 inflated length.
pub fn unwrap_attachment(stored: &[u8]) -> Result<Vec<u8>, String> {
    if stored.len() < 8 {
        return Ok(stored.to_vec());
    }
    let flag = read_u32(stored, 0);
    let body = &stored[8..];
    if flag == 0 {
        return Ok(body.to_vec());
    }
    let mut out = Vec::with_capacity(read_u32(stored, 4) as usize);
    flate2::read::ZlibDecoder::new(body)
        .read_to_end(&mut out)
        .map_err(|e| format!("attachment data could not be inflated: {e}"))?;
    Ok(out)
}

/// The payload of an OLE Object field: the embedded file when Access wrapped it
/// in an OLE "Package" or a known picture header, else the bytes unchanged.
pub fn ole_payload(data: &[u8]) -> Vec<u8> {
    // Look for well-known file signatures past the OLE header (which is small).
    const SIGNATURES: [&[u8]; 6] = [
        b"\x89PNG\r\n\x1a\n",
        b"\xFF\xD8\xFF",
        b"GIF8",
        b"%PDF",
        b"BM",
        b"PK\x03\x04",
    ];
    if data.len() > 2 && data[0] == 0x15 && data[1] == 0x1C {
        let limit = data.len().min(4096);
        for sig in SIGNATURES {
            if let Some(i) = data[..limit].windows(sig.len()).position(|w| w == sig) {
                return data[i..].to_vec();
            }
        }
    }
    data.to_vec()
}

/// Guesses a media type from a file extension.
pub fn media_type(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "csv" => "text/csv",
        _ => "application/octet-stream",
    }
}

fn read_u32(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .unwrap_or(0)
}
