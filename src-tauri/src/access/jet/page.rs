//! Page access for Jet/ACE files: the database header and its fixed mask,
//! protection checks, and table usage maps (`docs/access-format.md` §4.1–4.3).
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

/// Engine version from header byte 0x14.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Version {
    Jet3,
    Jet4,
    /// Access 2007 (0x02), 2010 (0x03), 2016 (0x05), 2019 (0x06).
    Ace(u8),
}

impl Version {
    pub fn jet3(self) -> bool {
        self == Version::Jet3
    }
}

/// The fixed mask over header bytes 0x18.. (the RC4 keystream of the key
/// 0x6B39DAC7, the same table Jackcess and mdbtools hard-code). Jet 3 uses the
/// first 126 bytes.
pub const HEADER_MASK: [u8; 128] = [
    0xB5, 0x6F, 0x03, 0x62, 0x61, 0x08, 0xC2, 0x55, 0xEB, 0xA9, 0x67, 0x72, 0x43, 0x3F, 0x00, 0x9C,
    0x7A, 0x9F, 0x90, 0xFF, 0x80, 0x9A, 0x31, 0xC5, 0x79, 0xBA, 0xED, 0x30, 0xBC, 0xDF, 0xCC, 0x9D,
    0x63, 0xD9, 0xE4, 0xC3, 0x7B, 0x42, 0xFB, 0x8A, 0xBC, 0x4E, 0x86, 0xFB, 0xEC, 0x37, 0x5D, 0x44,
    0x9C, 0xFA, 0xC6, 0x5E, 0x28, 0xE6, 0x13, 0xB6, 0x8A, 0x60, 0x54, 0x94, 0x7B, 0x36, 0xF5, 0x72,
    0xDF, 0xB1, 0x77, 0xF4, 0x13, 0x43, 0xCF, 0xAF, 0xB1, 0x33, 0x34, 0x61, 0x79, 0x5B, 0x92, 0xB5,
    0x7C, 0x2A, 0x05, 0xF1, 0x7C, 0x99, 0x01, 0x1B, 0x98, 0xFD, 0x12, 0x4F, 0x4A, 0x94, 0x6C, 0x3E,
    0x60, 0x26, 0x5F, 0x95, 0xF8, 0xD0, 0x89, 0x24, 0x85, 0x67, 0xC6, 0x1F, 0x27, 0x44, 0xD2, 0xEE,
    0xCF, 0x65, 0xED, 0xFF, 0x07, 0xC7, 0x46, 0xA1, 0x78, 0x16, 0x0C, 0xED, 0xE9, 0x2D, 0x62, 0xD4,
];

/// Header offsets of the database password and the creation date that masks it.
const PASSWORD_AT: usize = 0x42;
const DATE_AT: usize = 0x72;

pub fn u16_at(b: &[u8], at: usize) -> u16 {
    b.get(at..at + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .unwrap_or(0)
}

pub fn u32_at(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .unwrap_or(0)
}

/// A row pointer: row number in the low byte, page number in the upper three.
pub fn row_pointer(v: u32) -> (u32, usize) {
    (v >> 8, (v & 0xFF) as usize)
}

/// Facts from page 0.
#[derive(Debug, Clone)]
pub struct Header {
    pub version: Version,
    pub page_size: usize,
    /// Windows code page of Jet 3 text.
    pub code_page: u16,
    /// Non-zero when the pages are encrypted.
    pub encoding_key: u32,
    /// The database has a password (Jet 3/4 database password).
    pub password: bool,
}

pub fn parse_header(page0: &[u8]) -> Result<Header, String> {
    if page0.len() < 0x100
        || page0[0] != 0
        || &page0[4..19] != b"Standard Jet DB" && &page0[4..19] != b"Standard ACE DB"
    {
        if page0.len() >= 19 && &page0[4..19] == b"MSISAM Database" {
            return Err("Microsoft Money (MSISAM) files are not Access databases".into());
        }
        return Err("not an Access database (no Jet/ACE signature in the header)".into());
    }
    let version = match page0[0x14] {
        0 => Version::Jet3,
        1 => Version::Jet4,
        v @ 2..=6 => Version::Ace(v),
        v => return Err(format!("unknown Access file format code {v}")),
    };
    let mut masked = page0.to_vec();
    let mask_len = if version.jet3() { 126 } else { 128 };
    for (i, k) in HEADER_MASK[..mask_len].iter().enumerate() {
        masked[0x18 + i] ^= k;
    }
    // Jet 4+ also masks the password with the creation date's whole days.
    let (password_len, date_mask) = if version.jet3() {
        (20, [0u8; 4])
    } else {
        let date = f64::from_le_bytes(masked[DATE_AT..DATE_AT + 8].try_into().unwrap_or([0; 8]));
        (40, (date as i32).to_le_bytes())
    };
    let password = masked[PASSWORD_AT..PASSWORD_AT + password_len]
        .iter()
        .enumerate()
        .any(|(i, b)| b ^ date_mask[i % 4] != 0);
    Ok(Header {
        version,
        page_size: if version.jet3() { 2048 } else { 4096 },
        code_page: u16_at(&masked, 0x3C),
        encoding_key: u32_at(&masked, 0x3E),
        password,
    })
}

/// Protected files are out of scope: the user removes the protection in Access first.
pub fn protection_error(header: &Header) -> Option<&'static str> {
    if header.password || (header.encoding_key != 0 && matches!(header.version, Version::Ace(_))) {
        Some("the database has a password; remove it in Access, then import the database")
    } else if header.encoding_key != 0 {
        Some("the database is encrypted; decrypt it in Access, then import the database")
    } else {
        None
    }
}

