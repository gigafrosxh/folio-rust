//! The library on disk: scanning, organization (order, favorites, smart views), version history,
//! trash and backups. Folio's own data lives in `<library>/.folio/`, compatible with the Swift app.

use crate::archive::FILE_EXTENSION;
use crate::model::{self, NoteFile, NoteMetadata};
use crate::search::{manual_order, SearchFilter, SortMode};
use parking_lot::Mutex;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const DATA_FOLDER: &str = ".folio";

pub fn data_folder(library: &Path, name: &str) -> PathBuf {
    library.join(DATA_FOLDER).join(name)
}

pub fn is_note(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some(FILE_EXTENSION)
}

/// `dir/base.folio`, or `dir/base 2.folio` … when taken.
pub fn unique_path(dir: &Path, base: &str, ext: &str) -> PathBuf {
    let make = |n: usize| {
        let name = if n == 1 { base.to_string() } else { format!("{base} {n}") };
        if ext.is_empty() { dir.join(name) } else { dir.join(format!("{name}.{ext}")) }
    };
    let mut n = 1;
    loop {
        let p = make(n);
        if !p.exists() {
            return p;
        }
        n += 1;
    }
}

/// Same as `unique_path` but keeps an existing extension of `path`.
pub fn unique_like(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let dir = path.parent().unwrap_or(Path::new("."));
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let base = if ext.is_empty() {
        path.file_name().and_then(|n| n.to_str()).unwrap_or("Datei").to_string()
    } else {
        path.file_stem().and_then(|n| n.to_str()).unwrap_or("Datei").to_string()
    };
    unique_path(dir, &base, ext)
}

// MARK: - Stamps and retention

/// Sortable, file-name-safe UTC timestamps ("20261020T121314123Z").
pub mod stamp {
    use chrono::{TimeZone, Utc};

    pub fn string(swift_date: f64) -> String {
        let unix = crate::model::to_unix(swift_date);
        let ms = (unix * 1000.0).round() as i64;
        let dt = Utc.timestamp_millis_opt(ms).single().unwrap_or_default();
        dt.format("%Y%m%dT%H%M%S%3fZ").to_string()
    }

    pub fn date(s: &str) -> Option<f64> {
        let s: String = s.chars().take(19).collect();
        if s.len() < 19 {
            return None;
        }
        let dt = chrono::NaiveDateTime::parse_from_str(&s[..15], "%Y%m%dT%H%M%S").ok()?;
        let ms: i64 = s[15..18].parse().ok()?;
        let unix = Utc.from_utc_datetime(&dt).timestamp() as f64 + ms as f64 / 1000.0;
        Some(crate::model::from_unix(unix))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RetentionPolicy {
    pub keep_all: f64,
    pub hourly: f64,
    pub daily: f64,
    pub max_age: f64,
    pub max_count: usize,
}

const DAY: f64 = 86_400.0;

impl RetentionPolicy {
    pub const VERSIONS: RetentionPolicy = RetentionPolicy { keep_all: DAY, hourly: 7.0 * DAY, daily: 90.0 * DAY, max_age: 365.0 * DAY, max_count: 300 };
    pub const BACKUPS: RetentionPolicy = RetentionPolicy { keep_all: 2.0 * DAY, hourly: 2.0 * DAY, daily: 14.0 * DAY, max_age: 90.0 * DAY, max_count: 30 };

    /// The dates to delete; the newest copy is never deleted.
    pub fn to_delete(&self, dates: &[f64], now: f64) -> Vec<f64> {
        use chrono::Datelike;
        use chrono::Timelike;
        let mut sorted = dates.to_vec();
        sorted.sort_by(|a, b| b.partial_cmp(a).unwrap());
        let Some(&newest) = sorted.first() else { return vec![] };
        let mut keep: Vec<f64> = vec![newest];
        let mut buckets = HashSet::new();
        for &d in &sorted {
            let age = now - d;
            let utc = chrono::DateTime::from_timestamp(model::to_unix(d) as i64, 0).unwrap_or_default();
            let bucket = if age < self.keep_all {
                keep.push(d);
                continue;
            } else if age < self.hourly {
                format!("h{}-{}-{}-{}", utc.year(), utc.month(), utc.day(), utc.hour())
            } else if age < self.daily {
                format!("d{}-{}-{}", utc.year(), utc.month(), utc.day())
            } else if age < self.max_age {
                let w = utc.iso_week();
                format!("w{}-{}", w.year(), w.week())
            } else {
                continue;
            };
            if buckets.insert(bucket) {
                keep.push(d);
            }
        }
        let mut kept: Vec<f64> = sorted.iter().copied().filter(|d| keep.contains(d)).collect();
        kept.truncate(self.max_count);
        sorted.into_iter().filter(|d| !kept.contains(d)).collect()
    }
}

// MARK: - Version history

#[derive(Debug, Clone, Serialize)]
pub struct NoteVersion {
    pub path: PathBuf,
    pub date: f64,
    pub size: u64,
}

pub struct NoteHistory {
    pub directory: PathBuf,
    pub policy: RetentionPolicy,
    /// Minimum seconds between two automatic copies while editing.
    pub interval: f64,
}

impl NoteHistory {
    pub fn new(library: &Path) -> Self {
        NoteHistory { directory: data_folder(library, "history"), policy: RetentionPolicy::VERSIONS, interval: 300.0 }
    }

    fn folder(&self, id: &str) -> PathBuf {
        self.directory.join(id)
    }

    pub fn versions(&self, id: &str) -> Vec<NoteVersion> {
        let mut out: Vec<NoteVersion> = std::fs::read_dir(self.folder(id))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                if !is_note(&p) {
                    return None;
                }
                let date = stamp::date(p.file_name()?.to_str()?)?;
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                Some(NoteVersion { path: p, date, size })
            })
            .collect();
        out.sort_by(|a, b| b.date.partial_cmp(&a.date).unwrap());
        out
    }

