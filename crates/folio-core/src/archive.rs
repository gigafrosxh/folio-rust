//! The `.folio` container: a small binary envelope around LZFSE-compressed sections.
//!
//! Byte-compatible with the Swift implementation (`FolioArchive.swift`):
//!
//! ```text
//! magic        "FOLI"            4 bytes
//! version      u16               currently 1
//! flags        u16               bit 0 = LZFSE compression
//! meta         section           note metadata JSON
//! body         section           block JSON
//! count        u32               number of attachments
//! attachments  count × (u32 header length, header JSON, section)
//! checksum     u32               CRC-32 over everything before it
//! section  =   u64 raw length, u64 stored length, stored bytes
//! ```
//! All integers little endian. LZFSE comes from macOS' own libcompression, so the
//! compressed bytes are the same the Swift app writes.

use serde::{Deserialize, Serialize};
use std::fmt;

pub const MAGIC: &[u8; 4] = b"FOLI";
pub const CURRENT_VERSION: u16 = 1;
pub const FILE_EXTENSION: &str = "folio";

/// Limits for reading untrusted files (#161): a small file must not claim gigabytes.
pub mod limits {
    pub const MAX_SECTION: u64 = 1 << 30;
    pub const RATIO_CHECKED_ABOVE: u64 = 128 << 20;
    pub const MAX_RATIO: u64 = 1_000;
    pub const MAX_TOTAL: u64 = 4 << 30;
    pub const MAX_ATTACHMENTS: u32 = 100_000;

