//! Database blocks: SQL against SQLite, PostgreSQL or MySQL through their command-line clients, with the
//! result as a table. Passwords live in the Keychain under the same entries the Swift app uses.

use crate::app::{arg_str, opt_str, CmdResult, State};
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const SERVICE: &str = "de.daniel.folio.database";

pub fn keychain_password(id: &str) -> Option<String> {
    let out = Command::new("/usr/bin/security").args(["find-generic-password", "-s", SERVICE, "-a", id, "-w"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string())
}

pub fn set_keychain_password(id: &str, pw: &str) -> Result<(), String> {
    let st = Command::new("/usr/bin/security")
        .args(["add-generic-password", "-U", "-s", SERVICE, "-a", id, "-l", "Folio – Datenbank", "-w", pw])
        .status()
        .map_err(|e| e.to_string())?;
    if st.success() { Ok(()) } else { Err("Das Passwort konnte nicht im Schlüsselbund gesichert werden.".into()) }
}

fn redact(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_string();
    let mut s: Vec<&String> = secrets.iter().filter(|x| x.chars().count() >= 3).collect();
    s.sort_by_key(|x| std::cmp::Reverse(x.len()));
    for x in s {
        out = out.replace(x.as_str(), "••••••");
    }
    out
}

fn parse_csv(text: &str, sep: char) -> Vec<Vec<String>> {
    let mut rows = vec![];
    let mut row = vec![];
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    cell.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                cell.push(c);
            }
        } else if c == '"' && cell.is_empty() && sep == ',' {
            quoted = true;
        } else if c == sep {
            row.push(std::mem::take(&mut cell));
        } else if c == '\n' {
            row.push(std::mem::take(&mut cell));
            rows.push(std::mem::take(&mut row));
        } else if c != '\r' {
            cell.push(c);
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

pub fn query(st: &mut State, a: &Value) -> CmdResult {
    let sql = arg_str(a, "sql")?;
    let conn = a.get("connection").cloned().unwrap_or(json!({}));
    let driver = conn["driver"].as_str().unwrap_or("sqlite");
    let id = conn["id"].as_str().unwrap_or("");
    if let Some(pw) = opt_str(a, "password") {
        if !id.is_empty() {
            let _ = set_keychain_password(id, &pw);
        }
    }
    let password = opt_str(a, "password").or_else(|| if id.is_empty() { None } else { keychain_password(id) });
    let host = conn["host"].as_str().unwrap_or("localhost").to_string();
    let port = conn["port"].as_i64().unwrap_or(0).to_string();
    let user = conn["user"].as_str().unwrap_or("").to_string();
    let db = conn["database"].as_str().unwrap_or("").to_string();
    let path = conn["path"].as_str().unwrap_or("").to_string();
    let path = if let Some(r) = path.strip_prefix("~/") { format!("{}/{r}", std::env::var("HOME").unwrap_or_default()) } else { path };
    let (tool, args, env): (&str, Vec<String>, Vec<(String, String)>) = match driver {
        "sqlite" => {
            if path.is_empty() {
                return Err("Für SQLite fehlt der Pfad zur Datenbankdatei.".into());
            }
            ("sqlite3", vec!["-bail".into(), "-json".into(), path], vec![])
        }
        "postgresql" => (
            "psql",
            ["-X", "-q", "-w", "-v", "ON_ERROR_STOP=1", "--csv", "-h", &host, "-p", &port, "-U", &user, "-d", &db, "-f", "-"].iter().map(|s| s.to_string()).collect(),
            password.clone().map(|p| vec![("PGPASSWORD".to_string(), p)]).unwrap_or_default(),
        ),
        "mysql" => {
            let mut v: Vec<String> = ["--batch", "--column-names", "-h", &host, "-P", &port, "-u", &user, "--default-character-set=utf8mb4", "--connect-timeout=5"].iter().map(|s| s.to_string()).collect();
            if !db.is_empty() {
                v.push(format!("--database={db}"));
            }
            ("mysql", v, password.clone().map(|p| vec![("MYSQL_PWD".to_string(), p)]).unwrap_or_default())
        }
        other => return Err(format!("„{other}“ wird in der Rust-Version noch nicht unterstützt (SQLite, PostgreSQL und MySQL gehen).")),
    };
    let exe = folio_core::runner::find_executable(tool).ok_or_else(|| format!("„{tool}“ ist nicht installiert (z. B. mit Homebrew)."))?;
    let mut child = Command::new(exe)
        .args(&args)
        .envs(folio_core::runner::environment())
        .envs(env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(mut i) = child.stdin.take() {
        let _ = i.write_all(sql.as_bytes());
    }
    let timeout = st.settings.run_timeout.max(5.0);
    let start = Instant::now();
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        if start.elapsed() > Duration::from_secs_f64(timeout) {
            let _ = child.kill();
            return Err(format!("Zeitlimit von {} s überschritten.", timeout as i64));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let secrets: Vec<String> = password.into_iter().collect();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = redact(String::from_utf8_lossy(&out.stderr).trim(), &secrets);
    if !out.status.success() {
        return Err(if stderr.is_empty() { format!("{tool} meldet Status {:?}", out.status.code()) } else { stderr });
    }
    let (columns, rows): (Vec<String>, Vec<Vec<String>>) = match driver {
        "sqlite" => {
            let v: Vec<serde_json::Map<String, Value>> = serde_json::from_str(stdout.trim()).unwrap_or_default();
            let cols: Vec<String> = v.first().map(|r| r.keys().cloned().collect()).unwrap_or_default();
            let rows = v.iter().map(|r| cols.iter().map(|c| match &r[c] { Value::String(s) => s.clone(), Value::Null => "NULL".into(), other => other.to_string() }).collect()).collect();
            (cols, rows)
        }
        _ => {
            let sep = if driver == "mysql" { '\t' } else { ',' };
            let mut rows = parse_csv(&stdout, sep);
            if rows.is_empty() {
                (vec![], vec![])
            } else {
                let cols = rows.remove(0);
                (cols, rows)
            }
        }
    };
    Ok(json!({"columns": columns, "rows": rows, "message": if columns.is_empty() { if stderr.is_empty() { "Ausgeführt.".to_string() } else { stderr } } else { format!("{} Zeilen", rows.len()) }}))
}
