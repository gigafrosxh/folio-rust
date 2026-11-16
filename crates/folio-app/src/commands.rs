//! Command dispatch: every action of the user interface is one named command with JSON arguments.

use crate::app::*;
use folio_core::library::{self, backup, LibraryTrash, NoteHistory, SmartView};
use folio_core::model::{self, kind, BlockRecord, Fingerprint, NoteFile, NoteMetadata};
use folio_core::search::{manual_order, SearchFilter, SortMode, SwitcherEntry};
use folio_core::{ai, capture, languages, markdown, note, text};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "r");

pub fn dispatch(app: &App, cmd: &str, a: Value) -> CmdResult {
    match cmd {
        // Jobs are polled often; they must not wait for the library lock longer than needed.
        "job_poll" => return job_poll(app, &a),
        "job_cancel" => return job_cancel(app, &a),
        "job_send" => return job_send(app, &a),
        "ai_models" => {
            let (url, model) = { let s = app.state.lock(); (s.settings.ollama_url.clone(), s.settings.ollama_model.clone()) };
            let o = ai::Ollama::new(&url, &model)?;
            return Ok(json!(o.models()?));
        }
        _ => {}
    }
    let mut s = app.state.lock();
    let st = &mut *s;
    let result = match cmd {
        "init" => init(st),
        "settings_get" => Ok(serde_json::to_value(&st.settings).unwrap()),
        "settings_set" => settings_set(st, &a),
        "library" => library_view(st),
        "reload" => {
            st.reload();
            library_view(st)
        }
        "set_library" => set_library(st, &a),
        "notes_list" => notes_list(st, &a),
        "note_open" => note_open(st, &a),
        "note_save" => note_save(st, &a),
        "note_close" => {
            let id = arg_str(&a, "id")?;
            st.docs.remove(id);
            Ok(json!(true))
        }
        "note_check_external" => note_check_external(st, &a),
        "note_resolve_conflict" => note_resolve_conflict(st, &a),
        "note_create" => note_create(st, &a),
        "note_duplicate" => note_duplicate(st, &a),
        "note_delete" => note_delete(st, &a),
        "note_move" => note_move(st, &a),
        "note_pin" => note_pin(st, &a),
        "note_favorite" => {
            let id = arg_str(&a, "id")?;
            st.org.toggle_favorite(id);
            st.save_org();
            Ok(json!(st.org.favorites.contains(&id.to_string())))
        }
        "note_info" => note_info(st, &a),
        "note_links" => note_links(st, &a),
        "note_by_title" => note_by_title(st, &a),
        "note_embed" => note_embed(st, &a),
        "link_mention" => link_mention(st, &a),
        "recent" => Ok(json!(st.settings.recent.iter().filter_map(|id| st.find(id)).take(15).map(|n| st.summary(n)).collect::<Vec<_>>())),
        "folder_create" => folder_create(st, &a),
        "folder_rename" => folder_rename(st, &a),
        "folder_delete" => folder_delete(st, &a),
        "folder_move_order" => folder_move_order(st, &a),
        "folder_sort_alpha" => {
            let parent = opt_str(&a, "parent").unwrap_or_default();
            st.org.folder_order.remove(&parent);
            st.save_org();
            Ok(json!(true))
        }
        "sort_mode" => {
            st.org.sort_mode = serde_json::from_value(a["mode"].clone()).map_err(|e| e.to_string())?;
            st.save_org();
            Ok(json!(true))
        }
        "note_move_order" => note_move_order(st, &a),
        "smart_view_save" => smart_view_save(st, &a),
        "smart_view_delete" => {
            let id = arg_str(&a, "id")?;
            st.org.smart_views.retain(|v| v.id != id);
            st.save_org();
            Ok(json!(true))
        }
        "smart_view_rename" => {
            let id = arg_str(&a, "id")?;
            let name = arg_str(&a, "name")?.to_string();
            if let Some(v) = st.org.smart_views.iter_mut().find(|v| v.id == id) {
                v.name = name;
            }
            st.save_org();
            Ok(json!(true))
        }
        "tag_edit" => tag_edit(st, &a),
        "trash_list" => Ok(json!(LibraryTrash::new(&st.root).items())),
        "trash_restore" => trash_restore(st, &a),
        "trash_delete" => trash_delete(st, &a),
        "trash_empty" => {
            let n = LibraryTrash::new(&st.root).empty(Some(&NoteHistory::new(&st.root)));
            Ok(json!(n))
        }
        "history_list" => history_list(st, &a),
        "history_preview" => history_preview(st, &a),
        "history_restore" => history_restore(st, &a),
        "backup_list" => Ok(json!(backup::list(&st.settings.backup_dir()).iter().map(|b| json!({"path": b.path, "date": model::to_unix(b.date), "size": b.size})).collect::<Vec<_>>())),
        "backup_now" => backup_now(st),
        "backup_due" => backup_due(st),
        "backup_verify" => {
            let (n, damaged) = backup::verify(Path::new(arg_str(&a, "path")?))?;
            Ok(json!({"notes": n, "damaged": damaged}))
        }
        "backup_restore" => {
            let name = format!("Wiederhergestellt {}", chrono::Local::now().format("%Y-%m-%d %H-%M"));
            let p = backup::restore(Path::new(arg_str(&a, "path")?), &st.root.clone(), &name)?;
            st.reload();
            Ok(json!(st.rel_folder(&p.join("x"))))
        }
        "damaged_list" => Ok(json!(st.damaged.iter().map(|p| json!({"path": p, "name": p.file_name().map(|n| n.to_string_lossy().to_string()), "problem": NoteFile::read(p).err().map(|e| e.to_string())})).collect::<Vec<_>>())),
        "damaged_quarantine" => {
            let p = PathBuf::from(arg_str(&a, "path")?);
            let dir = library::data_folder(&st.root, "damaged");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let target = library::unique_like(&dir.join(p.file_name().unwrap_or_default()));
            std::fs::rename(&p, &target).map_err(|e| e.to_string())?;
            st.reload();
            Ok(json!(true))
        }
        "templates_list" => {
            ensure_starter_templates(st);
            Ok(json!(st.templates.iter().map(|t| st.summary(t)).collect::<Vec<_>>()))
        }
        "template_from_note" => template_from_note(st, &a),
        "template_new" => template_new(st),
        "template_delete" => {
            let id = arg_str(&a, "id")?;
            let p = st.path_of(id)?;
            st.docs.remove(id);
            let _ = std::fs::remove_file(&p);
            st.templates = st.scan_templates();
            Ok(json!(true))
        }
        "daily_note" => daily_note(st, &a),
        "switcher" => switcher(st, &a),
        "graph" => graph(st, &a),
        "attachment_add" => attachment_add(st, &a),
        "attachment_info" => attachment_info(st, &a),
        "attachment_export" => attachment_export(st, &a),
        "attachment_open" => attachment_open(st, &a),
        "attachment_replace" => attachment_replace(st, &a),
        "markdown_export" => markdown_export(st, &a),
        "markdown_import" => markdown_import(st, &a),
        "apple_notes_import" => crate::import::apple_notes(st, &a),
        "languages" => Ok(json!(languages::all())),
        "run_code" => run_code(st, &a),
        "check_test" => {
            let (ok, msg) = folio_core::runner::check_test(&a["test"], a["output"].as_str().unwrap_or(""), a["exitCode"].as_i64().unwrap_or(0) as i32);
            Ok(json!({"ok": ok, "message": msg}))
        }
        "db_query" => crate::database::query(st, &a),
        "ai_chat" => ai_chat(st, &a),
        "ai_action" => ai_action(st, &a),
        "ai_segments" => Ok(json!(ai::segments(arg_str(&a, "text")?))),
        "ai_tags" => ai_tags(st, &a),
        "capture" => capture_cmd(st, &a),
        "deep_link" => Ok(serde_json::to_value(capture::parse_url(arg_str(&a, "url")?)?).unwrap()),
        "find_ranges" => Ok(json!(text::find_ranges(arg_str(&a, "query")?, arg_str(&a, "text")?, a["caseSensitive"].as_bool().unwrap_or(false), a["wholeWord"].as_bool().unwrap_or(false)))),
        "fill_template_text" => {
            let (t, c) = text::fill_template(arg_str(&a, "text")?, &chrono::Local::now(), &opt_str(&a, "title").unwrap_or_default());
            Ok(json!({"text": t, "cursor": c}))
        }
        "reveal" => {
            let p = PathBuf::from(arg_str(&a, "path")?);
            std::process::Command::new("/usr/bin/open").arg("-R").arg(&p).spawn().map_err(|e| e.to_string())?;
            Ok(json!(true))
        }
        "open_url" => {
            let u = arg_str(&a, "url")?;
            if !markdown::is_safe_link(u) {
                return Err("Dieser Link wird aus Sicherheitsgründen nicht geöffnet.".into());
            }
            std::process::Command::new("/usr/bin/open").arg(u).spawn().map_err(|e| e.to_string())?;
            Ok(json!(true))
        }
        "last_error" => Ok(json!(st.last_error.take())),
        "selftest_report" => {
            // Background self test in the real window (FOLIO_SELFTEST): print the result and quit.
            for line in a["lines"].as_array().into_iter().flatten() {
                println!("{}", line.as_str().unwrap_or(""));
            }
            let failed = a["failed"].as_u64().unwrap_or(1);
            std::process::exit(if failed == 0 { 0 } else { 1 });
        }
        _ => Err(format!("Unbekannter Befehl „{cmd}“.")),
    };
    result
}

