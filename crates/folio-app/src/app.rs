//! The app's state and its commands. The window (Tauri) and the test server both call `App::call` with a
//! command name and JSON arguments; nothing here knows about the user interface toolkit.

use crate::settings::Settings;
use folio_core::library::{self, LibraryOrganization, LibraryTrash, NoteHistory, OrgLoad, Scanner};
use folio_core::links::LinkIndex;
use folio_core::model::{self, kind, BlockRecord, Fingerprint, NoteFile, NoteMetadata};
use folio_core::search::{manual_order, SearchFilter, SortMode};
use folio_core::text;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub type CmdResult = Result<Value, String>;

#[derive(Clone)]
pub struct NoteRef {
    pub path: PathBuf,
    pub meta: NoteMetadata,
    /// Folded title + search text, for repeated searches (#62).
    pub haystack: String,
}

pub struct OpenDoc {
    pub path: PathBuf,
    pub file: NoteFile,
    pub fingerprint: Option<Fingerprint>,
    pub last_version: f64,
}

pub enum Job {
    Run(folio_core::runner::CodeRun),
    Ai(Arc<Mutex<AiJob>>),
}

#[derive(Default, Clone, serde::Serialize)]
pub struct AiJob {
    pub text: String,
    pub finished: bool,
    pub error: Option<String>,
    pub cancelled: bool,
    pub sources: Vec<Value>,
}

pub struct State {
    pub settings: Settings,
    pub root: PathBuf,
    pub scanner: Scanner,
    pub notes: Vec<NoteRef>,
    pub folders: Vec<library::Folder>,
    pub templates: Vec<NoteRef>,
    pub damaged: Vec<PathBuf>,
    pub skipped_links: Vec<String>,
    pub org: LibraryOrganization,
    pub org_on_disk: Option<Vec<u8>>,
    pub links: LinkIndex,
    pub docs: HashMap<String, OpenDoc>,
    pub jobs: HashMap<String, Job>,
    pub last_error: Option<String>,
}

pub struct App {
    pub state: Mutex<State>,
}

pub fn arg_str<'a>(a: &'a Value, k: &str) -> Result<&'a str, String> {
    a.get(k).and_then(|v| v.as_str()).ok_or_else(|| format!("Argument „{k}“ fehlt."))
}

pub fn opt_str(a: &Value, k: &str) -> Option<String> {
    a.get(k).and_then(|v| v.as_str()).map(|s| s.to_string())
}

impl App {
    pub fn new() -> App {
        let settings = Settings::load();
        let root = PathBuf::from(&settings.library_path);
        let st = State {
            settings,
            root,
            scanner: Scanner::default(),
            notes: vec![],
            folders: vec![],
            templates: vec![],
            damaged: vec![],
            skipped_links: vec![],
            org: LibraryOrganization::default(),
            org_on_disk: None,
            links: LinkIndex::default(),
            docs: HashMap::new(),
            jobs: HashMap::new(),
            last_error: None,
        };
        let app = App { state: Mutex::new(st) };
        {
            let mut s = app.state.lock();
            s.prepare_root();
            s.load_org();
            s.reload();
            let days = s.settings.trash_retention_days;
            let root = s.root.clone();
            LibraryTrash::new(&root).purge(days, model::now(), Some(&NoteHistory::new(&root)));
        }
        app
    }

