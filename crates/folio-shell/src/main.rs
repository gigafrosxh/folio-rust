//! Folio's macOS app: the window, the menu bar, `folio://` links and the attachment URL scheme around
//! the shared command layer (`folio-app`).

use folio_app::App;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;

const RELEASE_ID: &str = "de.daniel.folio.rust";

/// Test runs must never use the released identity, or the real settings would change (#136).
fn refuse_test_mode() {
    let keys: Vec<String> = std::env::vars().map(|(k, _)| k).filter(|k| k == "FOLIO_LIBRARY" || (k.starts_with("FOLIO_") && k.ends_with("_TEST"))).collect();
    if keys.is_empty() {
        return;
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let plist = exe.parent().and_then(|p| p.parent()).map(|c| c.join("Info.plist"));
    if let Some(p) = plist {
        if let Ok(text) = std::fs::read_to_string(&p) {
            if let Some(i) = text.find("<key>CFBundleIdentifier</key>") {
                let rest = &text[i..];
                let id = rest.split("<string>").nth(1).and_then(|s| s.split("</string>").next()).unwrap_or("");
                if id == RELEASE_ID {
                    eprintln!("Folio startet nicht im Testmodus ({}) mit der Bundle-ID {RELEASE_ID}, sonst ändern sich die echten Einstellungen. Testkopie erzeugen: scripts/make-test-app.sh", keys.join(", "));
                    std::process::exit(78);
                }
            }
        }
    }
}

fn background() -> bool {
    std::env::var("FOLIO_BACKGROUND").is_ok() && std::env::var("FOLIO_LIBRARY").is_ok()
}

#[tauri::command]
async fn call(state: tauri::State<'_, Arc<App>>, cmd: String, args: Value) -> Result<Value, String> {
    let app = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || app.call(&cmd, args)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn dialog_open(app: tauri::AppHandle, opts: Value) -> Result<Value, String> {
    let mut d = app.dialog().file();
    if let Some(t) = opts["title"].as_str() {
        d = d.set_title(t);
    }
    if let Some(filters) = opts["filters"].as_array() {
        for f in filters {
            let exts: Vec<&str> = f["extensions"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
            d = d.add_filter(f["name"].as_str().unwrap_or(""), &exts);
        }
    }
    if opts["directory"].as_bool().unwrap_or(false) {
        return Ok(d.blocking_pick_folder().and_then(|p| p.into_path().ok()).map(|p| json!(p)).unwrap_or(Value::Null));
    }
    if opts["multiple"].as_bool().unwrap_or(false) {
        let v: Vec<String> = d.blocking_pick_files().unwrap_or_default().into_iter().filter_map(|p| p.into_path().ok()).map(|p| p.to_string_lossy().to_string()).collect();
        return Ok(if v.is_empty() { Value::Null } else { json!(v) });
    }
    Ok(d.blocking_pick_file().and_then(|p| p.into_path().ok()).map(|p| json!(p)).unwrap_or(Value::Null))
}

#[tauri::command]
async fn dialog_save(app: tauri::AppHandle, opts: Value) -> Result<Value, String> {
    let mut d = app.dialog().file();
    if let Some(t) = opts["title"].as_str() {
        d = d.set_title(t);
    }
    if let Some(n) = opts["defaultPath"].as_str() {
        d = d.set_file_name(n);
    }
    Ok(d.blocking_save_file().and_then(|p| p.into_path().ok()).map(|p| json!(p)).unwrap_or(Value::Null))
}

#[tauri::command]
fn open_note_window(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let label = format!("note-{}", id.to_lowercase());
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.set_focus();
        return Ok(());
    }
    let url = WebviewUrl::App(format!("index.html?note={id}&single=1").into());
    let mut b = WebviewWindowBuilder::new(&app, &label, url).title("Notiz").inner_size(760.0, 820.0).min_inner_size(420.0, 320.0);
    b = b.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true);
    if background() {
        b = b.visible(false);
    }
    b.build().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn print(window: tauri::WebviewWindow) -> Result<(), String> {
    window.print().map_err(|e| e.to_string())
}

#[tauri::command]
fn set_theme(app: tauri::AppHandle, theme: String) {
    let t = match theme.as_str() {
        "light" => Some(tauri::Theme::Light),
        "dark" => Some(tauri::Theme::Dark),
        _ => None,
    };
    for w in app.webview_windows().values() {
        let _ = w.set_theme(t);
    }
}

fn item(app: &tauri::AppHandle, id: &str, label: &str, accel: Option<&str>) -> tauri::Result<MenuItem<tauri::Wry>> {
    MenuItem::with_id(app, id, label, true, accel)
}

/// The menu bar, like the Swift app's (FolioCommands). Item ids are "command" or "command:argument".
fn build_menu(app: &tauri::AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let version = format!("{}r", env!("CARGO_PKG_VERSION"));
    let about = PredefinedMenuItem::about(app, Some("Über Folio"), Some(AboutMetadata { version: Some(version), copyright: Some("Folio – in Rust".into()), ..Default::default() }))?;
    let app_menu = Submenu::with_items(app, "Folio", true, &[
        &about,
        &PredefinedMenuItem::separator(app)?,
        &item(app, "settings", "Einstellungen …", Some("CmdOrCtrl+,"))?,
        &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::hide(app, Some("Folio ausblenden"))?,
        &PredefinedMenuItem::hide_others(app, Some("Andere ausblenden"))?,
        &PredefinedMenuItem::show_all(app, Some("Alle einblenden"))?,
        &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::quit(app, Some("Folio beenden"))?,
    ])?;
    let file = Submenu::with_items(app, "Ablage", true, &[
        &item(app, "new-note", "Neue Notiz", Some("CmdOrCtrl+N"))?,
        &item(app, "new-folder", "Neuer Ordner …", Some("CmdOrCtrl+Shift+N"))?,
        &item(app, "capture", "Schnellerfassung …", None)?,
        &item(app, "daily", "Tagesnotiz", Some("CmdOrCtrl+Ctrl+D"))?,
        &item(app, "new-tab", "Neuer Tab", Some("CmdOrCtrl+T"))?,
        &PredefinedMenuItem::separator(app)?,
        &item(app, "window", "In neuem Fenster öffnen", Some("CmdOrCtrl+Alt+O"))?,
        &item(app, "split", "Rechts daneben öffnen / Teilung schließen", Some("CmdOrCtrl+Alt+\\"))?,
        &item(app, "close-tab", "Tab schließen", Some("CmdOrCtrl+W"))?,
        &item(app, "next-tab", "Nächster Tab", Some("CmdOrCtrl+Shift+]"))?,
        &item(app, "prev-tab", "Vorheriger Tab", Some("CmdOrCtrl+Shift+["))?,
        &PredefinedMenuItem::separator(app)?,
        &item(app, "save", "Sichern", Some("CmdOrCtrl+S"))?,
        &item(app, "quick-open", "Schnell öffnen …", Some("CmdOrCtrl+P"))?,
        &item(app, "new-from-template", "Neu aus Vorlage …", None)?,
        &item(app, "template-save", "Als Vorlage sichern", None)?,
        &item(app, "duplicate", "Notiz duplizieren", Some("CmdOrCtrl+D"))?,
        &item(app, "delete", "Notiz in den Papierkorb legen", None)?,
        &PredefinedMenuItem::separator(app)?,
        &item(app, "import-md", "Markdown importieren …", None)?,
        &item(app, "import-apple", "Apple Notizen importieren …", None)?,
        &item(app, "export-md", "Als Markdown exportieren …", Some("CmdOrCtrl+Alt+E"))?,
        &item(app, "export-pdf", "Als PDF exportieren / Drucken …", Some("CmdOrCtrl+Shift+E"))?,
        &PredefinedMenuItem::separator(app)?,
        &item(app, "history", "Versionsverlauf …", Some("CmdOrCtrl+Alt+Y"))?,
        &item(app, "info", "Informationen zur Notiz", None)?,
        &item(app, "backup-now", "Bibliothek jetzt sichern", None)?,
    ])?;
    let edit = Submenu::with_items(app, "Bearbeiten", true, &[
        &item(app, "undo", "Widerrufen", Some("CmdOrCtrl+Z"))?,
        &item(app, "redo", "Wiederholen", Some("CmdOrCtrl+Shift+Z"))?,
        &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::cut(app, Some("Ausschneiden"))?,
        &PredefinedMenuItem::copy(app, Some("Kopieren"))?,
        &PredefinedMenuItem::paste(app, Some("Einsetzen"))?,
        &PredefinedMenuItem::select_all(app, Some("Alles auswählen"))?,
        &PredefinedMenuItem::separator(app)?,
        &item(app, "find", "In Notiz suchen …", Some("CmdOrCtrl+F"))?,
        &item(app, "replace", "Suchen und ersetzen …", Some("CmdOrCtrl+Ctrl+F"))?,
    ])?;
    let format = Submenu::with_items(app, "Format", true, &[
        &item(app, "style:title", "Titel", Some("CmdOrCtrl+Shift+1"))?,
        &item(app, "style:heading", "Überschrift", Some("CmdOrCtrl+Shift+2"))?,
        &item(app, "style:subheading", "Unterüberschrift", Some("CmdOrCtrl+Shift+3"))?,
        &item(app, "style:body", "Text", Some("CmdOrCtrl+Shift+0"))?,
        &item(app, "style:monospaced", "Monospace", None)?,
        &item(app, "style:quote", "Zitat", None)?,
        &PredefinedMenuItem::separator(app)?,
        &item(app, "bold", "Fett", Some("CmdOrCtrl+B"))?,
        &item(app, "italic", "Kursiv", Some("CmdOrCtrl+I"))?,
        &item(app, "underline", "Unterstrichen", Some("CmdOrCtrl+U"))?,
        &item(app, "strike", "Durchgestrichen", Some("CmdOrCtrl+Shift+X"))?,
        &item(app, "mono", "Code im Text", None)?,
        &PredefinedMenuItem::separator(app)?,
        &item(app, "list:checklist", "Checkliste", Some("CmdOrCtrl+Shift+L"))?,
        &item(app, "list:bullet", "Aufzählung", Some("CmdOrCtrl+Shift+7"))?,
        &item(app, "list:dash", "Strichliste", None)?,
        &item(app, "list:numbered", "Nummerierte Liste", Some("CmdOrCtrl+Shift+8"))?,
        &item(app, "indent", "Einrücken", Some("CmdOrCtrl+]"))?,
        &item(app, "outdent", "Ausrücken", Some("CmdOrCtrl+["))?,
        &item(app, "align:center", "Zentriert", None)?,
        &item(app, "align:left", "Linksbündig", None)?,
        &PredefinedMenuItem::separator(app)?,
        &item(app, "insert:link", "Link …", Some("CmdOrCtrl+K"))?,
        &item(app, "link-note", "Link zu Notiz …", Some("CmdOrCtrl+Alt+K"))?,
        &item(app, "chat", "Chat über die Notiz", Some("CmdOrCtrl+Alt+I"))?,
        &item(app, "ask-library", "Bibliothek fragen …", Some("CmdOrCtrl+Alt+Shift+I"))?,
        &item(app, "font-bigger", "Schrift größer", Some("CmdOrCtrl+Plus"))?,
        &item(app, "font-smaller", "Schrift kleiner", Some("CmdOrCtrl+-"))?,
    ])?;
    let insert = Submenu::with_items(app, "Einfügen", true, &[
        &item(app, "insert:code", "Code-Block", Some("CmdOrCtrl+Alt+C"))?,
        &item(app, "insert:file", "Datei, PDF oder Bild …", Some("CmdOrCtrl+Shift+A"))?,
        &item(app, "insert:database", "Datenbank-Abfrage", None)?,
        &item(app, "insert:math", "Formel (LaTeX)", Some("CmdOrCtrl+Alt+M"))?,
        &item(app, "insert:diagram", "Diagramm (Mermaid)", Some("CmdOrCtrl+Alt+G"))?,
        &item(app, "insert:embed", "Notiz einbetten …", None)?,
        &item(app, "insert:audio", "Audioaufnahme", Some("CmdOrCtrl+Alt+R"))?,
        &item(app, "insert:drawing", "Zeichnung", Some("CmdOrCtrl+Alt+D"))?,
        &item(app, "insert:web", "Webseite …", Some("CmdOrCtrl+Alt+L"))?,
    ])?;
    let view = Submenu::with_items(app, "Darstellung", true, &[
        &item(app, "toggle-sidebar", "Seitenleiste ein-/ausblenden", Some("CmdOrCtrl+Alt+S"))?,
        &item(app, "search", "Notizen durchsuchen", Some("CmdOrCtrl+Alt+F"))?,
        &item(app, "focus", "Fokusmodus", Some("CmdOrCtrl+Shift+F"))?,
        &item(app, "graph", "Graph", Some("CmdOrCtrl+Ctrl+G"))?,
        &item(app, "run-env", "Ausführungsumgebung der Notiz …", None)?,
        &item(app, "run-all", "Alle Code-Blöcke ausführen", None)?,
        &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::fullscreen(app, Some("Vollbild"))?,
    ])?;
    let window = Submenu::with_items(app, "Fenster", true, &[
        &PredefinedMenuItem::minimize(app, Some("Im Dock ablegen"))?,
        &PredefinedMenuItem::maximize(app, Some("Zoomen"))?,
        &PredefinedMenuItem::close_window(app, Some("Fenster schließen"))?,
    ])?;
    let help = Submenu::with_items(app, "Hilfe", true, &[
        &item(app, "help", "Folio-Hilfe", Some("CmdOrCtrl+?"))?,
        &item(app, "shortcuts", "Tastenkürzel …", None)?,
        &item(app, "privacy", "Datenschutz und Sicherheit", None)?,
        &item(app, "changelog", "Änderungen in dieser Version", None)?,
        &PredefinedMenuItem::separator(app)?,
        &item(app, "issues", "Fehler melden oder Idee vorschlagen …", None)?,
    ])?;
    Menu::with_items(app, &[&app_menu, &file, &edit, &format, &insert, &view, &window, &help])
}

/// Sends an event to the window the user works in (the focused one, else the main window).
fn emit_front(app: &tauri::AppHandle, payload: Value) {
    let windows = app.webview_windows();
    let target = windows.values().find(|w| w.is_focused().unwrap_or(false)).cloned().or_else(|| app.get_webview_window("main"));
    if let Some(w) = target {
        let _ = w.emit_to(w.label(), "folio", payload);
    }
}

fn main_window(app: &tauri::AppHandle) -> tauri::Result<()> {
    let mut b = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Folio")
        .inner_size(1280.0, 820.0)
        .min_inner_size(900.0, 560.0)
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(16.0, 20.0));
    if background() {
        b = b.visible(false).focused(false);
    }
    b.build()?;
    Ok(())
}

fn tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::tray::TrayIconBuilder;
    let menu = Menu::with_items(app, &[
        &MenuItem::with_id(app, "tray:capture", "Schnellerfassung …", true, None::<&str>)?,
        &MenuItem::with_id(app, "tray:new-note", "Neue Notiz", true, None::<&str>)?,
        &MenuItem::with_id(app, "tray:daily", "Tagesnotiz", true, None::<&str>)?,
        &PredefinedMenuItem::separator(app)?,
        &MenuItem::with_id(app, "tray:show", "Folio öffnen", true, None::<&str>)?,
    ])?;
    let mut t = TrayIconBuilder::with_id("folio").menu(&menu).tooltip("Folio");
    if let Some(icon) = app.default_window_icon() {
        t = t.icon(icon.clone());
    }
    t.build(app)?;
    Ok(())
}