fn init(st: &mut State) -> CmdResult {
    Ok(json!({
        "version": VERSION,
        "settings": st.settings,
        "library": st.root,
        "languages": languages::all(),
        "background": std::env::var("FOLIO_BACKGROUND").is_ok(),
        "selftest": std::env::var("FOLIO_SELFTEST").is_ok() && std::env::var("FOLIO_LIBRARY").is_ok(),
        "contentTypes": folio_core::search::CONTENT_TYPES.iter().map(|(id, l, _)| json!({"id": id, "label": l})).collect::<Vec<_>>(),
    }))
}

fn settings_set(st: &mut State, a: &Value) -> CmdResult {
    let mut cur = serde_json::to_value(&st.settings).unwrap();
    if let (Some(c), Some(n)) = (cur.as_object_mut(), a.get("settings").and_then(|v| v.as_object())) {
        for (k, v) in n {
            c.insert(k.clone(), v.clone());
        }
    }
    let new: crate::settings::Settings = serde_json::from_value(cur).map_err(|e| e.to_string())?;
    let lib_changed = new.library_path != st.settings.library_path;
    st.settings = new;
    st.settings.save();
    if lib_changed {
        let p = st.settings.library_path.clone();
        return set_library(st, &json!({"path": p}));
    }
    Ok(serde_json::to_value(&st.settings).unwrap())
}

fn set_library(st: &mut State, a: &Value) -> CmdResult {
    let p = PathBuf::from(arg_str(a, "path")?);
    st.docs.clear();
    st.root = p.clone();
    st.settings.library_path = p.to_string_lossy().to_string();
    st.settings.save();
    st.scanner.clear();
    st.prepare_root();
    st.load_org();
    st.reload();
    library_view(st)
}

fn library_view(st: &mut State) -> CmdResult {
    let tags: Vec<Value> = st.tag_counts().into_iter().map(|(t, c)| json!({"tag": t, "count": c})).collect();
    let views: Vec<Value> = st
        .org
        .smart_views
        .iter()
        .map(|v| json!({"id": v.id, "name": v.name, "summary": v.filter.summary(), "filter": v.filter, "count": st.matching(&v.filter, st.notes.iter().collect()).len()}))
        .collect();
    Ok(json!({
        "root": st.root,
        "folders": st.ordered_folders(&st.folders, ""),
        "tags": tags,
        "smartViews": views,
        "sortMode": st.org.sort_mode,
        "total": st.notes.len(),
        "pinned": st.notes.iter().filter(|n| n.meta.pinned).count(),
        "favorites": st.org.favorites.iter().filter(|f| st.notes.iter().any(|n| &n.meta.id == *f)).count(),
        "trash": LibraryTrash::new(&st.root).items().len(),
        "damaged": st.damaged.len(),
        "templates": st.templates.len(),
        "skippedLinks": st.skipped_links,
        "error": st.last_error.take(),
    }))
}

fn date_group(unix: f64) -> String {
    let now = chrono::Local::now();
    let d = chrono::DateTime::from_timestamp(unix as i64, 0).unwrap_or_default().with_timezone(&chrono::Local);
    let days = (now.date_naive() - d.date_naive()).num_days();
    match days {
        0 => "Heute".into(),
        1 => "Gestern".into(),
        2..=6 => "Letzte 7 Tage".into(),
        7..=29 => "Letzte 30 Tage".into(),
        _ => {
            use chrono::Datelike;
            if d.year() == now.year() {
                text::format_date(&d, "MMMM")
            } else {
                format!("{}", d.year())
            }
        }
    }
}

fn notes_list(st: &mut State, a: &Value) -> CmdResult {
    let selection = a.get("selection").cloned().unwrap_or(json!({"type": "all"}));
    let query = opt_str(a, "query").unwrap_or_default();
    let menu: SearchFilter = a.get("filter").and_then(|f| serde_json::from_value(f.clone()).ok()).unwrap_or_default();
    let search = SearchFilter::parse(&query).merged(&menu);
    let everywhere = st.settings.search_everywhere;
    let is_templates = selection.get("type").and_then(|t| t.as_str()) == Some("templates");
    let base: Vec<&NoteRef> = if !search.is_empty() && everywhere && !is_templates { st.notes.iter().collect() } else { st.selection_notes(&selection) };
    let list = if search.is_empty() { base } else { st.matching(&search, base) };
    let key = State::order_key(&selection);
    let sorted = if is_templates { list } else { st.sorted(list, key.as_deref()) };
    let mode = st.org.sort_mode;
    let out: Vec<Value> = sorted
        .iter()
        .map(|n| {
            let mut v = st.summary(n);
            if mode.groups_by_date() && !n.meta.pinned {
                let d = if mode == SortMode::Created { n.meta.created } else { n.meta.modified };
                v["group"] = json!(date_group(model::to_unix(d)));
            } else if n.meta.pinned {
                v["group"] = json!("Angeheftet");
            }
            if !search.text.is_empty() {
                v["snippet"] = json!(snippet(&n.meta.search_text, &search.text));
            }
            v
        })
        .collect();
    Ok(json!({"notes": out, "search": search, "searchSummary": if search.is_empty() { String::new() } else { search.summary() }}))
}

fn snippet(text: &str, query: &str) -> String {
    let words: Vec<String> = text::fold(query).split(' ').filter(|w| !w.is_empty()).map(|s| s.to_string()).collect();
    let Some(first) = words.first() else { return String::new() };
    let r = text::find_ranges(first, text, false, false);
    let Some(&(s, e)) = r.first() else { return String::new() };
    let start = text[..s].char_indices().rev().nth(40).map(|(i, _)| i).unwrap_or(0);
    let end = text[e..].char_indices().nth(80).map(|(i, _)| e + i).unwrap_or(text.len());
    let mut out = text[start..end].replace('\n', " ");
    if start > 0 {
        out = format!("…{out}");
    }
    if end < text.len() {
        out.push('…');
    }
    out
}

fn note_open(st: &mut State, a: &Value) -> CmdResult {
    let id = arg_str(a, "id")?.to_string();
    let path = st.path_of(&id)?;
    if !st.docs.contains_key(&id) {
        let file = NoteFile::read(&path).map_err(|e| format!("Die Notiz lässt sich nicht öffnen: {e}"))?;
        st.docs.insert(id.clone(), OpenDoc { fingerprint: Fingerprint::of(&path), path: path.clone(), file, last_version: 0.0 });
    }
    st.settings.recent.retain(|r| r != &id);
    st.settings.recent.insert(0, id.clone());
    st.settings.recent.truncate(30);
    st.settings.save();
    let size = st.settings.editor_font_size;
    let doc = st.docs.get(&id).unwrap();
    let mut blocks: Vec<Value> = doc.file.body.blocks.iter().map(|b| block_to_ui(b, size)).collect();
    // A note always starts and ends with a text block, like in the Swift app.
    let empty_text = || json!({"id": folio_core::new_id(), "kind": "text", "rich": {"paras": [{"runs": []}]}});
    if blocks.first().and_then(|b| b["kind"].as_str()) != Some("text") {
        blocks.insert(0, empty_text());
    }
    if blocks.last().and_then(|b| b["kind"].as_str()) != Some("text") {
        blocks.push(empty_text());
    }
    let atts: Vec<Value> = doc.file.attachments.iter().map(|x| json!({"id": x.header.id, "filename": x.header.filename, "type": x.header.type_identifier, "size": x.data.len(), "isPackage": x.header.is_package})).collect();
    let meta = doc.file.meta.clone();
    let path = doc.path.clone();
    Ok(json!({
        "id": id,
        "path": path,
        "folder": st.rel_folder(&path),
        "meta": meta,
        "created": model::to_unix(meta.created),
        "modified": model::to_unix(meta.modified),
        "blocks": blocks,
        "attachments": atts,
        "isTemplate": st.is_template(&path),
        "favorite": st.org.favorites.contains(&id),
    }))
}