    pub fn call(&self, cmd: &str, args: Value) -> CmdResult {
        crate::commands::dispatch(self, cmd, args)
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    pub fn prepare_root(&mut self) {
        let _ = std::fs::create_dir_all(&self.root);
        let empty = std::fs::read_dir(&self.root).map(|r| r.flatten().all(|e| e.file_name().to_string_lossy().starts_with('.'))).unwrap_or(true);
        if empty {
            let _ = std::fs::create_dir_all(self.root.join("Notizen"));
        }
    }

    pub fn rel_folder(&self, path: &Path) -> String {
        let dir = if path.extension().is_some() && !path.is_dir() { path.parent().unwrap_or(path) } else { path };
        dir.strip_prefix(&self.root).map(|p| p.to_string_lossy().to_string()).unwrap_or_default()
    }

    pub fn abs_folder(&self, rel: &str) -> PathBuf {
        if rel.is_empty() { self.root.clone() } else { self.root.join(rel) }
    }

    pub fn load_org(&mut self) {
        match LibraryOrganization::load(&self.root) {
            OrgLoad::Loaded(o) => {
                self.org = o;
                self.org_on_disk = std::fs::read(LibraryOrganization::path(&self.root)).ok();
            }
            OrgLoad::Missing => self.org = LibraryOrganization::default(),
            OrgLoad::Unreadable(kept) => {
                self.org = LibraryOrganization::default();
                self.last_error = Some(format!("Die Ordnung der Bibliothek war nicht lesbar{}; Reihenfolge und Favoriten beginnen neu.", kept.map(|k| format!(" und liegt jetzt als „{k}“ daneben")).unwrap_or_default()));
            }
        }
    }

    /// Saves the organization, merging changes another Mac made meanwhile (#175).
    pub fn save_org(&mut self) {
        if let Ok(disk) = std::fs::read(LibraryOrganization::path(&self.root)) {
            if self.org_on_disk.as_ref() != Some(&disk) {
                if let Ok(theirs) = serde_json::from_slice::<LibraryOrganization>(&disk) {
                    let base = self.org_on_disk.as_ref().and_then(|b| serde_json::from_slice::<LibraryOrganization>(b).ok());
                    self.org = self.org.merging(&theirs, base.as_ref());
                }
            }
        }
        match self.org.save(&self.root) {
            Ok(d) => self.org_on_disk = Some(d),
            Err(e) => self.last_error = Some(format!("Die Ordnung der Bibliothek konnte nicht gesichert werden: {e}")),
        }
    }

    pub fn note_ref(&self, path: PathBuf, meta: NoteMetadata) -> NoteRef {
        let haystack = SearchFilter::haystack(&meta);
        NoteRef { path, meta, haystack }
    }

    pub fn reload(&mut self) {
        let res = self.scanner.scan(&self.root);
        self.folders = res.folders;
        let found: Vec<(PathBuf, String)> = res.notes.iter().map(|n| (n.path.clone(), n.meta.id.clone())).collect();
        let known: HashMap<String, PathBuf> = self.notes.iter().map(|n| (n.meta.id.clone(), n.path.clone())).collect();
        let renew: HashSet<PathBuf> = library::resolve_duplicate_ids(&found, &known).into_iter().collect();
        let mut notes = vec![];
        for n in res.notes {
            if renew.contains(&n.path) {
                if let Ok(f) = NoteFile::read(&n.path) {
                    let f = model::renewed(&f);
                    if f.write(&n.path).is_ok() {
                        notes.push(self.note_ref(n.path, f.meta));
                    }
                }
                continue;
            }
            notes.push(self.note_ref(n.path, n.meta));
        }
        self.notes = notes;
        self.damaged = res.unreadable;
        self.skipped_links = res.skipped_links.iter().map(|(p, r)| format!("{}: {r}", p.file_name().unwrap_or_default().to_string_lossy())).collect();
        self.templates = self.scan_templates();
        self.rebuild_links();
        // Open documents follow moved files.
        let paths: HashMap<String, PathBuf> = self.notes.iter().map(|n| (n.meta.id.clone(), n.path.clone())).collect();
        for (id, d) in self.docs.iter_mut() {
            if let Some(p) = paths.get(id) {
                d.path = p.clone();
            }
        }
    }

    pub fn scan_templates(&self) -> Vec<NoteRef> {
        let dir = library::data_folder(&self.root, "templates");
        let mut v: Vec<NoteRef> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| library::is_note(&e.path()))
            .filter_map(|e| NoteFile::read_metadata(&e.path()).ok().map(|m| self.note_ref(e.path(), m)))
            .collect();
        v.sort_by(|a, b| library::natural_order(&a.meta.title, &b.meta.title));
        v
    }

    pub fn rebuild_links(&mut self) {
        let mut ix = LinkIndex::default();
        for n in &self.notes {
            ix.update(&n.meta, &self.rel_folder(&n.path));
        }
        self.links = ix;
    }

    pub fn find(&self, id: &str) -> Option<&NoteRef> {
        self.notes.iter().find(|n| n.meta.id == id).or_else(|| self.templates.iter().find(|n| n.meta.id == id))
    }

    pub fn path_of(&self, id: &str) -> Result<PathBuf, String> {
        if let Some(d) = self.docs.get(id) {
            return Ok(d.path.clone());
        }
        self.find(id).map(|n| n.path.clone()).ok_or_else(|| "Die Notiz gibt es nicht mehr (vielleicht im Papierkorb).".to_string())
    }

    pub fn is_template(&self, p: &Path) -> bool {
        p.starts_with(library::data_folder(&self.root, "templates"))
    }

    /// Updates the in-memory list after a note was written.
    pub fn upsert(&mut self, path: &Path, meta: &NoteMetadata) {
        let r = self.note_ref(path.to_path_buf(), meta.clone());
        let list = if self.is_template(path) { &mut self.templates } else { &mut self.notes };
        match list.iter().position(|n| n.meta.id == meta.id) {
            Some(i) => list[i] = r,
            None => list.push(r),
        }
        if !self.is_template(path) {
            let folder = self.rel_folder(path);
            self.links.update(meta, &folder);
        }
    }