    /// Stores an encoded note as a new version unless the newest is younger than `interval`
    /// (ignored with `force`) or has the same content.
    pub fn record(&self, data: &[u8], id: &str, date: f64, force: bool) -> std::io::Result<Option<NoteVersion>> {
        let existing = self.versions(id);
        if let Some(latest) = existing.first() {
            if !force && date - latest.date < self.interval {
                return Ok(None);
            }
            if let Ok(old) = std::fs::read(&latest.path) {
                if same_content(&old, data) {
                    return Ok(None);
                }
            }
        }
        let dir = self.folder(id);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.{FILE_EXTENSION}", stamp::string(date)));
        model::write_atomic(&path, data)?;
        self.prune(id, date);
        Ok(Some(NoteVersion { path, date, size: data.len() as u64 }))
    }

    pub fn prune(&self, id: &str, now: f64) -> usize {
        let all = self.versions(id);
        let doomed = self.policy.to_delete(&all.iter().map(|v| v.date).collect::<Vec<_>>(), now);
        all.iter().filter(|v| doomed.contains(&v.date)).filter(|v| std::fs::remove_file(&v.path).is_ok()).count()
    }

    pub fn remove_all(&self, id: &str) {
        let _ = std::fs::remove_dir_all(self.folder(id));
    }
}

/// Same body, attachments and properties (metadata always differs by its modification date).
pub fn same_content(a: &[u8], b: &[u8]) -> bool {
    let (Ok(x), Ok(y)) = (crate::archive::Archive::decode(a, false), crate::archive::Archive::decode(b, false)) else { return false };
    if x.attachments.iter().map(|a| &a.data).ne(y.attachments.iter().map(|a| &a.data)) {
        return false;
    }
    let px = serde_json::from_slice::<NoteMetadata>(&x.meta).ok().and_then(|m| m.properties);
    let py = serde_json::from_slice::<NoteMetadata>(&y.meta).ok().and_then(|m| m.properties);
    if px != py {
        return false;
    }
    if x.body == y.body {
        return true;
    }
    let canon = |b: &[u8]| serde_json::from_slice::<serde_json::Value>(b).ok();
    canon(&x.body).is_some() && canon(&x.body) == canon(&y.body)
}

// MARK: - Trash

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TrashItem {
    pub id: String,
    pub original_path: String,
    /// ISO 8601 in info.json, like the Swift app.
    pub deleted_at: String,
    pub is_folder: bool,
    pub title: String,
    pub note_count: usize,
    #[serde(rename = "noteIDs")]
    pub note_ids: Vec<String>,
}