fn note_save(st: &mut State, a: &Value) -> CmdResult {
    let id = arg_str(a, "id")?.to_string();
    let settings = st.settings.clone();
    let Some(doc) = st.docs.get_mut(&id) else { return Err("Die Notiz ist nicht geöffnet.".into()) };
    // Never overwrite a change made outside Folio since the note was loaded or saved.
    if !a["force"].as_bool().unwrap_or(false) {
        if let (Some(expected), Some(current)) = (doc.fingerprint, Fingerprint::of(&doc.path)) {
            if expected != current {
                return Ok(json!({"conflict": true}));
            }
        }
    }
    if let Some(blocks) = a.get("blocks").and_then(|b| b.as_array()) {
        let old: HashMap<String, BlockRecord> = doc.file.body.blocks.iter().map(|b| (b.id.clone(), b.clone())).collect();
        let mut new_blocks = vec![];
        for v in blocks {
            let bid = v.get("id").and_then(|i| i.as_str()).unwrap_or("");
            new_blocks.push(block_from_ui(v, old.get(bid), &settings)?);
        }
        // Empty leading/trailing text blocks are editor scaffolding; keep the file like the Swift app writes it.
        doc.file.body.blocks = new_blocks;
    }
    if let Some(p) = a.get("properties") {
        doc.file.meta.properties = if p.as_array().map(|x| x.is_empty()).unwrap_or(true) { None } else { serde_json::from_value(p.clone()).ok() };
    }
    if let Some(env) = a.get("environment") {
        doc.file.meta.environment = if env.is_null() { None } else { Some(env.clone()) };
    }
    if let Some(p) = a["pinned"].as_bool() {
        doc.file.meta.pinned = p;
    }
    note::refresh_meta(&mut doc.file, !a["keepModified"].as_bool().unwrap_or(false));
    note::prune_attachments(&mut doc.file);
    let data = doc.file.write(&doc.path).map_err(|e| format!("Speichern fehlgeschlagen: {e}"))?;
    doc.fingerprint = Fingerprint::of(&doc.path);
    let now = model::now();
    let force_version = now - doc.last_version > 300.0;
    if force_version {
        doc.last_version = now;
    }
    let meta = doc.file.meta.clone();
    let path = doc.path.clone();
    st.record_version(data, id.clone(), false);
    st.upsert(&path, &meta);
    st.rename_to_title(&id);
    let path = st.docs.get(&id).map(|d| d.path.clone()).unwrap_or(path);
    Ok(json!({"meta": meta, "path": path, "summary": st.find(&id).map(|n| st.summary(n))}))
}

fn note_check_external(st: &mut State, _a: &Value) -> CmdResult {
    // Open notes changed by another app or a sync service.
    let mut changed = vec![];
    for (id, d) in &st.docs {
        let now = Fingerprint::of(&d.path);
        if now.is_some() && now != d.fingerprint {
            changed.push(id.clone());
        } else if now.is_none() {
            changed.push(id.clone());
        }
    }
    // The library itself: new or removed files.
    Ok(json!({"changed": changed}))
}

fn note_resolve_conflict(st: &mut State, a: &Value) -> CmdResult {
    let id = arg_str(a, "id")?.to_string();
    let choice = arg_str(a, "choice")?; // "mine", "theirs", "both"
    let Some(doc) = st.docs.get(&id) else { return Err("Die Notiz ist nicht geöffnet.".into()) };
    let path = doc.path.clone();
    match choice {
        "theirs" => {
            // Our version goes into the history first, so nothing is lost.
            if let Ok(data) = doc.file.encode() {
                st.record_version(data, id.clone(), true);
            }
            st.docs.remove(&id);
            Ok(json!({"reopen": true}))
        }
        "both" => {
            let mine = model::renewed(&doc.file);
            let mut mine = mine;
            mine.meta.title = format!("{} (meine Version)", doc.file.meta.title);
            let dir = path.parent().unwrap_or(&st.root).to_path_buf();
            st.write_new(&mine, &dir)?;
            st.docs.remove(&id);
            st.reload();
            Ok(json!({"reopen": true, "copy": mine.meta.id}))
        }
        _ => {
            if let Ok(theirs) = std::fs::read(&path) {
                st.record_version(theirs, id.clone(), true);
            }
            if let Some(d) = st.docs.get_mut(&id) {
                d.fingerprint = Fingerprint::of(&path);
            }
            Ok(json!({"reopen": false}))
        }
    }
}

fn new_note_file(st: &State, initial_tag: Option<&str>, title: Option<&str>, body: Option<&str>) -> NoteFile {
    let mut meta = NoteMetadata::titled(title.unwrap_or("Neue Notiz"));
    meta.title = title.unwrap_or("Neue Notiz").to_string();
    let mut blocks = vec![];
    let mut lines = vec![];
    if let Some(t) = title {
        lines.push(t.to_string());
    }
    if let Some(b) = body {
        lines.push(b.to_string());
    }
    if let Some(tag) = initial_tag {
        lines.push(String::new());
        lines.push(format!("#{tag}"));
    }
    if !lines.is_empty() {
        let mut b = BlockRecord::new(kind::TEXT);
        let mut rt = folio_core::rtf::RichText::from_plain(&lines.join("\n"));
        if title.is_some() {
            for r in rt.paras[0].runs.iter_mut() {
                r.b = true;
                r.size = Some((st.settings.editor_font_size * 1.85 * 2.0).round() / 2.0);
            }
        }
        b.set_rtf(&folio_core::rtf::write(&rt, &st.settings.write_options()));
        blocks.push(b);
    }
    let mut f = NoteFile::new(meta, blocks);
    if title.is_some() || body.is_some() {
        note::refresh_meta(&mut f, true);
    }
    f
}

fn note_create(st: &mut State, a: &Value) -> CmdResult {
    if let Some(tid) = opt_str(a, "template") {
        return note_from_template(st, &tid, opt_str(a, "folder").as_deref(), opt_str(a, "title").as_deref());
    }
    let dir = st.target_folder(opt_str(a, "folder").as_deref());
    let f = new_note_file(st, opt_str(a, "tag").as_deref(), opt_str(a, "title").as_deref(), opt_str(a, "text").as_deref());
    let p = st.write_new(&f, &dir)?;
    Ok(json!({"id": f.meta.id, "path": p}))
}

fn note_from_template(st: &mut State, tid: &str, folder: Option<&str>, title: Option<&str>) -> CmdResult {
    let tp = st.path_of(tid)?;
    let mut file = NoteFile::read(&tp).map_err(|e| format!("Die Vorlage ist nicht lesbar: {e}"))?;
    let now = chrono::Local::now();
    let ctx_title = title.map(|t| t.to_string()).unwrap_or_else(|| text::fill_template(&file.meta.title, &now, "").0);
    let mut cursor: Option<(String, usize)> = None;
    let opts = st.settings.write_options();
    let size = st.settings.editor_font_size;
    for b in file.body.blocks.iter_mut() {
        b.id = folio_core::new_id();
        if b.kind == kind::TEXT {
            let mut rt = folio_core::rtf::parse_for_editor(&b.rtf_bytes().unwrap_or_default(), size);
            let mut offset = 0usize;
            for p in rt.paras.iter_mut() {
                for r in p.runs.iter_mut() {
                    let (t, c) = text::fill_template(&r.text, &now, &ctx_title);
                    if let (Some(c), None) = (c, &cursor) {
                        cursor = Some((b.id.clone(), offset + t[..c].chars().count()));
                    }
                    offset += t.chars().count();
                    r.text = t;
                }
                offset += 1;
            }
            b.set_rtf(&folio_core::rtf::write(&rt, &opts));
        } else if let Some(c) = &b.code {
            b.code = Some(text::fill_template(c, &now, &ctx_title).0);
        }
    }
    file = model::renewed(&file);
    let t = model::now();
    file.meta.created = t;
    file.meta.modified = t;
    file.meta.pinned = false;
    file.meta.title = title.map(|s| s.to_string()).unwrap_or_else(|| text::fill_template(&file.meta.title, &now, &ctx_title).0);
    note::refresh_meta(&mut file, true);
    let dir = st.target_folder(folder);
    let p = st.write_new(&file, &dir)?;
    Ok(json!({"id": file.meta.id, "path": p, "cursor": cursor.map(|(b, o)| json!({"block": b, "offset": o}))}))
}

fn note_duplicate(st: &mut State, a: &Value) -> CmdResult {
    let id = arg_str(a, "id")?;
    let p = st.path_of(id)?;
    let f = match st.docs.get(id) {
        Some(d) => d.file.clone(),
        None => NoteFile::read(&p).map_err(|e| e.to_string())?,
    };
    let c = note::duplicated(&f);
    let dir = p.parent().unwrap_or(&st.root).to_path_buf();
    let np = st.write_new(&c, &dir)?;
    Ok(json!({"id": c.meta.id, "path": np}))
}

fn note_delete(st: &mut State, a: &Value) -> CmdResult {
    let id = arg_str(a, "id")?.to_string();
    let p = st.path_of(&id)?;
    if st.is_template(&p) {
        let _ = std::fs::remove_file(&p);
        st.docs.remove(&id);
        st.templates = st.scan_templates();
        return Ok(json!(true));
    }
    // A version before it goes, so the trash and the history both have it.
    if let Ok(data) = std::fs::read(&p) {
        let _ = NoteHistory::new(&st.root).record(&data, &id, model::now(), true);
    }
    let title = st.find(&id).map(|n| n.meta.title.clone());
    LibraryTrash::new(&st.root).move_to_trash(&p, title.as_deref())?;
    st.docs.remove(&id);
    st.notes.retain(|n| n.meta.id != id);
    st.links.remove(&id);
    Ok(json!(true))
}

