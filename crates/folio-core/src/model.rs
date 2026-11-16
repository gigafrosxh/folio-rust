//! Note metadata and blocks, JSON-compatible with the Swift app's `Codable` types.
//!
//! Swift's `JSONEncoder` defaults: dates are `Double` seconds since 2001-01-01 (Apple's reference date),
//! UUIDs are uppercase strings, `Data` is base64, optionals are left out when nil. Fields this version does
//! not know are kept in `extra` and written back unchanged, so newer notes survive a round trip.

use crate::archive::{Archive, Attachment, FormatError};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::path::Path;

/// Seconds between 1970-01-01 and 2001-01-01.
pub const REFERENCE_OFFSET: f64 = 978_307_200.0;

/// Now as Swift `Date` (seconds since 2001).
pub fn now() -> f64 {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    d.as_secs_f64() - REFERENCE_OFFSET
}

pub fn to_unix(swift: f64) -> f64 {
    swift + REFERENCE_OFFSET
}

pub fn from_unix(unix: f64) -> f64 {
    unix - REFERENCE_OFFSET
}

pub fn to_chrono(swift: f64) -> chrono::DateTime<chrono::Local> {
    let unix = to_unix(swift);
    let secs = unix.floor() as i64;
    let nanos = ((unix - secs as f64) * 1e9) as u32;
    chrono::DateTime::from_timestamp(secs, nanos).unwrap_or_default().with_timezone(&chrono::Local)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NoteProperty {
    #[serde(default = "crate::new_id")]
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// text, number, date, status, checkbox
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Text; numbers with "." decimal; dates as yyyy-MM-dd; checkboxes "true"/"false".
    #[serde(default)]
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
}

fn default_kind() -> String {
    "text".into()
}

impl NoteProperty {
    pub fn display_value(&self) -> String {
        match self.kind.as_str() {
            "checkbox" => if self.value == "true" { "Ja".into() } else { "Nein".into() },
            "date" => chrono::NaiveDate::parse_from_str(&self.value, "%Y-%m-%d")
                .map(|d| crate::text::german_date_short(d))
                .unwrap_or_else(|_| self.value.clone()),
            _ => self.value.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NoteMetadata {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub preview: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "now")]
    pub created: f64,
    #[serde(default = "now")]
    pub modified: f64,
    #[serde(default)]
    pub pinned: bool,
    #[serde(rename = "searchText", default)]
    pub search_text: String,
    #[serde(default)]
    pub links: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<Value>,
    #[serde(rename = "wikiLinks", default, skip_serializing_if = "Option::is_none")]
    pub wiki_links: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient_properties")]
    pub properties: Option<Vec<NoteProperty>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Like Swift's `try?` decode: broken properties are dropped instead of failing the note.
fn lenient_properties<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Vec<NoteProperty>>, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(serde_json::from_value(v).ok())
}

impl Default for NoteMetadata {
    fn default() -> Self {
        let t = now();
        NoteMetadata {
            id: crate::new_id(),
            title: String::new(),
            preview: String::new(),
            tags: vec![],
            created: t,
            modified: t,
            pinned: false,
            search_text: String::new(),
            links: vec![],
            kinds: None,
            environment: None,
            wiki_links: None,
            properties: None,
            extra: Map::new(),
        }
    }
}

impl NoteMetadata {
    pub fn titled(title: &str) -> Self {
        NoteMetadata { title: title.to_string(), ..Default::default() }
    }
}

pub mod kind {
    pub const TEXT: &str = "text";
    pub const CODE: &str = "code";
    pub const PDF: &str = "pdf";
    pub const WEB: &str = "web";
    pub const IMAGE: &str = "image";
    pub const FILE: &str = "file";
    pub const DRAWING: &str = "drawing";
    pub const DATABASE: &str = "database";
    pub const MATH: &str = "math";
    pub const DIAGRAM: &str = "diagram";
    pub const AUDIO: &str = "audio";
    pub const EMBED: &str = "embed";
}

/// One block of a note's body. Fields not used by a block's kind stay `None`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct BlockRecord {
    pub id: String,
    pub kind: String,
    /// RTF of text blocks, base64 as Swift encodes `Data`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rtf: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_is_error: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(rename = "attachmentID", default, skip_serializing_if = "Option::is_none")]
    pub attachment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synced_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interactive: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_collapsed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_edit: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf_text: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl BlockRecord {
    pub fn new(kind: &str) -> Self {
        BlockRecord { id: crate::new_id(), kind: kind.to_string(), ..Default::default() }
    }

    pub fn rtf_bytes(&self) -> Option<Vec<u8>> {
        use base64::Engine;
        self.rtf.as_ref().and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok())
    }

    pub fn set_rtf(&mut self, bytes: &[u8]) {
        use base64::Engine;
        self.rtf = Some(base64::engine::general_purpose::STANDARD.encode(bytes));
    }

    /// Plain text of a text block (decoding its RTF).
    pub fn text(&self) -> String {
        if self.kind == kind::TEXT {
            self.rtf_bytes().map(|b| crate::rtf::plain_text(&b)).unwrap_or_default()
        } else {
            self.code.clone().unwrap_or_default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct NoteBody {
    #[serde(default)]
    pub blocks: Vec<BlockRecord>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A fully decoded note file.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteFile {
    pub meta: NoteMetadata,
    pub body: NoteBody,
    pub attachments: Vec<Attachment>,
}

/// JSON with sorted keys, like Swift's `.sortedKeys`, so the same note always gives the same bytes (#98).
pub fn sorted_json<T: Serialize>(value: &T) -> Result<Vec<u8>, FormatError> {
    let v = serde_json::to_value(value)?;
    Ok(serde_json::to_vec(&v)?)
}

impl NoteFile {
    pub fn new(meta: NoteMetadata, blocks: Vec<BlockRecord>) -> Self {
        NoteFile { meta, body: NoteBody { blocks, extra: Map::new() }, attachments: vec![] }
    }

    pub fn encode(&self) -> Result<Vec<u8>, FormatError> {
        Archive { meta: sorted_json(&self.meta)?, body: sorted_json(&self.body)?, attachments: self.attachments.clone() }.encode()
    }

    pub fn decode(data: &[u8]) -> Result<NoteFile, FormatError> {
        let archive = Archive::decode(data, false)?;
        let original_ids: Vec<String> = archive.attachments.iter().map(|a| a.header.id.clone()).collect();
        let attachments = unique_attachments(archive.attachments);
        let mut body: NoteBody = serde_json::from_slice(&archive.body)?;
        // Attachments that got a new id are referenced by no block: a file block keeps each of them (#194).
        let originals: HashSet<&String> = original_ids.iter().collect();
        for att in &attachments {
            if !originals.contains(&att.header.id) {
                let mut b = BlockRecord::new(kind::FILE);
                b.attachment_id = Some(att.header.id.clone());
                b.caption = Some(format!("Wiederhergestellter Anhang: {}", att.header.filename));
                body.blocks.push(b);
            }
        }
        let meta: NoteMetadata = serde_json::from_slice(&archive.meta)?;
        Ok(NoteFile { meta, body, attachments })
    }

    pub fn read(path: &Path) -> Result<NoteFile, FormatError> {
        NoteFile::decode(&std::fs::read(path)?)
    }

    pub fn read_metadata(path: &Path) -> Result<NoteMetadata, FormatError> {
        Ok(serde_json::from_slice(&Archive::read_meta(path)?)?)
    }

    pub fn write(&self, path: &Path) -> Result<Vec<u8>, FormatError> {
        let data = self.encode()?;
        write_atomic(path, &data)?;
        Ok(data)
    }

    pub fn attachment(&self, id: &str) -> Option<&Attachment> {
        self.attachments.iter().find(|a| a.header.id == id)
    }
}

/// Two attachments under one id: the first keeps it, every further one gets a new id (#187).
fn unique_attachments(list: Vec<Attachment>) -> Vec<Attachment> {
    let mut seen = HashSet::new();
    list.into_iter()
        .map(|mut a| {
            if !seen.insert(a.header.id.clone()) {
                a.header.id = crate::new_id();
                seen.insert(a.header.id.clone());
            }
            a
        })
        .collect()
}

/// Writes to a temporary file next to `path`, then renames it over, like Swift's `.atomic`.
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("note");
    let tmp = dir.join(format!(".{}.{}.tmp", name, &crate::new_id()[..8]));
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all().ok();
    }
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// The same note with new ids for itself, its blocks and its attachments (#166).
pub fn renewed(file: &NoteFile) -> NoteFile {
    let mut copy = file.clone();
    copy.meta.id = crate::new_id();
    let mut map = std::collections::HashMap::new();
    for a in copy.attachments.iter_mut() {
        let n = crate::new_id();
        map.insert(a.header.id.clone(), n.clone());
        a.header.id = n;
    }
    for b in copy.body.blocks.iter_mut() {
        b.id = crate::new_id();
        if let Some(old) = &b.attachment_id {
            b.attachment_id = map.get(old).cloned().or(Some(old.clone()));
        }
    }
    copy
}

/// File size and modification time, to notice changes made by other apps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fingerprint {
    pub size: u64,
    pub modified_ns: i128,
}

impl Fingerprint {
    pub fn of(path: &Path) -> Option<Fingerprint> {
        let m = std::fs::metadata(path).ok()?;
        let modified = m.modified().ok()?;
        let ns = match modified.duration_since(std::time::UNIX_EPOCH) {
            Ok(d) => d.as_nanos() as i128,
            Err(e) => -(e.duration().as_nanos() as i128),
        };
        Some(Fingerprint { size: m.len(), modified_ns: ns })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_roundtrip_keeps_unknown_fields() {
        let json = br#"{"id":"A","title":"T","created":1,"modified":2,"futureField":{"x":1}}"#;
        let m: NoteMetadata = serde_json::from_slice(json).unwrap();
        assert_eq!(m.extra.get("futureField").unwrap()["x"], 1);
        let out = String::from_utf8(sorted_json(&m).unwrap()).unwrap();
        assert!(out.contains("futureField"));
        assert!(out.find("\"created\"").unwrap() < out.find("\"id\"").unwrap());
    }

    #[test]
    fn file_roundtrip() {
        let mut b = BlockRecord::new(kind::TEXT);
        b.set_rtf(b"{\\rtf1 Hallo}");
        let mut f = NoteFile::new(NoteMetadata::titled("Hallo"), vec![b]);
        f.attachments.push(Attachment::new("x.bin", "public.data", vec![9; 100]));
        let enc = f.encode().unwrap();
        let d = NoteFile::decode(&enc).unwrap();
        assert_eq!(d.meta.title, "Hallo");
        assert_eq!(d.body.blocks[0].rtf_bytes().unwrap(), b"{\\rtf1 Hallo}");
    }
}