impl TrashItem {
    pub fn name(&self) -> String {
        Path::new(&self.original_path).file_name().and_then(|n| n.to_str()).unwrap_or("").to_string()
    }
    pub fn deleted_swift(&self) -> f64 {
        chrono::DateTime::parse_from_rfc3339(&self.deleted_at).map(|d| model::from_unix(d.timestamp() as f64)).unwrap_or(0.0)
    }
}

pub struct LibraryTrash {
    pub library: PathBuf,
    pub directory: PathBuf,
}

pub fn note_files_in(folder: &Path) -> Vec<PathBuf> {
    walkdir_notes(folder, true)
}

fn walkdir_notes(folder: &Path, skip_hidden: bool) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut stack = vec![folder.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = e.path();
            let name = e.file_name();
            if skip_hidden && name.to_string_lossy().starts_with('.') {
                continue;
            }
            let ft = match e.file_type() {
                Ok(f) => f,
                Err(_) => continue,
            };
            if ft.is_dir() {
                stack.push(p);
            } else if is_note(&p) {
                out.push(p);
            }
        }
    }
    out
}

impl LibraryTrash {
    pub fn new(library: &Path) -> Self {
        LibraryTrash { library: library.to_path_buf(), directory: data_folder(library, "trash") }
    }

    pub fn content_path(&self, item: &TrashItem) -> PathBuf {
        self.directory.join(&item.id).join(item.name())
    }

    pub fn move_to_trash(&self, path: &Path, title: Option<&str>) -> Result<TrashItem, String> {
        let root = self.library.to_string_lossy().trim_end_matches('/').to_string() + "/";
        let src = path.to_string_lossy().to_string();
        if !src.starts_with(&root) {
            return Err("Nur Notizen und Ordner der Bibliothek können in den Papierkorb.".into());
        }
        let meta = std::fs::metadata(path).map_err(|_| format!("„{}“ ist nicht mehr da.", path.display()))?;
        let notes = if meta.is_dir() { note_files_in(path) } else { vec![path.to_path_buf()] };
        let ids: Vec<String> = notes.iter().filter_map(|p| NoteFile::read_metadata(p).ok().map(|m| m.id)).collect();
        let item = TrashItem {
            id: crate::new_id(),
            original_path: src[root.len()..].to_string(),
            deleted_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            is_folder: meta.is_dir(),
            title: title.map(|s| s.to_string()).unwrap_or_else(|| path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string()),
            note_count: notes.len(),
            note_ids: ids,
        };
        let holder = self.directory.join(&item.id);
        std::fs::create_dir_all(&holder).map_err(|e| e.to_string())?;
        let info = serde_json::to_vec(&item).map_err(|e| e.to_string())?;
        model::write_atomic(&holder.join("info.json"), &info).map_err(|e| e.to_string())?;
        if let Err(e) = std::fs::rename(path, self.content_path(&item)) {
            let _ = std::fs::remove_dir_all(&holder);
            return Err(e.to_string());
        }
        Ok(item)
    }

    pub fn items(&self) -> Vec<TrashItem> {
        let mut out: Vec<TrashItem> = std::fs::read_dir(&self.directory)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let data = std::fs::read(e.path().join("info.json")).ok()?;
                let item: TrashItem = serde_json::from_slice(&data).ok()?;
                if self.content_path(&item).exists() { Some(item) } else { None }
            })
            .collect();
        out.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at));
        out
    }

    pub fn restore(&self, item: &TrashItem) -> Result<PathBuf, String> {
        let content = self.content_path(item);
        if !content.exists() {
            return Err(format!("„{}“ ist nicht mehr im Papierkorb.", item.title));
        }
        let original = self.library.join(&item.original_path);
        if let Some(parent) = original.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let target = unique_like(&original);
        std::fs::rename(&content, &target).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_dir_all(self.directory.join(&item.id));
        Ok(target)
    }

    pub fn delete_permanently(&self, item: &TrashItem, history: Option<&NoteHistory>) -> Result<(), String> {
        let holder = self.directory.join(&item.id);
        if !holder.exists() {
            return Err(format!("„{}“ ist nicht mehr im Papierkorb.", item.title));
        }
        std::fs::remove_dir_all(&holder).map_err(|e| e.to_string())?;
        if let Some(h) = history {
            for id in &item.note_ids {
                h.remove_all(id);
            }
        }
        Ok(())
    }

    pub fn purge(&self, older_than_days: i64, now: f64, history: Option<&NoteHistory>) -> usize {
        if older_than_days <= 0 {
            return 0;
        }
        let limit = now - older_than_days as f64 * DAY;
        self.items().iter().filter(|i| i.deleted_swift() < limit).filter(|i| self.delete_permanently(i, history).is_ok()).count()
    }

    pub fn empty(&self, history: Option<&NoteHistory>) -> usize {
        self.items().iter().filter(|i| self.delete_permanently(i, history).is_ok()).count()
    }
}

