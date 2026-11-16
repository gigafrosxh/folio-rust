//! A tiny local HTTP server that serves the user interface and the commands, so the whole app can be
//! tested in a headless browser without a window (only on 127.0.0.1).

use crate::App;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn content_type(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "json" => "application/json",
        _ => "application/octet-stream",
    }
}

fn respond(s: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) {
    let head = format!("HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len());
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
}

fn handle(app: &App, ui: &Path, mut s: TcpStream) {
    let mut r = BufReader::new(s.try_clone().unwrap());
    let mut line = String::new();
    if r.read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let mut len = 0usize;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).unwrap_or(0) == 0 {
            break;
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some(v) = h.to_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
    }
    let path = target.split('?').next().unwrap_or("/").to_string();
    if method == "POST" && path == "/api" {
        let mut body = vec![0u8; len];
        let _ = r.read_exact(&mut body);
        let req: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
        let cmd = req["cmd"].as_str().unwrap_or("");
        let res = match app.call(cmd, req["args"].clone()) {
            Ok(v) => serde_json::json!({"ok": v}),
            Err(e) => serde_json::json!({"err": e}),
        };
        respond(&mut s, "200 OK", "application/json", res.to_string().as_bytes());
        return;
    }
    if let Some(rest) = path.strip_prefix("/att/") {
        let mut it = rest.split('/');
        let (note, att) = (it.next().unwrap_or(""), it.next().unwrap_or(""));
        match crate::commands::attachment_bytes(app, note, att) {
            Some((data, mime)) => respond(&mut s, "200 OK", &mime, &data),
            None => respond(&mut s, "404 Not Found", "text/plain", b"not found"),
        }
        return;
    }
    let rel = if path == "/" { "index.html".to_string() } else { percent(path.trim_start_matches('/')) };
    if rel.contains("..") {
        respond(&mut s, "403 Forbidden", "text/plain", b"no");
        return;
    }
    let file = ui.join(&rel);
    match std::fs::read(&file) {
        Ok(d) => respond(&mut s, "200 OK", content_type(&file), &d),
        Err(_) => respond(&mut s, "404 Not Found", "text/plain", b"not found"),
    }
}

fn percent(s: &str) -> String {
    folio_core_decode(s)
}

fn folio_core_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Serves until the process ends. Returns the bound port.
pub fn serve(app: Arc<App>, ui: PathBuf, port: u16) -> std::io::Result<u16> {
    let l = TcpListener::bind(("127.0.0.1", port))?;
    let port = l.local_addr()?.port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let app = app.clone();
            let ui = ui.clone();
            std::thread::spawn(move || handle(&app, &ui, s));
        }
    });
    Ok(port)
}
