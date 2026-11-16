//! `folio://` deep links (#89) and quick capture (#87, #93).

use crate::archive::Attachment;
use crate::model::{kind, BlockRecord, NoteFile, NoteMetadata};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum FolioAction {
    OpenNote { id: String, passage: Option<i64> },
    Capture { text: Option<String>, target: Option<String> },
    Search { query: String },
    NewNote { title: Option<String>, text: Option<String> },
    Open,
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn parse_url(url: &str) -> Result<FolioAction, String> {
    let lower = url.to_lowercase();
    let rest = lower.strip_prefix("folio://").map(|_| &url[8..]).ok_or("Das ist kein Folio-Link.")?;
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut params = std::collections::HashMap::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        params.insert(k.to_lowercase(), percent_decode(v));
    }
    if let Some((k, _)) = params.iter().find(|(_, v)| v.chars().count() > 100_000) {
        return Err(format!("„{k}“ ist zu lang (höchstens 100.000 Zeichen)."));
    }
    let mut parts = path.trim_end_matches('/').splitn(2, '/');
    let action = parts.next().unwrap_or("").to_lowercase();
    let arg = parts.next().unwrap_or("");
    let non_empty = |k: &str| params.get(k).filter(|v| !v.is_empty()).cloned();
    match action.as_str() {
        "note" => {
            if arg.is_empty() {
                return Err("Im Link fehlt „Notiz-ID“.".into());
            }
            let id = arg.split('/').next().unwrap_or("").to_uppercase();
            if uuid::Uuid::parse_str(&id).is_err() {
                return Err(format!("„{arg}“ ist keine gültige Notiz-ID."));
            }
            Ok(FolioAction::OpenNote { id, passage: params.get("p").and_then(|p| p.parse().ok()) })
        }
        "capture" => {
            let target = non_empty("target");
            if let Some(t) = &target {
                if t.to_lowercase() != "inbox" && uuid::Uuid::parse_str(t).is_err() {
                    return Err(format!("„{t}“ ist keine gültige Notiz-ID."));
                }
            }
            Ok(FolioAction::Capture { text: non_empty("text"), target })
        }
        "search" => match params.get("q").filter(|q| !q.trim().is_empty()) {
            Some(q) => Ok(FolioAction::Search { query: q.clone() }),
            None => Err("Im Link fehlt „q“.".into()),
        },
        "new" => Ok(FolioAction::NewNote { title: non_empty("title"), text: non_empty("text") }),
        "open" | "" => Ok(FolioAction::Open),
        "wiki" => Ok(FolioAction::Search { query: percent_decode(arg) }),
        other => Err(format!("Folio kennt die Aktion „{other}“ nicht. Möglich sind note, capture, search, new und open.")),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CaptureItem {
    pub id: String,
    pub text: String,
    /// Images as base64 (Swift encodes Data as base64 too).
    #[serde(default)]
    pub images: Vec<String>,
    /// "inbox" or a note id. The Swift app stores an enum; both forms are read.
    #[serde(default = "inbox", deserialize_with = "target_de")]
    pub target: String,
    pub created: f64,
    #[serde(default, rename = "lastError", skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

fn inbox() -> String {
    "inbox".into()
}

fn target_de<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(match v {
        serde_json::Value::String(s) => s,
        serde_json::Value::Object(o) => {
            if o.contains_key("inbox") {
                "inbox".into()
            } else if let Some(n) = o.get("note") {
                n.get("_0").and_then(|x| x.as_str()).unwrap_or("inbox").to_string()
            } else {
                "inbox".into()
            }
        }
        _ => "inbox".into(),
    })
}

pub fn links_in(text: &str) -> Vec<String> {
    static RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| regex::Regex::new(r#"https?://[^\s<>"]+"#).unwrap());
    let mut out: Vec<String> = vec![];
    for m in RE.find_iter(text) {
        let u = m.as_str().trim_end_matches(['.', ',', ')', ';', ':', '!', '?']).to_string();
        if !out.contains(&u) {
            out.push(u);
        }
    }
    out
}

pub const INBOX_NAME: &str = "Inbox";

/// Blocks a capture adds: a dated text block, a web block per link, an image block per image.
pub fn capture_blocks(item: &CaptureItem, date_text: &str, opts: &crate::rtf::WriteOptions) -> (Vec<BlockRecord>, Vec<Attachment>) {
    use base64::Engine;
    let mut blocks = vec![];
    let mut atts = vec![];
    let body = item.text.trim();
    let mut t = BlockRecord::new(kind::TEXT);
    t.set_rtf(&crate::rtf::from_plain(&format!("— {date_text} —{}", if body.is_empty() { String::new() } else { format!("\n{body}") }), opts));
    blocks.push(t);
    for l in links_in(&item.text) {
        let mut b = BlockRecord::new(kind::WEB);
        b.url = Some(l);
        b.mode = Some("preview".into());
        b.height = Some(180.0);
        b.width = Some(1.0);
        blocks.push(b);
    }
    for (i, img) in item.images.iter().enumerate() {
        let Ok(data) = base64::engine::general_purpose::STANDARD.decode(img) else { continue };
        let jpeg = data.starts_with(&[0xFF, 0xD8]);
        let a = Attachment::new(&format!("Erfasst {}.{}", i + 1, if jpeg { "jpg" } else { "png" }), if jpeg { "public.jpeg" } else { "public.png" }, data);
        let mut b = BlockRecord::new(kind::IMAGE);
        b.attachment_id = Some(a.header.id.clone());
        b.width = Some(1.0);
        atts.push(a);
        blocks.push(b);
    }
    (blocks, atts)
}

/// Appends a capture to its target note (the Inbox by default). Never overwrites a change made meanwhile.
pub fn deliver(library: &Path, item: &CaptureItem, target_path: Option<&Path>, date_text: &str, opts: &crate::rtf::WriteOptions) -> Result<PathBuf, String> {
    if !library.is_dir() {
        return Err(format!("Die Bibliothek „{}“ ist gerade nicht erreichbar.", library.display()));
    }
    let target = if item.target == "inbox" {
        library.join(format!("{INBOX_NAME}.folio"))
    } else {
        match target_path {
            Some(p) if p.exists() => p.to_path_buf(),
            _ => return Err("Die Zielnotiz gibt es nicht mehr.".into()),
        }
    };
    let before = crate::model::Fingerprint::of(&target);
    let mut file = if target.exists() {
        NoteFile::read(&target).map_err(|e| format!("Die Zielnotiz ist beschädigt: {e}"))?
    } else {
        let mut meta = NoteMetadata::titled(INBOX_NAME);
        meta.pinned = true;
        let mut h = BlockRecord::new(kind::TEXT);
        h.set_rtf(&crate::rtf::from_plain(INBOX_NAME, opts));
        NoteFile::new(meta, vec![h])
    };
    let (blocks, atts) = capture_blocks(item, date_text, opts);
    file.body.blocks.extend(blocks);
    file.attachments.extend(atts);
    crate::note::refresh_meta(&mut file, true);
    if before.is_some() && crate::model::Fingerprint::of(&target) != before {
        return Err("Die Zielnotiz wurde gleichzeitig woanders geändert.".into());
    }
    file.write(&target).map_err(|e| e.to_string())?;
    Ok(target)
}

/// Captures that could not be stored yet; one JSON file each (#93).
pub struct CaptureQueue {
    pub directory: PathBuf,
}

impl CaptureQueue {
    pub fn add(&self, item: &CaptureItem) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.directory)?;
        crate::model::write_atomic(&self.directory.join(format!("{}.json", item.id)), &serde_json::to_vec(item).unwrap())
    }

    pub fn items(&self) -> Vec<CaptureItem> {
        let mut v: Vec<CaptureItem> = std::fs::read_dir(&self.directory)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
            .filter_map(|e| serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok())
            .collect();
        v.sort_by(|a, b| a.created.partial_cmp(&b.created).unwrap());
        v
    }

    pub fn remove(&self, id: &str) {
        let _ = std::fs::remove_file(self.directory.join(format!("{id}.json")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(parse_url("folio://search?q=hallo%20welt").unwrap(), FolioAction::Search { query: "hallo welt".into() });
        assert!(parse_url("folio://note/xyz").is_err());
        assert!(matches!(parse_url("folio://new?title=A").unwrap(), FolioAction::NewNote { .. }));
        assert!(parse_url("https://x").is_err());
    }
}