// MARK: - Backups

#[derive(Debug, Clone, Serialize)]
pub struct BackupInfo {
    pub path: PathBuf,
    pub date: f64,
    pub size: u64,
}

pub mod backup {
    use super::*;
    use std::process::Command;

    pub const PREFIX: &str = "Folio-Backup-";
    const MANIFEST: &str = "Folio-Backup.json";

    pub fn list(dir: &Path) -> Vec<BackupInfo> {
        let mut out: Vec<BackupInfo> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                if p.extension()? != "zip" {
                    return None;
                }
                let stem = p.file_stem()?.to_str()?.to_string();
                let date = stamp::date(stem.strip_prefix(PREFIX)?)?;
                Some(BackupInfo { size: e.metadata().map(|m| m.len()).unwrap_or(0), path: p, date })
            })
            .collect();
        out.sort_by(|a, b| b.date.partial_cmp(&a.date).unwrap());
        out
    }

    fn run(exe: &str, args: &[&str], cwd: Option<&Path>, allow_nothing: bool) -> Result<(), String> {
        let mut c = Command::new(exe);
        c.args(args);
        if let Some(d) = cwd {
            c.current_dir(d);
        }
        let out = c.output().map_err(|e| format!("Die Sicherung ist fehlgeschlagen: {e}"))?;
        if allow_nothing && out.status.code() == Some(12) {
            return Ok(());
        }
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(format!("Die Sicherung ist fehlgeschlagen: {}", if msg.is_empty() { format!("{exe} Status {:?}", out.status.code()) } else { msg }));
        }
        Ok(())
    }

    /// Zips the library (without `.folio/` except organization.json) into `dir`.
    pub fn create(library: &Path, dir: &Path, date: f64) -> Result<BackupInfo, String> {
        if !library.exists() {
            return Err("Der Bibliotheksordner fehlt.".into());
        }
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let final_path = dir.join(format!("{PREFIX}{}.zip", stamp::string(date)));
        let partial = dir.join(format!(".partial-{}.zip", crate::new_id()));
        let tmp = std::env::temp_dir().join(format!("folio-manifest-{}", crate::new_id()));
        std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        let manifest = tmp.join(MANIFEST);
        let notes = note_files_in(library).len();
        let info = serde_json::json!({"library": library.to_string_lossy(), "created": chrono::Utc::now().to_rfc3339(), "notes": notes});
        std::fs::write(&manifest, serde_json::to_vec_pretty(&info).unwrap()).map_err(|e| e.to_string())?;
        let result = (|| {
            run("/usr/bin/zip", &["-q", "-j", &partial.to_string_lossy(), &manifest.to_string_lossy()], None, false)?;
            run("/usr/bin/zip", &["-r", "-q", "-y", &partial.to_string_lossy(), ".", "-x", ".folio/*", "*.DS_Store", ".partial-*", "*Folio-Backup-*.zip"], Some(library), true)?;
            if library.join(DATA_FOLDER).join("organization.json").exists() {
                run("/usr/bin/zip", &["-q", &partial.to_string_lossy(), ".folio/organization.json"], Some(library), true)?;
            }
            std::fs::rename(&partial, &final_path).map_err(|e| e.to_string())
        })();
        let _ = std::fs::remove_file(&partial);
        let _ = std::fs::remove_dir_all(&tmp);
        result?;
        let size = std::fs::metadata(&final_path).map(|m| m.len()).unwrap_or(0);
        Ok(BackupInfo { path: final_path, date, size })
    }

    pub fn unpack(zip: &Path, folder: &Path) -> Result<(), String> {
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        run("/usr/bin/ditto", &["-x", "-k", &zip.to_string_lossy(), &folder.to_string_lossy()], None, false)
    }

    /// Unpacks and decodes every note; returns (notes, damaged relative paths).
    pub fn verify(zip: &Path) -> Result<(usize, Vec<String>), String> {
        let tmp = std::env::temp_dir().join(format!("folio-backup-check-{}", crate::new_id()));
        unpack(zip, &tmp)?;
        let notes = note_files_in(&tmp);
        let base = tmp.to_string_lossy().to_string();
        let damaged = notes.iter().filter(|p| NoteFile::read(p).is_err()).map(|p| p.to_string_lossy()[base.len() + 1..].to_string()).collect();
        let _ = std::fs::remove_dir_all(&tmp);
        Ok((notes.len(), damaged))
    }

    pub fn restore(zip: &Path, parent: &Path, name: &str) -> Result<PathBuf, String> {
        let target = unique_like(&parent.join(name));
        unpack(zip, &target)?;
        let _ = std::fs::remove_file(target.join(MANIFEST));
        Ok(target)
    }

    pub fn prune(dir: &Path, now: f64) -> Vec<PathBuf> {
        let all = list(dir);
        let doomed = RetentionPolicy::BACKUPS.to_delete(&all.iter().map(|b| b.date).collect::<Vec<_>>(), now);
        all.into_iter().filter(|b| doomed.contains(&b.date)).filter(|b| std::fs::remove_file(&b.path).is_ok()).map(|b| b.path).collect()
    }
}