    pub fn summary(&self, n: &NoteRef) -> Value {
        json!({
            "id": n.meta.id,
            "title": n.meta.title,
            "preview": n.meta.preview,
            "tags": n.meta.tags,
            "created": model::to_unix(n.meta.created),
            "modified": model::to_unix(n.meta.modified),
            "pinned": n.meta.pinned,
            "folder": self.rel_folder(&n.path),
            "kinds": n.meta.kinds,
            "favorite": self.org.favorites.contains(&n.meta.id),
            "file": n.path.file_name().map(|f| f.to_string_lossy().to_string()),
        })
    }

    pub fn order_key(selection: &Value) -> Option<String> {
        let t = selection.get("type").and_then(|t| t.as_str()).unwrap_or("all");
        let v = selection.get("value").and_then(|v| v.as_str()).unwrap_or("");
        match t {
            "all" => Some("all".into()),
            "pinned" => Some("pinned".into()),
            "favorites" => Some("favorites".into()),
            "folder" => Some(format!("folder:{v}")),
            "tag" => Some(format!("tag:{v}")),
            "smart" => Some(format!("smart:{v}")),
            _ => None,
        }
    }

    fn in_folder(&self, n: &NoteRef, rel: &str, sub: bool) -> bool {
        let f = self.rel_folder(&n.path);
        f == rel || (sub && (rel.is_empty() || f.starts_with(&format!("{rel}/"))))
    }

    pub fn selection_notes(&self, selection: &Value) -> Vec<&NoteRef> {
        let t = selection.get("type").and_then(|t| t.as_str()).unwrap_or("all");
        let v = selection.get("value").and_then(|v| v.as_str()).unwrap_or("");
        match t {
            "all" => self.notes.iter().collect(),
            "pinned" => self.notes.iter().filter(|n| n.meta.pinned).collect(),
            "favorites" => self.notes.iter().filter(|n| self.org.favorites.contains(&n.meta.id)).collect(),
            "folder" => self.notes.iter().filter(|n| self.in_folder(n, v, self.settings.include_subfolders)).collect(),
            "tag" => self.notes.iter().filter(|n| n.meta.tags.iter().any(|t| t == v || t.starts_with(&format!("{v}/")))).collect(),
            "smart" => match self.org.smart_views.iter().find(|s| s.id == v) {
                Some(sv) => self.matching(&sv.filter, self.notes.iter().collect()),
                None => vec![],
            },
            "templates" => self.templates.iter().collect(),
            _ => vec![],
        }
    }

    pub fn matching<'a>(&'a self, f: &SearchFilter, list: Vec<&'a NoteRef>) -> Vec<&'a NoteRef> {
        let now = model::now();
        list.into_iter().filter(|n| f.matches(&n.meta, &self.rel_folder(&n.path), None, now, Some(&n.haystack))).collect()
    }

    pub fn sorted<'a>(&'a self, mut list: Vec<&'a NoteRef>, key: Option<&str>) -> Vec<&'a NoteRef> {
        let mode = self.org.sort_mode;
        match mode {
            SortMode::Modified => list.sort_by(|a, b| b.meta.modified.partial_cmp(&a.meta.modified).unwrap()),
            SortMode::Created => list.sort_by(|a, b| b.meta.created.partial_cmp(&a.meta.created).unwrap()),
            SortMode::Title => list.sort_by(|a, b| library::natural_order(&a.meta.title, &b.meta.title)),
            SortMode::Manual => {
                list.sort_by(|a, b| b.meta.modified.partial_cmp(&a.meta.modified).unwrap());
                if let Some(order) = key.and_then(|k| self.org.note_order.get(k)) {
                    list = manual_order::apply(list, order, |n| n.meta.id.clone(), true);
                }
            }
        }
        // Pinned notes stay on top in every order.
        let (pinned, rest): (Vec<&NoteRef>, Vec<&NoteRef>) = list.into_iter().partition(|n| n.meta.pinned);
        pinned.into_iter().chain(rest).collect()
    }

    pub fn tag_counts(&self) -> Vec<(String, usize)> {
        let mut c: HashMap<String, usize> = HashMap::new();
        for n in &self.notes {
            for t in &n.meta.tags {
                *c.entry(t.clone()).or_default() += 1;
            }
        }
        let mut v: Vec<(String, usize)> = c.into_iter().collect();
        v.sort_by(|a, b| library::natural_order(&a.0, &b.0));
        v
    }