fn note_move(st: &mut State, a: &Value) -> CmdResult {
    let id = arg_str(a, "id")?.to_string();
    let folder = arg_str(a, "folder")?;
    let p = st.path_of(&id)?;
    let dir = st.abs_folder(folder);
    if p.parent() == Some(dir.as_path()) {
        return Ok(json!(true));
    }
    let target = library::unique_like(&dir.join(p.file_name().unwrap()));
    std::fs::rename(&p, &target).map_err(|e| format!("Verschieben fehlgeschlagen: {e}"))?;
    if let Some(d) = st.docs.get_mut(&id) {
        d.path = target.clone();
        d.fingerprint = Fingerprint::of(&target);
    }
    if let Some(n) = st.notes.iter_mut().find(|n| n.meta.id == id) {
        n.path = target.clone();
    }
    st.rebuild_links();
    Ok(json!({"path": target}))
}

fn note_pin(st: &mut State, a: &Value) -> CmdResult {
    let id = arg_str(a, "id")?.to_string();
    let pinned = a["pinned"].as_bool();
    let p = st.path_of(&id)?;
    let mut f = match st.docs.get(&id) {
        Some(d) => d.file.clone(),
        None => NoteFile::read(&p).map_err(|e| e.to_string())?,
    };
    f.meta.pinned = pinned.unwrap_or(!f.meta.pinned);
    f.write(&p).map_err(|e| e.to_string())?;
    if let Some(d) = st.docs.get_mut(&id) {
        d.file.meta.pinned = f.meta.pinned;
        d.fingerprint = Fingerprint::of(&p);
    }
    st.upsert(&p, &f.meta);
    Ok(json!(f.meta.pinned))
}

fn doc_file(st: &State, id: &str) -> Result<NoteFile, String> {
    match st.docs.get(id) {
        Some(d) => Ok(d.file.clone()),
        None => NoteFile::read(&st.path_of(id)?).map_err(|e| e.to_string()),
    }
}

fn note_info(st: &mut State, a: &Value) -> CmdResult {
    let id = arg_str(a, "id")?;
    let f = doc_file(st, id)?;
    let p = st.path_of(id)?;
    let texts: Vec<String> = f.body.blocks.iter().filter(|b| b.kind == kind::TEXT).map(|b| b.text()).collect();
    let code: Vec<String> = f.body.blocks.iter().filter(|b| b.kind == kind::CODE).filter_map(|b| b.code.clone()).collect();
    let stats = text::statistics(&texts, &code);
    let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
    Ok(json!({
        "stats": stats,
        "created": model::to_unix(f.meta.created),
        "modified": model::to_unix(f.meta.modified),
        "path": p,
        "size": size,
        "attachments": f.attachments.len(),
        "blocks": f.body.blocks.len(),
        "tags": f.meta.tags,
    }))
}

fn note_links(st: &mut State, a: &Value) -> CmdResult {
    let id = arg_str(a, "id")?;
    let mut back: Vec<Value> = st.links.backlinks(id).iter().filter_map(|b| st.find(b)).map(|n| st.summary(n)).collect();
    back.sort_by(|x, y| x["title"].as_str().cmp(&y["title"].as_str()));
    let mut out: Vec<Value> = st.links.outgoing(id).iter().filter_map(|b| st.find(b)).map(|n| st.summary(n)).collect();
    out.sort_by(|x, y| x["title"].as_str().cmp(&y["title"].as_str()));
    let title = st.find(id).map(|n| n.meta.title.clone()).unwrap_or_default();
    let linking = st.links.backlinks(id);
    let mentions: Vec<Value> = st.notes.iter().filter(|n| n.meta.id != id && !linking.contains(&n.meta.id) && text::mentions(&title, &n.meta.search_text) > 0).take(30).map(|n| st.summary(n)).collect();
    Ok(json!({"backlinks": back, "outgoing": out, "unresolved": st.links.unresolved(id), "mentions": mentions}))
}

fn note_by_title(st: &mut State, a: &Value) -> CmdResult {
    let title = arg_str(a, "title")?;
    if let Some(id) = st.links.resolve(title) {
        return Ok(json!({"id": id, "created": false}));
    }
    if !a["create"].as_bool().unwrap_or(false) {
        return Ok(json!({"id": null}));
    }
    let dir = st.target_folder(opt_str(a, "folder").as_deref());
    let f = new_note_file(st, None, Some(title), None);
    st.write_new(&f, &dir)?;
    Ok(json!({"id": f.meta.id, "created": true}))
}

/// Another note, read-only, for an embed block (#120).
fn note_embed(st: &mut State, a: &Value) -> CmdResult {
    let title = arg_str(a, "title")?;
    let Some(id) = st.links.resolve(title) else { return Ok(json!({"missing": true})) };
    let f = doc_file(st, &id)?;
    let size = st.settings.editor_font_size;
    let blocks: Vec<Value> = f.body.blocks.iter().filter(|b| b.kind != kind::EMBED).map(|b| block_to_ui(b, size)).collect();
    Ok(json!({"id": id, "title": f.meta.title, "blocks": blocks}))
}

/// Turns the first unlinked mention of `target`'s title in `source` into a [[link]] (#112).
fn link_mention(st: &mut State, a: &Value) -> CmdResult {
    let source = arg_str(a, "source")?.to_string();
    let target = arg_str(a, "target")?;
    let title = st.find(target).map(|n| n.meta.title.clone()).ok_or("Notiz fehlt.")?;
    let p = st.path_of(&source)?;
    let mut f = doc_file(st, &source)?;
    let opts = st.settings.write_options();
    let size = st.settings.editor_font_size;
    let mut done = false;
    for b in f.body.blocks.iter_mut().filter(|b| b.kind == kind::TEXT) {
        let mut rt = folio_core::rtf::parse_for_editor(&b.rtf_bytes().unwrap_or_default(), size);
        for p in rt.paras.iter_mut() {
            for r in p.runs.iter_mut() {
                if done || r.link.is_some() {
                    continue;
                }
                let ranges = text::find_ranges(&title, &r.text, false, true);
                if let Some(&(s, e)) = ranges.first() {
                    r.text = format!("{}[[{}]]{}", &r.text[..s], &r.text[s..e], &r.text[e..]);
                    done = true;
                }
            }
        }
        if done {
            b.set_rtf(&folio_core::rtf::write(&rt, &opts));
            break;
        }
    }
    if !done {
        return Ok(json!(false));
    }
    note::refresh_meta(&mut f, true);
    f.write(&p).map_err(|e| e.to_string())?;
    if let Some(d) = st.docs.get_mut(&source) {
        d.file = f.clone();
        d.fingerprint = Fingerprint::of(&p);
    }
    st.upsert(&p, &f.meta);
    Ok(json!(true))
}

fn folder_create(st: &mut State, a: &Value) -> CmdResult {
    let name = text::sanitize_filename(arg_str(a, "name")?);
    let parent = st.abs_folder(&opt_str(a, "parent").unwrap_or_default());
    let dir = parent.join(&name);
    std::fs::create_dir(&dir).map_err(|e| format!("Ordner konnte nicht angelegt werden: {e}"))?;
    st.reload();
    Ok(json!(st.rel_folder(&dir.join("x"))))
}

fn folder_rename(st: &mut State, a: &Value) -> CmdResult {
    let rel = arg_str(a, "path")?.to_string();
    let name = text::sanitize_filename(arg_str(a, "name")?);
    let src = st.abs_folder(&rel);
    let target = src.parent().unwrap_or(&st.root).join(&name);
    if target == src {
        return Ok(json!(rel));
    }
    if target.exists() {
        return Err(format!("„{name}“ gibt es dort schon."));
    }
    std::fs::rename(&src, &target).map_err(|e| format!("Umbenennen fehlgeschlagen: {e}"))?;
    let new_rel = st.rel_folder(&target.join("x"));
    // Orders and smart views follow the folder.
    let rename = |p: &str| -> String {
        if p == rel { new_rel.clone() } else if let Some(r) = p.strip_prefix(&format!("{rel}/")) { format!("{new_rel}/{r}") } else { p.to_string() }
    };
    let old_name = Path::new(&rel).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    st.org.folder_order = st.org.folder_order.iter().map(|(k, v)| (rename(k), v.iter().map(|n| if *n == old_name { name.clone() } else { n.clone() }).collect())).collect();
    st.org.note_order = st.org.note_order.iter().map(|(k, v)| (k.strip_prefix("folder:").map(|f| format!("folder:{}", rename(f))).unwrap_or(k.clone()), v.clone())).collect();
    for sv in st.org.smart_views.iter_mut() {
        if let Some(f) = &sv.filter.folder {
            sv.filter.folder = Some(rename(f));
        }
    }
    st.save_org();
    st.reload();
    Ok(json!(new_rel))
}