    pub fn allows(raw: u64, stored: u64) -> bool {
        raw <= MAX_SECTION && (raw <= RATIO_CHECKED_ABOVE || raw / MAX_RATIO <= stored)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    BadMagic,
    UnsupportedVersion(u16),
    Truncated,
    ChecksumMismatch,
    CompressionFailed,
    TooLarge,
    Json(String),
    Io(String),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::TooLarge => write!(f, "Die Datei gibt unglaubwürdig große Inhalte an und wird nicht geöffnet."),
            FormatError::BadMagic => write!(f, "Keine Folio-Notiz (falsche Dateikennung)."),
            FormatError::UnsupportedVersion(v) => write!(f, "Dateiversion {v} wird nicht unterstützt."),
            FormatError::Truncated => write!(f, "Die Datei ist unvollständig."),
            FormatError::ChecksumMismatch => write!(f, "Die Prüfsumme stimmt nicht – die Datei ist beschädigt."),
            FormatError::CompressionFailed => write!(f, "Komprimierung fehlgeschlagen."),
            FormatError::Json(m) => write!(f, "Inhalt nicht lesbar: {m}"),
            FormatError::Io(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for FormatError {}

impl From<std::io::Error> for FormatError {
    fn from(e: std::io::Error) -> Self {
        FormatError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for FormatError {
    fn from(e: serde_json::Error) -> Self {
        FormatError::Json(e.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AttachmentHeader {
    pub id: String,
    pub filename: String,
    #[serde(rename = "typeIdentifier")]
    pub type_identifier: String,
    /// `true` when `data` is a serialized `NSFileWrapper` (packages such as .numbers).
    #[serde(rename = "isPackage", default)]
    pub is_package: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    pub header: AttachmentHeader,
    pub data: Vec<u8>,
}

impl Attachment {
    pub fn new(filename: &str, type_identifier: &str, data: Vec<u8>) -> Self {
        Attachment {
            header: AttachmentHeader {
                id: crate::new_id(),
                filename: filename.to_string(),
                type_identifier: type_identifier.to_string(),
                is_package: false,
            },
            data,
        }
    }
    pub fn id(&self) -> &str {
        &self.header.id
    }
}

#[derive(Debug, Clone, Default)]
pub struct Archive {
    pub meta: Vec<u8>,
    pub body: Vec<u8>,
    pub attachments: Vec<Attachment>,
}

// MARK: - LZFSE via libcompression

mod lzfse {
    use std::os::raw::c_void;

    const COMPRESSION_LZFSE: i32 = 0x801;

    #[link(name = "compression")]
    extern "C" {
        fn compression_encode_buffer(dst: *mut u8, dst_size: usize, src: *const u8, src_size: usize, scratch: *mut c_void, alg: i32) -> usize;
        fn compression_decode_buffer(dst: *mut u8, dst_size: usize, src: *const u8, src_size: usize, scratch: *mut c_void, alg: i32) -> usize;
    }

    pub fn compress(data: &[u8]) -> Option<Vec<u8>> {
        if data.is_empty() {
            return Some(Vec::new());
        }
        let mut cap = data.len() + data.len() / 8 + 4096;
        for _ in 0..4 {
            let mut out = vec![0u8; cap];
            let n = unsafe { compression_encode_buffer(out.as_mut_ptr(), cap, data.as_ptr(), data.len(), std::ptr::null_mut(), COMPRESSION_LZFSE) };
            if n > 0 {
                out.truncate(n);
                return Some(out);
            }
            cap *= 2;
        }
        None
    }

    /// Unpacks into a buffer of exactly the size the file states (+1 to notice files that hold more).
    pub fn decompress(data: &[u8], size: usize) -> Option<Vec<u8>> {
        if data.is_empty() || size == 0 {
            return Some(Vec::new());
        }
        let mut out = vec![0u8; size + 1];
        let n = unsafe { compression_decode_buffer(out.as_mut_ptr(), size + 1, data.as_ptr(), data.len(), std::ptr::null_mut(), COMPRESSION_LZFSE) };
        if n != size {
            return None;
        }
        out.truncate(size);
        Some(out)
    }
}

pub fn crc32(data: &[u8]) -> u32 {
    static TABLE: once_cell::sync::Lazy<[u32; 256]> = once_cell::sync::Lazy::new(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    });
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = TABLE[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

struct Writer {
    data: Vec<u8>,
}

impl Writer {
    fn section(&mut self, raw: &[u8]) -> Result<(), FormatError> {
        let stored = lzfse::compress(raw).ok_or(FormatError::CompressionFailed)?;
        self.data.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        self.data.extend_from_slice(&(stored.len() as u64).to_le_bytes());
        self.data.extend_from_slice(&stored);
        Ok(())
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
    unpacked: u64,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], FormatError> {
        let end = self.offset.checked_add(n).ok_or(FormatError::Truncated)?;
        if end > self.bytes.len() {
            return Err(FormatError::Truncated);
        }
        let s = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16, FormatError> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, FormatError> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, FormatError> {
        Ok(u64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }
    fn section(&mut self, compressed: bool) -> Result<Vec<u8>, FormatError> {
        let raw_len = self.u64()?;
        let stored_len = self.u64()?;
        if stored_len > usize::MAX as u64 {
            return Err(FormatError::Truncated);
        }
        if (compressed && !limits::allows(raw_len, stored_len)) || raw_len > limits::MAX_TOTAL - self.unpacked {
            return Err(FormatError::TooLarge);
        }
        self.unpacked += raw_len;
        let stored = self.bytes(stored_len as usize)?;
        let raw = if compressed {
            lzfse::decompress(stored, raw_len as usize).ok_or(FormatError::CompressionFailed)?
        } else {
            stored.to_vec()
        };
        if raw.len() as u64 != raw_len {
            return Err(FormatError::Truncated);
        }
        Ok(raw)
    }
}

impl Archive {
    pub fn encode(&self) -> Result<Vec<u8>, FormatError> {
        let mut w = Writer { data: Vec::with_capacity(self.meta.len() + self.body.len() + 64) };
        w.data.extend_from_slice(MAGIC);
        w.data.extend_from_slice(&CURRENT_VERSION.to_le_bytes());
        w.data.extend_from_slice(&1u16.to_le_bytes());
        w.section(&self.meta)?;
        w.section(&self.body)?;
        w.data.extend_from_slice(&(self.attachments.len() as u32).to_le_bytes());
        for a in &self.attachments {
            let header = serde_json::to_vec(&a.header)?;
            w.data.extend_from_slice(&(header.len() as u32).to_le_bytes());
            w.data.extend_from_slice(&header);
            w.section(&a.data)?;
        }
        let crc = crc32(&w.data);
        w.data.extend_from_slice(&crc.to_le_bytes());
        Ok(w.data)
    }

    pub fn decode(data: &[u8], meta_only: bool) -> Result<Archive, FormatError> {
        let mut r = Reader { bytes: data, offset: 0, unpacked: 0 };
        if r.bytes(4)? != MAGIC {
            return Err(FormatError::BadMagic);
        }
        let version = r.u16()?;
        if version > CURRENT_VERSION {
            return Err(FormatError::UnsupportedVersion(version));
        }
        let flags = r.u16()?;
        let compressed = flags & 1 == 1;
        let meta = r.section(compressed)?;
        if meta_only {
            return Ok(Archive { meta, ..Default::default() });
        }
        if data.len() < 4 {
            return Err(FormatError::Truncated);
        }
        let (payload, tail) = data.split_at(data.len() - 4);
        if crc32(payload) != u32::from_le_bytes(tail.try_into().unwrap()) {
            return Err(FormatError::ChecksumMismatch);
        }
        let body = r.section(compressed)?;
        let count = r.u32()?;
        if count > limits::MAX_ATTACHMENTS {
            return Err(FormatError::TooLarge);
        }
        let mut attachments = Vec::with_capacity(count.min(1024) as usize);
        for _ in 0..count {
            let hl = r.u32()? as usize;
            let header: AttachmentHeader = serde_json::from_slice(r.bytes(hl)?)?;
            let content = r.section(compressed)?;
            attachments.push(Attachment { header, data: content });
        }
        Ok(Archive { meta, body, attachments })
    }

    /// Reads only the metadata section, memory-mapping the file so the body is never touched.
    pub fn read_meta(path: &std::path::Path) -> Result<Vec<u8>, FormatError> {
        let file = std::fs::File::open(path)?;
        let len = file.metadata()?.len();
        if len < 8 {
            return Err(FormatError::Truncated);
        }
        let map = unsafe { memmap2::Mmap::map(&file)? };
        Ok(Archive::decode(&map, true)?.meta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let a = Archive {
            meta: br#"{"id":"X"}"#.to_vec(),
            body: vec![b'a'; 10_000],
            attachments: vec![Attachment::new("a.png", "public.png", vec![1, 2, 3])],
        };
        let enc = a.encode().unwrap();
        let d = Archive::decode(&enc, false).unwrap();
        assert_eq!(d.meta, a.meta);
        assert_eq!(d.body, a.body);
        assert_eq!(d.attachments, a.attachments);
        let mut bad = enc.clone();
        let n = bad.len() / 2;
        bad[n] ^= 0xFF;
        assert!(Archive::decode(&bad, false).is_err());
    }

    #[test]
    fn crc_known() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
