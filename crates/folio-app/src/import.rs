//! Import from Apple Notes through Apple's scripting bridge (JavaScript for Automation). macOS asks once
//! for permission to control Notes (System Settings › Privacy & Security › Automation).

use crate::app::{opt_str, CmdResult, State};
use folio_core::model::{self, kind, BlockRecord, NoteFile, NoteMetadata};
use serde_json::{json, Value};

const SCRIPT: &str = r#"
const Notes = Application('Notes');
const out = [];
const skip = ['Recently Deleted', 'Zuletzt gelöscht'];
Notes.accounts().forEach(acc => {
  acc.folders().forEach(f => {
    const name = f.name();
    if (skip.indexOf(name) >= 0) return;
    const notes = f.notes;
    const names = notes.name(), bodies = notes.body(), created = notes.creationDate(), modified = notes.modificationDate();
    for (let i = 0; i < names.length; i++) {
      out.push({account: acc.name(), folder: name, name: names[i], body: bodies[i], created: created[i].getTime() / 1000, modified: modified[i].getTime() / 1000});
    }
  });
});
JSON.stringify(out);
"#;

pub fn apple_notes(st: &mut State, a: &Value) -> CmdResult {
    let json_text = if let Some(p) = opt_str(a, "fromFile") {
        // For tests: the same JSON the script produces.
        std::fs::read_to_string(p).map_err(|e| e.to_string())?
    } else {
        if std::env::var("FOLIO_BACKGROUND").is_ok() {
            return Err("Der Import aus Apple Notizen braucht die Erlaubnis von macOS und läuft nicht im Hintergrundtest.".into());
        }
        let out = std::process::Command::new("/usr/bin/osascript").args(["-l", "JavaScript", "-e", SCRIPT]).output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            if err.contains("-1743") || err.contains("Not authorized") || err.contains("nicht berechtigt") {
                return Err("Folio darf Notizen nicht steuern. Erlaube es unter Systemeinstellungen › Datenschutz & Sicherheit › Automation › Folio und versuche es erneut.".into());
            }
            return Err(format!("Apple Notizen konnte nicht gelesen werden: {}", err.trim()));
        }
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let items: Vec<Value> = serde_json::from_str(json_text.trim()).map_err(|e| format!("Unerwartete Antwort von Notizen: {e}"))?;
    let base = st.root.join("Apple Notizen");
    let size = st.settings.editor_font_size;
    let opts = st.settings.write_options();
    let accounts: std::collections::HashSet<&str> = items.iter().filter_map(|i| i["account"].as_str()).collect();
    let multi = accounts.len() > 1;
    let mut count = 0;
    for it in &items {
        let html = it["body"].as_str().unwrap_or("");
        let mut rt = folio_core::html::to_rich_text(html, size);
        // Apple Notes' first line is the title: make it look like one.
        if let Some(p) = rt.paras.first_mut() {
            if !p.text().trim().is_empty() && p.runs.iter().all(|r| r.size.is_none()) {
                for r in p.runs.iter_mut() {
                    r.b = true;
                    r.size = Some((size * 1.85 * 2.0).round() / 2.0);
                }
            }
        }
        let mut b = BlockRecord::new(kind::TEXT);
        b.set_rtf(&folio_core::rtf::write(&rt, &opts));
        let mut meta = NoteMetadata::titled(it["name"].as_str().unwrap_or("Notiz"));
        if let Some(c) = it["created"].as_f64() {
            meta.created = model::from_unix(c);
        }
        let mut f = NoteFile::new(meta, vec![b]);
        folio_core::note::refresh_meta(&mut f, false);
        if let Some(m) = it["modified"].as_f64() {
            f.meta.modified = model::from_unix(m);
        }
        let mut dir = base.clone();
        if multi {
            dir = dir.join(folio_core::text::sanitize_filename(it["account"].as_str().unwrap_or("")));
        }
        dir = dir.join(folio_core::text::sanitize_filename(it["folder"].as_str().unwrap_or("Notizen")));
        if st.write_new(&f, &dir).is_ok() {
            count += 1;
        }
    }
    st.reload();
    Ok(json!(count))
}