fn folder_delete(st: &mut State, a: &Value) -> CmdResult {
    let rel = arg_str(a, "path")?;
    if rel.is_empty() {
        return Err("Die Bibliothek selbst kann nicht gelöscht werden.".into());
    }
    let dir = st.abs_folder(rel);
    let inside: Vec<String> = st.docs.iter().filter(|(_, d)| d.path.starts_with(&dir)).map(|(id, _)| id.clone()).collect();
    LibraryTrash::new(&st.root).move_to_trash(&dir, None)?;
    for id in inside {
        st.docs.remove(&id);
    }
    st.reload();
    Ok(json!(true))
}

fn folder_move_order(st: &mut State, a: &Value) -> CmdResult {
    let parent = opt_str(a, "parent").unwrap_or_default();
    let name = arg_str(a, "name")?.to_string();
    let before = opt_str(a, "before");
    let visible: Vec<String> = a["visible"].as_array().map(|v| v.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    let order = manual_order::moving(&name, before.as_ref(), &visible);
    st.org.folder_order.insert(parent, order);
    st.save_org();
    Ok(json!(true))
}

fn note_move_order(st: &mut State, a: &Value) -> CmdResult {
    let selection = a.get("selection").cloned().unwrap_or(json!({"type": "all"}));
    let Some(key) = State::order_key(&selection) else { return Ok(json!(false)) };
    let id = arg_str(a, "id")?.to_string();
    let visible: Vec<String> = a["visible"].as_array().map(|v| v.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    let moved = match a.get("delta").and_then(|d| d.as_i64()) {
        Some(d) => manual_order::stepping(&id, d, &visible),
        None => manual_order::moving(&id, opt_str(a, "before").as_ref(), &visible),
    };
    // The whole list of this place, in its current order, gets the new order of the visible part (#170).
    let full: Vec<String> = st.sorted(st.selection_notes(&selection), Some(&key)).iter().map(|n| n.meta.id.clone()).collect();
    let merged = manual_order::merging(&moved, &full);
    st.org.note_order.insert(key, merged);
    st.org.sort_mode = SortMode::Manual;
    st.save_org();
    Ok(json!(true))
}

fn smart_view_save(st: &mut State, a: &Value) -> CmdResult {
    let name = arg_str(a, "name")?.to_string();
    let query = opt_str(a, "query").unwrap_or_default();
    let menu: SearchFilter = a.get("filter").and_then(|f| serde_json::from_value(f.clone()).ok()).unwrap_or_default();
    let filter = SearchFilter::parse(&query).merged(&menu);
    if let Some(id) = opt_str(a, "id") {
        if let Some(v) = st.org.smart_views.iter_mut().find(|v| v.id == id) {
            v.filter = filter;
            v.name = name;
        }
        st.save_org();
        return Ok(json!(id));
    }
    let v = SmartView { id: folio_core::new_id(), name, filter };
    let id = v.id.clone();
    st.org.smart_views.push(v);
    st.save_org();
    Ok(json!(id))
}

/// Renames or removes a tag in every note (#18); nested tags follow.
fn tag_edit(st: &mut State, a: &Value) -> CmdResult {
    let tag = arg_str(a, "tag")?.to_lowercase();
    let new = match opt_str(a, "to") {
        Some(t) => Some(text::normalized_tag(&t).ok_or("Ungültiger Tag-Name.")?),
        None => None,
    };
    let opts = st.settings.write_options();
    let size = st.settings.editor_font_size;
    let affected: Vec<(String, PathBuf)> = st.notes.iter().filter(|n| n.meta.tags.iter().any(|t| *t == tag || t.starts_with(&format!("{tag}/")))).map(|n| (n.meta.id.clone(), n.path.clone())).collect();
    let mut changed = 0;
    for (id, path) in affected {
        let mut f = match st.docs.get(&id) {
            Some(d) => d.file.clone(),
            None => match NoteFile::read(&path) {
                Ok(f) => f,
                Err(_) => continue,
            },
        };
        let mut any = false;
        for b in f.body.blocks.iter_mut().filter(|b| b.kind == kind::TEXT) {
            let mut rt = folio_core::rtf::parse_for_editor(&b.rtf_bytes().unwrap_or_default(), size);
            let mut touched = false;
            for p in rt.paras.iter_mut() {
                for r in p.runs.iter_mut() {
                    let n = text::rename_tag(&r.text, &tag, new.as_deref());
                    if n != r.text {
                        r.text = n;
                        touched = true;
                    }
                }
            }
            if touched {
                b.set_rtf(&folio_core::rtf::write(&rt, &opts));
                any = true;
            }
        }
        if !any {
            continue;
        }
        note::refresh_meta(&mut f, true);
        if let Ok(data) = f.write(&path) {
            st.record_version(data, id.clone(), true);
            if let Some(d) = st.docs.get_mut(&id) {
                d.file = f.clone();
                d.fingerprint = Fingerprint::of(&path);
            }
            st.upsert(&path, &f.meta);
            changed += 1;
        }
    }
    st.org.rename_tag(&tag, new.as_deref());
    st.save_org();
    Ok(json!(changed))
}

fn trash_find(st: &State, a: &Value) -> Result<library::TrashItem, String> {
    let id = arg_str(a, "id")?;
    LibraryTrash::new(&st.root).items().into_iter().find(|i| i.id == id).ok_or_else(|| "Nicht mehr im Papierkorb.".to_string())
}

fn trash_restore(st: &mut State, a: &Value) -> CmdResult {
    let item = trash_find(st, a)?;
    let p = LibraryTrash::new(&st.root).restore(&item)?;
    st.reload();
    Ok(json!({"path": p, "ids": item.note_ids}))
}

fn trash_delete(st: &mut State, a: &Value) -> CmdResult {
    let item = trash_find(st, a)?;
    LibraryTrash::new(&st.root).delete_permanently(&item, Some(&NoteHistory::new(&st.root)))?;
    Ok(json!(true))
}

fn history_list(st: &mut State, a: &Value) -> CmdResult {
    let id = arg_str(a, "id")?;
    Ok(json!(NoteHistory::new(&st.root).versions(id).iter().map(|v| json!({"path": v.path, "date": model::to_unix(v.date), "size": v.size})).collect::<Vec<_>>()))
}

fn history_preview(st: &mut State, a: &Value) -> CmdResult {
    let f = NoteFile::read(Path::new(arg_str(a, "path")?)).map_err(|e| e.to_string())?;
    let size = st.settings.editor_font_size;
    Ok(json!({"title": f.meta.title, "blocks": f.body.blocks.iter().map(|b| block_to_ui(b, size)).collect::<Vec<_>>()}))
}

fn history_restore(st: &mut State, a: &Value) -> CmdResult {
    let id = arg_str(a, "id")?.to_string();
    let vp = PathBuf::from(arg_str(a, "path")?);
    let p = st.path_of(&id)?;
    let mut f = NoteFile::read(&vp).map_err(|e| e.to_string())?;
    // The current state becomes a version too, so restoring can be undone.
    if let Ok(cur) = std::fs::read(&p) {
        let _ = NoteHistory::new(&st.root).record(&cur, &id, model::now(), true);
    }
    f.meta.id = id.clone();
    f.meta.modified = model::now();
    f.write(&p).map_err(|e| e.to_string())?;
    st.docs.remove(&id);
    st.upsert(&p, &f.meta);
    Ok(json!(true))
}

fn backup_now(st: &mut State) -> CmdResult {
    let dir = st.settings.backup_dir();
    let b = backup::create(&st.root, &dir, model::now())?;
    backup::prune(&dir, model::now());
    st.settings.last_backup = model::now();
    st.settings.save();
    Ok(json!({"path": b.path, "size": b.size}))
}

fn backup_due(st: &mut State) -> CmdResult {
    if !st.settings.backups_enabled {
        return Ok(json!(false));
    }
    let last = backup::list(&st.settings.backup_dir()).first().map(|b| b.date).unwrap_or(0.0).max(st.settings.last_backup);
    if model::now() - last < st.settings.backup_interval_hours.max(1.0) * 3600.0 {
        return Ok(json!(false));
    }
    let root = st.root.clone();
    let dir = st.settings.backup_dir();
    st.settings.last_backup = model::now();
    st.settings.save();
    std::thread::spawn(move || {
        if backup::create(&root, &dir, model::now()).is_ok() {
            backup::prune(&dir, model::now());
        }
    });
    Ok(json!(true))
}

const STARTERS: &[(&str, &str)] = &[
    ("Meeting", "# Meeting {{datum}}\n{{wochentag}}, {{uhrzeit}} · #meeting\n## Teilnehmende\n• \n## Themen\n1. \n## Entscheidungen\n• \n## Aufgaben\n☐ "),
    ("Lernnotiz", "# Lernnotiz: Thema\n{{datum}} · #lernen\n## Ziel\nWas will ich verstehen?\n## Notizen\n• \n## Fragen\n• \n## Zusammenfassung in drei Sätzen"),
    ("Projekt", "# Projekt: Name\nGestartet am {{datum}} · #projekt\n## Ziel\n## Meilensteine\n☐ \n## Offene Punkte\n• \n## Links & Ressourcen"),
];

fn ensure_starter_templates(st: &mut State) {
    let dir = library::data_folder(&st.root, "templates");
    if dir.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(&dir);
    let size = st.settings.editor_font_size;
    for (name, body) in STARTERS {
        let paras = body
            .split('\n')
            .map(|line| {
                let (text, scale) = if let Some(r) = line.strip_prefix("## ") { (r, Some(1.15)) } else if let Some(r) = line.strip_prefix("# ") { (r, Some(1.85)) } else { (line, None) };
                let mut p = folio_core::rtf::Para::plain(text);
                if let Some(s) = scale {
                    for r in p.runs.iter_mut() {
                        r.b = true;
                        r.size = Some((size * s * 2.0).round() / 2.0);
                    }
                }
                p
            })
            .collect();
        let mut b = BlockRecord::new(kind::TEXT);
        b.set_rtf(&folio_core::rtf::write(&folio_core::rtf::RichText { paras }, &st.settings.write_options()));
        let f = NoteFile::new(NoteMetadata::titled(name), vec![b]);
        let _ = f.write(&library::unique_path(&dir, name, "folio"));
    }
    st.templates = st.scan_templates();
}

fn template_from_note(st: &mut State, a: &Value) -> CmdResult {
    let id = arg_str(a, "id")?;
    let mut f = model::renewed(&doc_file(st, id)?);
    f.meta.pinned = false;
    if f.meta.title.is_empty() {
        f.meta.title = "Vorlage".into();
    }
    let dir = library::data_folder(&st.root, "templates");
    let p = st.write_new(&f, &dir)?;
    st.templates = st.scan_templates();
    Ok(json!({"id": f.meta.id, "path": p}))
}

fn template_new(st: &mut State) -> CmdResult {
    let mut b = BlockRecord::new(kind::TEXT);
    b.set_rtf(&folio_core::rtf::from_plain("Neue Vorlage\n{{datum}}\n", &st.settings.write_options()));
    let f = NoteFile::new(NoteMetadata::titled("Neue Vorlage"), vec![b]);
    let dir = library::data_folder(&st.root, "templates");
    st.write_new(&f, &dir)?;
    st.templates = st.scan_templates();
    Ok(json!({"id": f.meta.id}))
}

/// Today's note („2026-10-21“) in the daily folder, from the daily template when set (#117, #208).
fn daily_note(st: &mut State, a: &Value) -> CmdResult {
    let date = opt_str(a, "date").and_then(|d| chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()).unwrap_or_else(|| chrono::Local::now().date_naive());
    let title = text::daily_title(date);
    if let Some(id) = st.org.daily_notes.get(&title).cloned() {
        if st.find(&id).is_some() {
            return Ok(json!({"id": id, "created": false}));
        }
    }
    if let Some(id) = st.links.resolve(&title) {
        st.org.daily_notes.insert(title.clone(), id.clone());
        st.save_org();
        return Ok(json!({"id": id, "created": false}));
    }
    let folder = st.settings.daily_folder.clone();
    let dir = st.root.join(if folder.trim().is_empty() { "Tagesnotizen" } else { folder.trim() });
    let _ = std::fs::create_dir_all(&dir);
    let rel = st.rel_folder(&dir.join("x"));
    let tmpl = st.settings.daily_template.clone();
    let id = if let Some(t) = st.templates.iter().find(|t| t.meta.title == tmpl || t.meta.id == tmpl).map(|t| t.meta.id.clone()) {
        let r = note_from_template(st, &t, Some(&rel), Some(&title))?;
        r["id"].as_str().unwrap_or_default().to_string()
    } else {
        let f = new_note_file(st, None, Some(&title), None);
        st.write_new(&f, &dir)?;
        f.meta.id
    };
    st.org.daily_notes.insert(title, id.clone());
    st.save_org();
    Ok(json!({"id": id, "created": true}))
}

fn switcher(st: &mut State, a: &Value) -> CmdResult {
    let q = opt_str(a, "query").unwrap_or_default();
    let mut entries: Vec<SwitcherEntry> = st
        .notes
        .iter()
        .map(|n| SwitcherEntry { kind: "note".into(), title: if n.meta.title.is_empty() { "Ohne Titel".into() } else { n.meta.title.clone() }, detail: st.rel_folder(&n.path), key: n.meta.id.clone() })
        .collect();
    // Recently opened first when nothing is typed.
    let rank: HashMap<&String, usize> = st.settings.recent.iter().enumerate().map(|(i, id)| (id, i)).collect();
    entries.sort_by_key(|e| rank.get(&e.key).copied().unwrap_or(usize::MAX));
    fn walk(f: &[library::Folder], st: &State, out: &mut Vec<SwitcherEntry>) {
        for x in f {
            let rel = st.rel_folder(&x.path.join("x"));
            out.push(SwitcherEntry { kind: "folder".into(), title: x.name.clone(), detail: rel.clone(), key: rel });
            walk(&x.children, st, out);
        }
    }
    let mut folders = vec![];
    walk(&st.folders, st, &mut folders);
    entries.extend(folders);
    entries.extend(st.tag_counts().into_iter().map(|(t, c)| SwitcherEntry { kind: "tag".into(), title: format!("#{t}"), detail: format!("{c} Notizen"), key: t }));
    let ranked = folio_core::search::rank_switcher(entries, &q);
    Ok(json!(ranked.into_iter().take(60).collect::<Vec<_>>()))
}

fn graph(st: &mut State, a: &Value) -> CmdResult {
    let center = opt_str(a, "center");
    let depth = a["depth"].as_u64().unwrap_or(1) as usize;
    let with_folders = a["folders"].as_bool().unwrap_or(false);
    let only_orphans = a["orphans"].as_bool().unwrap_or(false);
    let tag = opt_str(a, "tag");
    let mut ids: Vec<String> = match &center {
        Some(c) => st.links.neighborhood(c, depth).into_iter().collect(),
        None => st.links.nodes.keys().cloned().collect(),
    };
    if only_orphans {
        let o = st.links.orphans();
        ids.retain(|i| o.contains(i));
    }
    if let Some(t) = &tag {
        ids.retain(|i| st.links.nodes.get(i).map(|n| n.tags.iter().any(|x| x == t || x.starts_with(&format!("{t}/")))).unwrap_or(false));
    }
    ids.sort();
    let set: std::collections::HashSet<&String> = ids.iter().collect();
    let mut edges: Vec<(String, String)> = st.links.edges().into_iter().filter(|(x, y)| set.contains(x) && set.contains(y)).collect();
    let mut folder_nodes = HashMap::new();
    if with_folders {
        let pairs: Vec<(String, String)> = ids.iter().filter_map(|i| st.links.nodes.get(i).map(|n| (i.clone(), n.folder.clone()))).collect();
        let (f, e) = folio_core::links::folder_graph(&pairs);
        folder_nodes = f;
        edges.extend(e);
    }
    let mut all = ids.clone();
    let mut fids: Vec<String> = folder_nodes.keys().cloned().collect();
    fids.sort();
    all.extend(fids);
    let pos = folio_core::links::layout(&all, &edges, &HashMap::new(), 300);
    let mut degree: HashMap<&String, usize> = HashMap::new();
    for (x, y) in &edges {
        *degree.entry(x).or_default() += 1;
        *degree.entry(y).or_default() += 1;
    }
    let nodes: Vec<Value> = all
        .iter()
        .map(|id| {
            let (x, y) = pos.get(id).copied().unwrap_or((0.0, 0.0));
            match st.links.nodes.get(id) {
                Some(n) => json!({"id": id, "title": n.title, "folder": n.folder, "tags": n.tags, "x": x, "y": y, "degree": degree.get(id).copied().unwrap_or(0), "kind": "note"}),
                None => json!({"id": id, "title": folder_nodes.get(id).cloned().unwrap_or_default(), "x": x, "y": y, "degree": degree.get(id).copied().unwrap_or(0), "kind": "folder"}),
            }
        })
        .collect();
    let path = match (opt_str(a, "from"), opt_str(a, "to")) {
        (Some(f), Some(t)) => st.links.path(&f, &t),
        _ => None,
    };
    Ok(json!({"nodes": nodes, "edges": edges, "path": path, "orphans": st.links.orphans().len()}))
}

fn type_for(name: &str) -> &'static str {
    match Path::new(name).extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref() {
        Some("png") => "public.png",
        Some("jpg" | "jpeg") => "public.jpeg",
        Some("gif") => "com.compuserve.gif",
        Some("heic") => "public.heic",
        Some("webp") => "org.webmproject.webp",
        Some("svg") => "public.svg-image",
        Some("tif" | "tiff") => "public.tiff",
        Some("pdf") => "com.adobe.pdf",
        Some("m4a") => "com.apple.m4a-audio",
        Some("mp3") => "public.mp3",
        Some("wav") => "com.microsoft.waveform-audio",
        Some("aiff" | "aif") => "public.aiff-audio",
        Some("webm") => "org.webmproject.webm",
        Some("mp4") => "public.mpeg-4",
        Some("mov") => "com.apple.quicktime-movie",
        Some("txt") => "public.plain-text",
        Some("md") => "net.daringfireball.markdown",
        Some("csv") => "public.comma-separated-values-text",
        Some("json") => "public.json",
        Some("numbers") => "com.apple.iwork.numbers.numbers",
        Some("pages") => "com.apple.iwork.pages.pages",
        Some("key") => "com.apple.iwork.keynote.key",
        Some("docx") => "org.openxmlformats.wordprocessingml.document",
        Some("xlsx") => "org.openxmlformats.spreadsheetml.sheet",
        Some("pptx") => "org.openxmlformats.presentationml.presentation",
        Some("zip") => "public.zip-archive",
        _ => "public.data",
    }
}

pub fn mime_for(type_id: &str, name: &str) -> &'static str {
    match type_id {
        "public.png" => "image/png",
        "public.jpeg" => "image/jpeg",
        "com.compuserve.gif" => "image/gif",
        "public.heic" => "image/heic",
        "org.webmproject.webp" => "image/webp",
        "public.svg-image" => "image/svg+xml",
        "public.tiff" => "image/tiff",
        "com.adobe.pdf" => "application/pdf",
        "com.apple.m4a-audio" => "audio/mp4",
        "public.mp3" => "audio/mpeg",
        "com.microsoft.waveform-audio" => "audio/wav",
        "public.aiff-audio" => "audio/aiff",
        "org.webmproject.webm" => "audio/webm",
        "public.mpeg-4" => "video/mp4",
        "com.apple.quicktime-movie" => "video/quicktime",
        "public.plain-text" | "net.daringfireball.markdown" | "public.comma-separated-values-text" => "text/plain; charset=utf-8",
        _ => {
            let t = type_for(name);
            if t != "public.data" && t != type_id { mime_for(t, "") } else { "application/octet-stream" }
        }
    }
}