// MARK: - Organization (#13, #14, #15)

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SmartView {
    pub id: String,
    pub name: String,
    pub filter: SearchFilter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct LibraryOrganization {
    #[serde(default)]
    pub sort_mode: SortMode,
    #[serde(default)]
    pub favorites: Vec<String>,
    #[serde(default)]
    pub note_order: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub folder_order: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub smart_views: Vec<SmartView>,
    #[serde(default)]
    pub daily_notes: BTreeMap<String, String>,
}

pub enum OrgLoad {
    Loaded(LibraryOrganization),
    Missing,
    Unreadable(Option<String>),
}

impl LibraryOrganization {
    pub fn path(library: &Path) -> PathBuf {
        library.join(DATA_FOLDER).join("organization.json")
    }

    fn cleaned(mut self) -> Self {
        self.favorites = manual_order::unique(&self.favorites);
        for v in self.note_order.values_mut() {
            *v = manual_order::unique(v);
        }
        for v in self.folder_order.values_mut() {
            *v = manual_order::unique(v);
        }
        let mut ids = HashSet::new();
        self.smart_views.retain(|s| ids.insert(s.id.clone()));
        self
    }

    pub fn load(library: &Path) -> OrgLoad {
        let p = Self::path(library);
        match std::fs::read(&p) {
            Ok(data) => match serde_json::from_slice::<LibraryOrganization>(&data) {
                Ok(o) => OrgLoad::Loaded(o.cleaned()),
                Err(_) => OrgLoad::Unreadable(Self::set_aside(&p)),
            },
            Err(_) if !p.exists() => OrgLoad::Missing,
            Err(_) => OrgLoad::Unreadable(Self::set_aside(&p)),
        }
    }

    fn set_aside(p: &Path) -> Option<String> {
        let stamp = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ");
        let kept = p.with_file_name(format!("organization-defekt-{stamp}.json"));
        std::fs::rename(p, &kept).ok().map(|_| kept.file_name().unwrap().to_string_lossy().to_string())
    }

    pub fn save(&self, library: &Path) -> std::io::Result<Vec<u8>> {
        let p = Self::path(library);
        std::fs::create_dir_all(p.parent().unwrap())?;
        let v = serde_json::to_value(self).unwrap();
        let data = serde_json::to_vec_pretty(&v).unwrap();
        model::write_atomic(&p, &data)?;
        Ok(data)
    }

    pub fn toggle_favorite(&mut self, id: &str) {
        if let Some(i) = self.favorites.iter().position(|f| f == id) {
            self.favorites.remove(i);
        } else {
            self.favorites.push(id.to_string());
        }
    }

    /// Three-way merge with the file as another Mac left it (#175, #180).
    pub fn merging(&self, other: &LibraryOrganization, base: Option<&LibraryOrganization>) -> LibraryOrganization {
        let mut out = self.clone();
        let Some(base) = base else {
            out.favorites = manual_order::unique(&[self.favorites.clone(), other.favorites.clone()].concat());
            for (k, v) in &other.note_order {
                out.note_order.entry(k.clone()).or_insert(v.clone());
            }
            for (k, v) in &other.folder_order {
                out.folder_order.entry(k.clone()).or_insert(v.clone());
            }
            let have: HashSet<&String> = self.smart_views.iter().map(|s| &s.id).collect();
            out.smart_views.extend(other.smart_views.iter().filter(|s| !have.contains(&s.id)).cloned());
            let mut daily = other.daily_notes.clone();
            daily.extend(self.daily_notes.clone());
            out.daily_notes = daily;
            return out;
        };
        if self.sort_mode == base.sort_mode {
            out.sort_mode = other.sort_mode;
        }
        out.favorites = merge_list(&self.favorites, &other.favorites, &base.favorites, |s| s.clone());
        out.note_order = merge_map(&self.note_order, &other.note_order, &base.note_order);
        out.folder_order = merge_map(&self.folder_order, &other.folder_order, &base.folder_order);
        let mut views = merge_list(&self.smart_views, &other.smart_views, &base.smart_views, |s| s.id.clone());
        for v in views.iter_mut() {
            if let (Some(b), Some(t)) = (base.smart_views.iter().find(|x| x.id == v.id), other.smart_views.iter().find(|x| x.id == v.id)) {
                if v == b {
                    *v = t.clone();
                }
            }
        }
        out.smart_views = views;
        let mut daily = other.daily_notes.clone();
        daily.extend(self.daily_notes.clone());
        out.daily_notes = daily;
        out
    }

    /// A renamed tag keeps its list order and its place in smart views (#174).
    pub fn rename_tag(&mut self, old: &str, new: Option<&str>) {
        let renamed = |tag: &str| -> Option<String> {
            if tag == old {
                return new.map(|s| s.to_string());
            }
            if let Some(rest) = tag.strip_prefix(&format!("{old}/")) {
                return new.map(|n| format!("{n}/{rest}"));
            }
            Some(tag.to_string())
        };
        let mut orders: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (k, v) in &self.note_order {
            let Some(t) = k.strip_prefix("tag:") else {
                orders.insert(k.clone(), v.clone());
                continue;
            };
            let Some(nt) = renamed(t) else { continue };
            let key = format!("tag:{nt}");
            let merged = [orders.get(&key).cloned().unwrap_or_default(), v.clone()].concat();
            orders.insert(key, manual_order::unique(&merged));
        }
        self.note_order = orders;
        for sv in self.smart_views.iter_mut() {
            sv.filter.tags = sv.filter.tags.iter().filter_map(|t| renamed(t)).collect();
        }
    }

    pub fn prune(&mut self, ids: &HashSet<String>) {
        self.favorites.retain(|f| ids.contains(f));
        for v in self.note_order.values_mut() {
            v.retain(|i| ids.contains(i));
        }
        self.note_order.retain(|_, v| !v.is_empty());
    }
}

fn merge_list<T: Clone, K: std::hash::Hash + Eq>(mine: &[T], theirs: &[T], base: &[T], key: impl Fn(&T) -> K) -> Vec<T> {
    let base_k: HashSet<K> = base.iter().map(&key).collect();
    let their_k: HashSet<K> = theirs.iter().map(&key).collect();
    let my_k: HashSet<K> = mine.iter().map(&key).collect();
    let mut seen = HashSet::new();
    mine.iter()
        .filter(|t| { let k = key(t); !(base_k.contains(&k) && !their_k.contains(&k)) })
        .chain(theirs.iter().filter(|t| { let k = key(t); !base_k.contains(&k) && !my_k.contains(&k) }))
        .filter(|t| seen.insert(key(t)))
        .cloned()
        .collect()
}

fn merge_map<V: Clone + PartialEq>(mine: &BTreeMap<String, V>, theirs: &BTreeMap<String, V>, base: &BTreeMap<String, V>) -> BTreeMap<String, V> {
    let keys: HashSet<&String> = mine.keys().chain(theirs.keys()).chain(base.keys()).collect();
    let mut out = BTreeMap::new();
    for k in keys {
        let (m, t, b) = (mine.get(k), theirs.get(k), base.get(k));
        let v = if m == b { t } else { m };
        if let Some(v) = v {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

// MARK: - Scanner

#[derive(Debug, Clone, Serialize)]
pub struct Folder {
    pub path: PathBuf,
    pub name: String,
    pub children: Vec<Folder>,
}

#[derive(Debug, Clone)]
pub struct ScannedNote {
    pub path: PathBuf,
    pub meta: NoteMetadata,
}

pub struct ScanResult {
    pub folders: Vec<Folder>,
    pub notes: Vec<ScannedNote>,
    pub unreadable: Vec<PathBuf>,
    pub skipped_links: Vec<(PathBuf, String)>,
    pub cache_hits: usize,
}

/// Walks the library; metadata is read in parallel and cached by path, size and modification time (#62).
#[derive(Default)]
pub struct Scanner {
    cache: Mutex<HashMap<(PathBuf, u64, i128), NoteMetadata>>,
}

impl Scanner {
    pub fn clear(&self) {
        self.cache.lock().clear();
    }

    pub fn scan(&self, root: &Path) -> ScanResult {
        let mut walk = Walk { real_root: canonical(root), visited: HashSet::new(), notes: vec![], skipped: vec![] };
        let folders = walk.folders(root);
        let results: Vec<(PathBuf, Option<NoteMetadata>, bool)> = walk
            .notes
            .par_iter()
            .map(|p| {
                let fp = model::Fingerprint::of(p);
                let key = fp.map(|f| (p.clone(), f.size, f.modified_ns));
                if let Some(k) = &key {
                    if let Some(m) = self.cache.lock().get(k) {
                        return (p.clone(), Some(m.clone()), true);
                    }
                }
                match NoteFile::read_metadata(p) {
                    Ok(m) => {
                        if let Some(k) = key {
                            self.cache.lock().insert(k, m.clone());
                        }
                        (p.clone(), Some(m), false)
                    }
                    Err(_) => (p.clone(), None, false),
                }
            })
            .collect();
        let mut notes = vec![];
        let mut unreadable = vec![];
        let mut hits = 0;
        let mut seen = HashSet::new();
        for (p, m, hit) in results {
            if hit {
                hits += 1;
            }
            match m {
                Some(meta) => {
                    if seen.insert(canonical(&p)) {
                        notes.push(ScannedNote { path: p, meta });
                    }
                }
                None => unreadable.push(p),
            }
        }
        ScanResult { folders, notes, unreadable, skipped_links: walk.skipped, cache_hits: hits }
    }
}

fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

struct Walk {
    real_root: PathBuf,
    visited: HashSet<PathBuf>,
    notes: Vec<PathBuf>,
    skipped: Vec<(PathBuf, String)>,
}

fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    // Like Finder: case-insensitive, numbers by value.
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = ai.peek().copied().filter(|c| c.is_ascii_digit()) {
                    na.push(c);
                    ai.next();
                }
                let mut nb = String::new();
                while let Some(c) = bi.peek().copied().filter(|c| c.is_ascii_digit()) {
                    nb.push(c);
                    bi.next();
                }
                let o = na.trim_start_matches('0').len().cmp(&nb.trim_start_matches('0').len()).then(na.trim_start_matches('0').cmp(nb.trim_start_matches('0')));
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                let o = crate::text::fold_char(x).cmp(&crate::text::fold_char(y));
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
                ai.next();
                bi.next();
            }
        }
    }
}