fn main() {
    refuse_test_mode();
    let state = Arc::new(App::new());
    let menu_bar = state.state.lock().settings.menu_bar_icon && !background();
    let att_state = state.clone();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .register_asynchronous_uri_scheme_protocol("folioatt", move |_ctx, request, responder| {
            let app = att_state.clone();
            let path = request.uri().path().to_string();
            std::thread::spawn(move || {
                let mut parts = path.trim_start_matches('/').split('/');
                let (note, att) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                let resp = match folio_app::commands::attachment_bytes(&app, note, att) {
                    Some((data, mime)) => tauri::http::Response::builder().header("Content-Type", mime).header("Access-Control-Allow-Origin", "*").body(data),
                    None => tauri::http::Response::builder().status(404).body(b"not found".to_vec()),
                };
                responder.respond(resp.unwrap());
            });
        })
        .invoke_handler(tauri::generate_handler![call, dialog_open, dialog_save, open_note_window, print, set_theme])
        .menu(|app| build_menu(app))
        .on_menu_event(|app, event| {
            let id = event.id().0.as_str();
            if let Some(rest) = id.strip_prefix("tray:") {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
                if rest != "show" {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.emit_to("main", "folio", json!({"type": "menu", "id": rest}));
                    }
                }
                return;
            }
            let (cmd, arg) = id.split_once(':').map(|(a, b)| (a, Some(b))).unwrap_or((id, None));
            emit_front(app, json!({"type": "menu", "id": cmd, "arg": arg}));
        })
        .setup(move |app| {
            if background() {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
            main_window(app.handle())?;
            if menu_bar {
                let _ = tray(app.handle());
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Folio konnte nicht starten");
    app.run(|app, event| match event {
        RunEvent::Opened { urls } => {
            for u in urls {
                let s = u.to_string();
                if s.starts_with("folio:") {
                    emit_front(app, json!({"type": "open-url", "url": s}));
                } else if let Ok(p) = u.to_file_path() {
                    emit_front(app, json!({"type": "files", "paths": [p]}));
                }
            }
        }
        RunEvent::Reopen { has_visible_windows, .. } => {
            if !has_visible_windows {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                } else {
                    let _ = main_window(app);
                }
            }
        }
        RunEvent::ExitRequested { .. } => {
            // Open notes are saved by the page itself on every change; nothing is left in memory here.
        }
        _ => {}
    });
}