/// Adds a file (by path) or data (base64) to an open note; returns the attachment and the block kind.
fn attachment_add(st: &mut State, a: &Value) -> CmdResult {
    use base64::Engine;
    let id = arg_str(a, "id")?;
    let (name, data) = if let Some(p) = opt_str(a, "path") {
        let pb = PathBuf::from(&p);
        if pb.is_dir() {
            return Err(format!("„{}“ ist ein Ordner oder ein Paket und kann so nicht eingebettet werden.", pb.file_name().unwrap_or_default().to_string_lossy()));
        }
        (pb.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or("Datei".into()), std::fs::read(&pb).map_err(|e| e.to_string())?)
    } else {
        let d = base64::engine::general_purpose::STANDARD.decode(arg_str(a, "data")?).map_err(|e| e.to_string())?;
        (opt_str(a, "filename").unwrap_or("Datei".into()), d)
    };
    let type_id = opt_str(a, "type").unwrap_or_else(|| type_for(&name).to_string());
    let att = folio_core::archive::Attachment::new(&name, &type_id, data);
    let block_kind = match type_id.as_str() {
        "com.adobe.pdf" => "pdf",
        t if t.starts_with("public.png") || t == "public.jpeg" || t == "com.compuserve.gif" || t == "public.heic" || t == "org.webmproject.webp" || t == "public.tiff" || t == "public.svg-image" => "image",
        "com.apple.m4a-audio" | "public.mp3" | "com.microsoft.waveform-audio" | "public.aiff-audio" | "org.webmproject.webm" => "audio",
        _ => "file",
    };
    let info = json!({"id": att.header.id, "filename": att.header.filename, "type": att.header.type_identifier, "size": att.data.len(), "kind": block_kind});
    let doc = st.docs.get_mut(id).ok_or("Die Notiz ist nicht geöffnet.")?;
    doc.file.attachments.push(att);
    Ok(info)
}