pub fn natural_order(a: &str, b: &str) -> std::cmp::Ordering {
    natural_cmp(a, b)
}

impl Walk {
    fn inside(&self, p: &Path) -> bool {
        p == self.real_root || p.starts_with(&self.real_root)
    }

    fn folders(&mut self, dir: &Path) -> Vec<Folder> {
        if !self.visited.insert(canonical(dir)) {
            return vec![];
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
        let mut items: Vec<_> = rd.flatten().filter(|e| !e.file_name().to_string_lossy().starts_with('.')).collect();
        items.sort_by(|a, b| natural_cmp(&a.file_name().to_string_lossy(), &b.file_name().to_string_lossy()));
        let mut out = vec![];
        for e in items {
            let path = e.path();
            let Ok(ft) = e.file_type() else { continue };
            let target = if ft.is_symlink() {
                let resolved = canonical(&path);
                if !resolved.exists() {
                    self.skipped.push((path, "Ziel fehlt".into()));
                    continue;
                }
                if !self.inside(&resolved) {
                    self.skipped.push((path, "zeigt aus der Bibliothek heraus".into()));
                    continue;
                }
                resolved
            } else {
                path.clone()
            };
            let is_dir = target.is_dir();
            if is_note(&path) && !is_dir {
                self.notes.push(path);
                continue;
            }
            if !is_dir || is_package(&target) {
                continue;
            }
            if self.visited.contains(&canonical(&target)) {
                self.skipped.push((path, "Schleife (Ordner ist schon enthalten)".into()));
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            let children = self.folders(&path);
            out.push(Folder { path, name, children });
        }
        out
    }
}

/// Bundles such as .app, .numbers, .rtfd are files for the user, not folders.
fn is_package(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref(),
        Some("app" | "numbers" | "pages" | "key" | "rtfd" | "bundle" | "framework" | "photoslibrary" | "playground" | "xcodeproj" | "xcworkspace" | "plugin" | "kext" | "pkg")
    )
}

/// Ids shared by several files: the known (or oldest) file keeps it, the others get renewed (#166).
pub fn resolve_duplicate_ids(notes: &[(PathBuf, String)], known: &HashMap<String, PathBuf>) -> Vec<PathBuf> {
    let mut groups: HashMap<&String, Vec<&PathBuf>> = HashMap::new();
    for (p, id) in notes {
        groups.entry(id).or_default().push(p);
    }
    let mut renew = vec![];
    for (id, paths) in groups {
        if paths.len() < 2 {
            continue;
        }
        let keeper = known
            .get(id)
            .and_then(|k| paths.iter().find(|p| **p == k).copied())
            .unwrap_or_else(|| {
                *paths
                    .iter()
                    .min_by(|a, b| {
                        let ca = std::fs::metadata(a).and_then(|m| m.created()).ok();
                        let cb = std::fs::metadata(b).and_then(|m| m.created()).ok();
                        ca.cmp(&cb).then(a.cmp(b))
                    })
                    .unwrap()
            });
        renew.extend(paths.into_iter().filter(|p| *p != keeper).cloned());
    }
    renew.sort();
    renew
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps() {
        let d = 800_000_000.123;
        let s = stamp::string(d);
        assert_eq!(s.len(), 19);
        assert!((stamp::date(&s).unwrap() - d).abs() < 0.002);
    }

    #[test]
    fn retention_keeps_newest() {
        let now = 800_000_000.0;
        let dates: Vec<f64> = (0..100).map(|i| now - i as f64 * 3600.0 * 5.0).collect();
        let del = RetentionPolicy::VERSIONS.to_delete(&dates, now);
        assert!(!del.contains(&now));
        assert!(!del.is_empty());
    }

    #[test]
    fn trash_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path();
        std::fs::create_dir_all(lib.join("A")).unwrap();
        let f = NoteFile::new(NoteMetadata::titled("X"), vec![]);
        f.write(&lib.join("A/X.folio")).unwrap();
        let t = LibraryTrash::new(lib);
        let item = t.move_to_trash(&lib.join("A/X.folio"), Some("X")).unwrap();
        assert_eq!(t.items().len(), 1);
        assert!(!lib.join("A/X.folio").exists());
        t.restore(&item).unwrap();
        assert!(lib.join("A/X.folio").exists());
        let s = Scanner::default().scan(lib);
        assert_eq!(s.notes.len(), 1);
        assert_eq!(s.folders.len(), 1);
    }
}