/// Reads the pages of an unprotected file.
pub struct Pages {
    file: File,
    pub header: Header,
    pub page_count: u32,
}

impl Pages {
    pub fn open(mut file: File) -> Result<Self, String> {
        let mut page0 = vec![0u8; 4096];
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        let n = file.read(&mut page0).map_err(|e| e.to_string())?;
        page0.truncate(n);
        let header = parse_header(&page0)?;
        if let Some(e) = protection_error(&header) {
            return Err(e.into());
        }
        let page_count = (len / header.page_size as u64) as u32;
        Ok(Self {
            file,
            header,
            page_count,
        })
    }

    pub fn page_size(&self) -> usize {
        self.header.page_size
    }

    pub fn read(&mut self, page: u32) -> Result<Vec<u8>, String> {
        if page >= self.page_count {
            return Err(format!("page {page} is past the end of the file"));
        }
        let size = self.header.page_size;
        let mut buf = vec![0u8; size];
        self.file
            .seek(SeekFrom::Start(page as u64 * size as u64))
            .and_then(|_| self.file.read_exact(&mut buf))
            .map_err(|e| format!("page {page}: {e}"))?;
        Ok(buf)
    }

    /// The bytes of row `row` on `page`, with the deleted and overflow flags.
    pub fn row(&mut self, page: u32, row: usize) -> Result<RawRow, String> {
        let buf = self.read(page)?;
        row_on_page(&buf, row, self.header.version)
            .ok_or_else(|| format!("row {row} on page {page} does not exist"))
    }

    /// Pages owned by a table, from the usage map its TDEF points to.
    pub fn usage_map(&mut self, pointer: u32) -> Result<Vec<u32>, String> {
        let (page, row) = row_pointer(pointer);
        let map = self.row(page, row)?.data;
        let mut pages = vec![];
        match map.first() {
            Some(0) => {
                let start = u32_at(&map, 1);
                collect_bits(&map[5..], start, &mut pages);
            }
            Some(1) => {
                let per_page = ((self.page_size() - 4) * 8) as u32;
                for (i, chunk) in map[1..].chunks_exact(4).enumerate() {
                    let map_page = u32_at(chunk, 0);
                    if map_page == 0 {
                        continue;
                    }
                    let buf = self.read(map_page)?;
                    if buf[0] != 0x05 {
                        return Err(format!("usage map page {map_page} has type {}", buf[0]));
                    }
                    collect_bits(&buf[4..], i as u32 * per_page, &mut pages);
                }
            }
            other => return Err(format!("unknown usage map type {other:?}")),
        }
        Ok(pages)
    }
}

fn collect_bits(bitmap: &[u8], start: u32, out: &mut Vec<u32>) {
    for (i, byte) in bitmap.iter().enumerate() {
        for bit in 0..8 {
            if byte & (1 << bit) != 0 {
                out.push(start + (i * 8 + bit) as u32);
            }
        }
    }
}

/// A row slot of a data page.
#[derive(Debug, Clone)]
pub struct RawRow {
    pub data: Vec<u8>,
    pub deleted: bool,
    /// The row holds only a pointer to the row's new place.
    pub overflow: bool,
}

/// Number of rows on a data page.
pub fn rows_on_page(buf: &[u8], version: Version) -> usize {
    u16_at(buf, if version.jet3() { 8 } else { 12 }) as usize
}

/// Slices row `row` out of a data page: rows grow down from the page end.
pub fn row_on_page(buf: &[u8], row: usize, version: Version) -> Option<RawRow> {
    let base = if version.jet3() { 10 } else { 14 };
    if row >= rows_on_page(buf, version) {
        return None;
    }
    let raw = u16_at(buf, base + row * 2);
    let start = (raw & 0x1FFF) as usize;
    let end = if row == 0 {
        buf.len()
    } else {
        (u16_at(buf, base + (row - 1) * 2) & 0x1FFF) as usize
    };
    if start > end || end > buf.len() {
        return None;
    }
    Some(RawRow {
        data: buf[start..end].to_vec(),
        deleted: raw & 0x8000 != 0,
        overflow: raw & 0x4000 != 0,
    })
}