fn attachment_info(st: &mut State, a: &Value) -> CmdResult {
    let f = doc_file(st, arg_str(a, "id")?)?;
    let att = f.attachment(arg_str(a, "att")?).ok_or("Anhang fehlt.")?;
    Ok(json!({"filename": att.header.filename, "type": att.header.type_identifier, "size": att.data.len(), "isPackage": att.header.is_package}))
}

/// The bytes of an attachment, for the attachment URL scheme.
pub fn attachment_bytes(app: &App, note: &str, att: &str) -> Option<(Vec<u8>, String)> {
    let st = app.state.lock();
    let f = match st.docs.get(note) {
        Some(d) => d.file.attachment(att).map(|x| (x.data.clone(), mime_for(&x.header.type_identifier, &x.header.filename).to_string())),
        None => {
            let p = st.path_of(note).ok()?;
            drop(st);
            let f = NoteFile::read(&p).ok()?;
            return f.attachment(att).map(|x| (x.data.clone(), mime_for(&x.header.type_identifier, &x.header.filename).to_string()));
        }
    };
    f
}

fn attachment_export(st: &mut State, a: &Value) -> CmdResult {
    let f = doc_file(st, arg_str(a, "id")?)?;
    let att = f.attachment(arg_str(a, "att")?).ok_or("Anhang fehlt.")?;
    if att.header.is_package {
        return Err("Dieser Anhang ist ein Paket aus der Swift-Version (z. B. Numbers) und lässt sich in der Rust-Version noch nicht auspacken.".into());
    }
    let dest = PathBuf::from(arg_str(a, "dest")?);
    std::fs::write(&dest, &att.data).map_err(|e| e.to_string())?;
    Ok(json!(dest))
}

fn attachment_open(st: &mut State, a: &Value) -> CmdResult {
    let f = doc_file(st, arg_str(a, "id")?)?;
    let att = f.attachment(arg_str(a, "att")?).ok_or("Anhang fehlt.")?;
    if att.header.is_package {
        return Err("Dieser Anhang ist ein Paket aus der Swift-Version (z. B. Numbers) und lässt sich in der Rust-Version noch nicht öffnen.".into());
    }
    let dir = std::env::temp_dir().join(format!("folio-open-{}", &att.header.id[..8]));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let p = dir.join(text::sanitize_filename(&att.header.filename));
    std::fs::write(&p, &att.data).map_err(|e| e.to_string())?;
    if std::env::var("FOLIO_BACKGROUND").is_err() {
        std::process::Command::new("/usr/bin/open").arg(&p).spawn().map_err(|e| e.to_string())?;
    }
    Ok(json!(p))
}

fn attachment_replace(st: &mut State, a: &Value) -> CmdResult {
    use base64::Engine;
    let id = arg_str(a, "id")?;
    let att = arg_str(a, "att")?;
    let data = base64::engine::general_purpose::STANDARD.decode(arg_str(a, "data")?).map_err(|e| e.to_string())?;
    let doc = st.docs.get_mut(id).ok_or("Die Notiz ist nicht geöffnet.")?;
    let x = doc.file.attachments.iter_mut().find(|x| x.header.id == att).ok_or("Anhang fehlt.")?;
    x.data = data;
    if let Some(n) = opt_str(a, "filename") {
        x.header.filename = n;
    }
    Ok(json!(true))
}

fn markdown_export(st: &mut State, a: &Value) -> CmdResult {
    let id = arg_str(a, "id")?;
    let f = doc_file(st, id)?;
    let dest = PathBuf::from(arg_str(a, "dest")?);
    let base = dest.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("Notiz".into());
    let folder = format!("{base} Dateien");
    let (md, files) = markdown::export(&f, &folder, st.settings.editor_font_size);
    std::fs::write(&dest, md).map_err(|e| e.to_string())?;
    if !files.is_empty() {
        let dir = dest.parent().unwrap_or(Path::new(".")).join(&folder);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for (name, idx) in files {
            let _ = std::fs::write(dir.join(name), &f.attachments[idx].data);
        }
    }
    Ok(json!(dest))
}