    pub fn ordered_folders(&self, folders: &[library::Folder], parent: &str) -> Vec<Value> {
        let mut list: Vec<&library::Folder> = folders.iter().collect();
        if let Some(order) = self.org.folder_order.get(parent) {
            list = manual_order::apply(list, order, |f| f.name.clone(), false);
        }
        list.into_iter()
            .map(|f| {
                let rel = self.rel_folder(&f.path);
                let count = self.notes.iter().filter(|n| self.rel_folder(&n.path) == rel).count();
                json!({"path": rel, "name": f.name, "count": count, "children": self.ordered_folders(&f.children, &rel)})
            })
            .collect()
    }

    /// Folder for new notes: the selection's folder, else the default "Notizen".
    pub fn target_folder(&self, rel: Option<&str>) -> PathBuf {
        if let Some(r) = rel.filter(|r| !r.is_empty()) {
            let p = self.root.join(r);
            if p.is_dir() {
                return p;
            }
        }
        let def = self.root.join("Notizen");
        let _ = std::fs::create_dir_all(&def);
        def
    }

    pub fn record_version(&self, data: Vec<u8>, id: String, force: bool) {
        let root = self.root.clone();
        std::thread::spawn(move || {
            let _ = NoteHistory::new(&root).record(&data, &id, model::now(), force);
        });
    }

    pub fn write_new(&mut self, file: &NoteFile, dir: &Path) -> Result<PathBuf, String> {
        let _ = std::fs::create_dir_all(dir);
        let base = text::sanitize_filename(if file.meta.title.is_empty() { "Neue Notiz" } else { &file.meta.title });
        let path = library::unique_path(dir, &base, "folio");
        file.write(&path).map_err(|e| format!("Notiz konnte nicht angelegt werden: {e}"))?;
        self.upsert(&path, &file.meta);
        Ok(path)
    }

    /// The file is named after the note's title.
    pub fn rename_to_title(&mut self, id: &str) {
        let Some(doc) = self.docs.get(id) else { return };
        let base = text::sanitize_filename(if doc.file.meta.title.is_empty() { "Neue Notiz" } else { &doc.file.meta.title });
        let cur = doc.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if cur == base || (cur.starts_with(&format!("{base} ")) && cur[base.len() + 1..].parse::<u32>().is_ok()) {
            return;
        }
        let dir = doc.path.parent().unwrap_or(&self.root).to_path_buf();
        let target = library::unique_path(&dir, &base, "folio");
        if std::fs::rename(&doc.path, &target).is_ok() {
            let meta = doc.file.meta.clone();
            let old = doc.path.clone();
            if let Some(d) = self.docs.get_mut(id) {
                d.path = target.clone();
                d.fingerprint = Fingerprint::of(&target);
            }
            let list = if self.is_template(&old) { &mut self.templates } else { &mut self.notes };
            if let Some(n) = list.iter_mut().find(|n| n.meta.id == meta.id) {
                n.path = target;
            }
        }
    }
}

/// Block for the editor: text blocks carry their rich text instead of RTF.
pub fn block_to_ui(b: &BlockRecord, body_size: f32) -> Value {
    let mut v = serde_json::to_value(b).unwrap_or(json!({}));
    if b.kind == kind::TEXT {
        let rt = folio_core::rtf::parse_for_editor(&b.rtf_bytes().unwrap_or_default(), body_size);
        v["rich"] = serde_json::to_value(rt).unwrap();
        if let Some(o) = v.as_object_mut() {
            o.remove("rtf");
        }
    }
    v
}

/// Block from the editor: rich text becomes RTF again; unknown fields of the old record are kept.
pub fn block_from_ui(v: &Value, old: Option<&BlockRecord>, settings: &Settings) -> Result<BlockRecord, String> {
    let mut v = v.clone();
    let rich = v.as_object_mut().and_then(|o| o.remove("rich"));
    let mut b: BlockRecord = serde_json::from_value(v).map_err(|e| format!("Block nicht lesbar: {e}"))?;
    if let Some(old) = old {
        for (k, val) in &old.extra {
            b.extra.entry(k.clone()).or_insert(val.clone());
        }
    }
    if b.kind == kind::TEXT {
        if let Some(r) = rich {
            let mut rt: folio_core::rtf::RichText = serde_json::from_value(r).map_err(|e| format!("Text nicht lesbar: {e}"))?;
            rt.normalize();
            b.set_rtf(&folio_core::rtf::write(&rt, &settings.write_options()));
        } else if let Some(old) = old {
            b.rtf = old.rtf.clone();
        }
    }
    Ok(b)
}