fn markdown_import(st: &mut State, a: &Value) -> CmdResult {
    let paths: Vec<String> = a["paths"].as_array().map(|v| v.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    let dir = st.target_folder(opt_str(a, "folder").as_deref());
    let mut ids = vec![];
    for p in paths {
        let pb = PathBuf::from(&p);
        let Ok(text) = std::fs::read_to_string(&pb) else { continue };
        let base = pb.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mut f = markdown::import(&text, st.settings.editor_font_size, &st.settings.write_options(), |src| {
            let file = folio_core::markdown::is_safe_link(src).then_some(()).map_or_else(|| {
                // Only files inside the Markdown file's folder (#161).
                let p = base.join(src);
                let real = std::fs::canonicalize(&p).ok()?;
                let root = std::fs::canonicalize(&base).ok()?;
                if !real.starts_with(&root) {
                    return None;
                }
                Some(real)
            }, |_| None)?;
            let data = std::fs::read(&file).ok()?;
            let name = file.file_name()?.to_string_lossy().to_string();
            Some(folio_core::archive::Attachment::new(&name, type_for(&name), data))
        });
        if f.meta.title.is_empty() {
            f.meta.title = pb.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        }
        note::refresh_meta(&mut f, true);
        if f.meta.title.is_empty() {
            f.meta.title = pb.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        }
        st.write_new(&f, &dir)?;
        ids.push(f.meta.id);
    }
    Ok(json!(ids))
}

fn run_code(st: &mut State, a: &Value) -> CmdResult {
    let code = arg_str(a, "code")?;
    let lang_id = opt_str(a, "language").unwrap_or("python".into());
    let lang = languages::spec_for(&lang_id).ok_or_else(|| format!("{lang_id} kann nicht ausgeführt werden."))?;
    // The note's own environment (#25) overrides the app settings.
    let env_v = opt_str(a, "note").and_then(|n| st.docs.get(&n).and_then(|d| d.file.meta.environment.clone()));
    let note_override = env_v.as_ref().and_then(|e| e["interpreters"][lang.id].as_str().map(|s| s.to_string()));
    let override_cmd = note_override.or_else(|| st.settings.interpreter_overrides.get(lang.id).cloned());
    let mut env = HashMap::new();
    if let Some(vars) = env_v.as_ref().and_then(|e| e["variables"].as_object()) {
        for (k, v) in vars {
            if let Some(s) = v.as_str() {
                env.insert(k.clone(), s.to_string());
            }
        }
    }
    let working_dir = env_v.as_ref().and_then(|e| e["workingDirectory"].as_str()).filter(|s| !s.trim().is_empty()).map(|s| {
        if let Some(r) = s.strip_prefix("~/") { PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(r) } else { PathBuf::from(s) }
    });
    let timeout = env_v.as_ref().and_then(|e| e["timeout"].as_f64()).unwrap_or(st.settings.run_timeout);
    let opts = folio_core::runner::RunOptions {
        stdin: opt_str(a, "stdin"),
        interactive: a["interactive"].as_bool().unwrap_or(false),
        working_dir,
        env,
        timeout,
        max_output: (st.settings.max_output_kb.max(1.0) * 1024.0) as usize,
    };
    let run = folio_core::runner::CodeRun::start(code, &lang, override_cmd.as_deref(), opts);
    let job = folio_core::new_id();
    st.jobs.insert(job.clone(), Job::Run(run));
    Ok(json!(job))
}

fn job_poll(app: &App, a: &Value) -> CmdResult {
    let id = arg_str(a, "job")?;
    let mut st = app.state.lock();
    let (v, done) = match st.jobs.get(id) {
        Some(Job::Run(r)) => {
            let s = r.snapshot();
            let done = s.finished;
            (serde_json::to_value(s).unwrap(), done)
        }
        Some(Job::Ai(j)) => {
            let j = j.lock().clone();
            let done = j.finished;
            (serde_json::to_value(j).unwrap(), done)
        }
        None => return Err("Unbekannter Auftrag.".into()),
    };
    if done && a["release"].as_bool().unwrap_or(true) {
        st.jobs.remove(id);
    }
    Ok(v)
}

fn job_cancel(app: &App, a: &Value) -> CmdResult {
    let id = arg_str(a, "job")?;
    let st = app.state.lock();
    match st.jobs.get(id) {
        Some(Job::Run(r)) => r.cancel(),
        Some(Job::Ai(j)) => j.lock().cancelled = true,
        None => {}
    }
    Ok(json!(true))
}

fn job_send(app: &App, a: &Value) -> CmdResult {
    let id = arg_str(a, "job")?;
    let st = app.state.lock();
    if let Some(Job::Run(r)) = st.jobs.get(id) {
        if a["close"].as_bool().unwrap_or(false) {
            r.close_input();
        } else {
            r.send(arg_str(a, "text")?);
        }
    }
    Ok(json!(true))
}

fn start_ai(st: &mut State, system: String, messages: Vec<(String, String)>, sources: Vec<Value>) -> CmdResult {
    if st.settings.ai_provider != "ollama" {
        return Err("Die KI ist ausgeschaltet. In den Einstellungen unter „KI“ lässt sich ein lokales Ollama einschalten.".into());
    }
    let o = ai::Ollama::new(&st.settings.ollama_url, &st.settings.ollama_model)?;
    let job = Arc::new(parking_lot::Mutex::new(AiJob { sources, ..Default::default() }));
    let j2 = job.clone();
    std::thread::spawn(move || {
        let r = o.chat(&system, &messages, |t| {
            let mut j = j2.lock();
            j.text = t.to_string();
            !j.cancelled
        });
        let mut j = j2.lock();
        match r {
            Ok(t) => j.text = t,
            Err(e) => j.error = Some(e),
        }
        j.finished = true;
    });
    let id = folio_core::new_id();
    st.jobs.insert(id.clone(), Job::Ai(job));
    Ok(json!(id))
}

/// Chat about the open note or the whole library (#33, #34).
fn ai_chat(st: &mut State, a: &Value) -> CmdResult {
    let question = arg_str(a, "question")?.to_string();
    let history: Vec<(String, String)> = a["history"].as_array().map(|h| h.iter().filter_map(|m| Some((m["role"].as_str()?.to_string(), m["content"].as_str()?.to_string()))).collect()).unwrap_or_default();
    let scope = opt_str(a, "scope").unwrap_or("note".into());
    let mut sources = vec![];
    let mut passages_all = vec![];
    if scope == "library" {
        for n in &st.notes {
            passages_all.extend(ai::passages(&n.meta.id, &n.meta.title, &n.meta.search_text, 900));
        }
        sources = ai::rank_passages(&question, &passages_all, 6);
    } else if let Some(id) = opt_str(a, "note") {
        let f = doc_file(st, &id)?;
        let text: String = f.body.blocks.iter().map(note::block_plain_text).collect::<Vec<_>>().join("\n");
        let ps = ai::passages(&id, &f.meta.title, &text, 1500);
        sources = if ps.len() > 6 { ai::rank_passages(&question, &ps, 6) } else { ps };
    }
    let prompt = ai::library_prompt(&question, &sources);
    let mut messages = history;
    messages.push(("user".into(), prompt));
    let src: Vec<Value> = sources.iter().map(|p| json!({"id": p.note_id, "title": p.title, "passage": p.index})).collect();
    start_ai(st, ai::CHAT_SYSTEM.to_string(), messages, src)
}

fn ai_action(st: &mut State, a: &Value) -> CmdResult {
    let action = arg_str(a, "action")?;
    let text = arg_str(a, "text")?;
    start_ai(st, ai::action_instructions(action), vec![("user".into(), ai::action_prompt(text, 6000))], vec![])
}

fn ai_tags(st: &mut State, a: &Value) -> CmdResult {
    let id = arg_str(a, "id")?;
    let f = doc_file(st, id)?;
    let text: String = f.body.blocks.iter().filter(|b| b.kind == kind::TEXT).map(|b| b.text()).collect::<Vec<_>>().join("\n");
    let lib = st.tag_counts();
    Ok(json!(ai::suggest_tags(&text, &f.meta.tags, &lib, 6)))
}

fn capture_cmd(st: &mut State, a: &Value) -> CmdResult {
    let item = capture::CaptureItem {
        id: folio_core::new_id(),
        text: opt_str(a, "text").unwrap_or_default(),
        images: a["images"].as_array().map(|v| v.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()).unwrap_or_default(),
        target: opt_str(a, "target").unwrap_or("inbox".into()),
        created: model::now(),
        last_error: None,
    };
    let target_path = if item.target != "inbox" { st.find(&item.target).map(|n| n.path.clone()) } else { None };
    if let Some(d) = st.docs.get(&item.target) {
        // An open note gets the capture through the editor, so nothing is overwritten.
        let _ = d;
    }
    let date = text::capture_date_text(&chrono::Local::now());
    match capture::deliver(&st.root, &item, target_path.as_deref(), &date, &st.settings.write_options()) {
        Ok(p) => {
            let meta = NoteFile::read_metadata(&p).map_err(|e| e.to_string())?;
            st.docs.remove(&meta.id);
            st.upsert(&p, &meta);
            Ok(json!({"id": meta.id}))
        }
        Err(e) => {
            let q = capture::CaptureQueue { directory: library::data_folder(&st.root, "capture-queue") };
            let mut item = item;
            item.last_error = Some(e.clone());
            let _ = q.add(&item);
            Err(format!("{e} Die Erfassung wartet und wird später erneut versucht."))
        }
    }
}
