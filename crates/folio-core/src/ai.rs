//! Local AI: an Ollama client (only localhost, so notes never leave the Mac), text actions, answers as
//! blocks, tag suggestions and a BM25 search that picks the passages a library question is answered from.

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

// MARK: - Ollama

pub struct Ollama {
    host: String,
    port: u16,
    pub base: String,
    pub model: String,
}

impl Ollama {
    /// Nil for anything that is not an http address on this Mac.
    pub fn new(base_url: &str, model: &str) -> Result<Ollama, String> {
        let t = base_url.trim();
        let t = if t.contains("://") { t.to_string() } else { format!("http://{t}") };
        let notlocal = || format!("„{base_url}“ ist kein Server auf diesem Mac. Folio spricht nur mit einem lokalen Ollama (localhost), damit Notizen den Mac nicht verlassen.");
        let rest = t.strip_prefix("http://").ok_or_else(notlocal)?;
        let hostport = rest.split('/').next().unwrap_or("");
        let (host, port) = if let Some(h) = hostport.strip_prefix('[') {
            let (h, p) = h.split_once(']').ok_or_else(notlocal)?;
            (h.to_string(), p.trim_start_matches(':').parse().unwrap_or(80))
        } else {
            match hostport.rsplit_once(':') {
                Some((h, p)) => (h.to_string(), p.parse().map_err(|_| notlocal())?),
                None => (hostport.to_string(), 80),
            }
        };
        let h = host.to_lowercase();
        if !(h == "localhost" || h == "127.0.0.1" || h == "::1" || h.ends_with(".localhost")) {
            return Err(notlocal());
        }
        let connect_host = if h == "::1" { "[::1]".to_string() } else if h.ends_with("localhost") { "127.0.0.1".into() } else { h };
        Ok(Ollama { host: connect_host, port, base: t, model: model.to_string() })
    }

    fn connect(&self, timeout: u64) -> Result<TcpStream, String> {
        let addr = format!("{}:{}", self.host, self.port);
        let sock = addr.parse().map_err(|_| format!("Ungültige Adresse {addr}"))?;
        let s = TcpStream::connect_timeout(&sock, Duration::from_secs(3))
            .map_err(|_| format!("Ollama antwortet nicht unter {}. Starte die Ollama-App oder im Terminal `ollama serve`.", self.base))?;
        s.set_read_timeout(Some(Duration::from_secs(timeout))).ok();
        Ok(s)
    }

    fn request(&self, method: &str, path: &str, body: Option<&str>, timeout: u64) -> Result<(u16, Box<dyn BufRead>), String> {
        let mut s = self.connect(timeout)?;
        let body = body.unwrap_or("");
        let req = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.host, self.port, body.len()
        );
        s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
        let mut r = BufReader::new(s);
        let mut status_line = String::new();
        r.read_line(&mut status_line).map_err(|e| e.to_string())?;
        let status: u16 = status_line.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        let mut chunked = false;
        loop {
            let mut h = String::new();
            if r.read_line(&mut h).map_err(|e| e.to_string())? == 0 {
                break;
            }
            let h = h.trim_end();
            if h.is_empty() {
                break;
            }
            let lower = h.to_lowercase();
            if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
                chunked = true;
            }
        }
        if chunked {
            Ok((status, Box::new(BufReader::new(Chunked { inner: r, left: 0, done: false }))))
        } else {
            Ok((status, Box::new(r)))
        }
    }

    fn failure(&self, status: u16, body: &str) -> String {
        let lower = body.to_lowercase();
        if status == 404 && lower.contains("not found") {
            return format!("Das Modell „{}“ ist in Ollama nicht installiert. Lade es mit `ollama pull {}` oder wähle ein anderes.", self.model, self.model);
        }
        if lower.contains("does not support chat") || lower.contains("does not support generate") {
            return format!("„{}“ kann keinen Text schreiben (z. B. ein Embedding-Modell). Wähle in den Einstellungen ein anderes Modell, etwa `gemma3` oder `llama3.2`.", self.model);
        }
        let msg = serde_json::from_str::<serde_json::Value>(body).ok().and_then(|v| v.get("error").and_then(|e| e.as_str()).map(|s| s.to_string())).unwrap_or(format!("HTTP {status}"));
        format!("Ollama meldet einen Fehler: {msg}")
    }

    /// Installed models that can write text (embedding-only models are left out, #225).
    pub fn models(&self) -> Result<Vec<String>, String> {
        let (status, mut r) = self.request("GET", "/api/tags", None, 5)?;
        let mut body = String::new();
        r.read_to_string(&mut body).map_err(|e| e.to_string())?;
        if status != 200 {
            return Err(self.failure(status, &body));
        }
        let v: serde_json::Value = serde_json::from_str(&body).map_err(|_| "Ollama meldet einen Fehler: unerwartete Antwort auf /api/tags".to_string())?;
        let mut out: Vec<String> = v["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m.get("capabilities").and_then(|c| c.as_array()).map(|c| c.iter().any(|x| x == "completion")).unwrap_or(true))
            .filter_map(|m| m["name"].as_str().map(|s| s.to_string()))
            .collect();
        out.sort();
        Ok(out)
    }

    /// Streams the answer; `on_part` gets the whole text so far. Returns false from `on_part` to stop.
    pub fn chat(&self, system: &str, messages: &[(String, String)], mut on_part: impl FnMut(&str) -> bool) -> Result<String, String> {
        if self.model.is_empty() {
            return Err("In Ollama ist noch kein Modell ausgewählt. Wähle eines in den Einstellungen (KI) oder lade eines mit `ollama pull llama3.2`.".into());
        }
        let mut msgs = vec![serde_json::json!({"role": "system", "content": system})];
        for (role, content) in messages {
            msgs.push(serde_json::json!({"role": role, "content": content}));
        }
        let body = serde_json::json!({"model": self.model, "messages": msgs, "stream": true}).to_string();
        let (status, mut r) = self.request("POST", "/api/chat", Some(&body), 300)?;
        if status != 200 {
            let mut b = String::new();
            r.read_to_string(&mut b).ok();
            return Err(self.failure(status, &b));
        }
        let mut text = String::new();
        let mut line = String::new();
        loop {
            line.clear();
            if r.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
                break;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else { continue };
            if let Some(e) = v.get("error").and_then(|e| e.as_str()) {
                return Err(format!("Ollama meldet einen Fehler: {e}"));
            }
            if let Some(c) = v.pointer("/message/content").and_then(|c| c.as_str()) {
                text.push_str(c);
                if !on_part(&text) {
                    break;
                }
            }
            if v.get("done").and_then(|d| d.as_bool()) == Some(true) {
                break;
            }
        }
        Ok(text)
    }
}

/// Decodes HTTP chunked transfer encoding.
struct Chunked<R: BufRead> {
    inner: R,
    left: usize,
    done: bool,
}

impl<R: BufRead> Read for Chunked<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.done {
            return Ok(0);
        }
        if self.left == 0 {
            let mut line = String::new();
            self.inner.read_line(&mut line)?;
            if line.trim().is_empty() {
                line.clear();
                self.inner.read_line(&mut line)?;
            }
            let size = usize::from_str_radix(line.trim().split(';').next().unwrap_or("0"), 16).unwrap_or(0);
            if size == 0 {
                self.done = true;
                return Ok(0);
            }
            self.left = size;
        }
        let n = buf.len().min(self.left);
        let got = self.inner.read(&mut buf[..n])?;
        self.left -= got;
        if self.left == 0 {
            let mut crlf = [0u8; 2];
            let _ = self.inner.read_exact(&mut crlf);
        }
        Ok(got)
    }
}

// MARK: - Text actions (#36)

pub fn action_label(action: &str) -> &'static str {
    match action {
        "explain" => "Erklären",
        "simplify" => "Vereinfachen",
        "rewrite" => "Umschreiben",
        "quiz" => "Lernfragen erstellen",
        "summarize" => "Zusammenfassen",
        _ => "KI",
    }
}

pub fn action_instructions(action: &str) -> String {
    let base = "Du arbeitest in der Notizen-App Folio. Antworte in der Sprache des Textes. Gib nur das Ergebnis aus, ohne Einleitung und ohne Anführungszeichen drumherum.";
    let extra = match action {
        "explain" => " Erkläre den Text verständlich in wenigen Sätzen; nenne Fachbegriffe und was sie bedeuten.",
        "simplify" => " Formuliere den Text einfacher und kürzer, ohne Inhalt wegzulassen. Behalte Listen und Absätze bei.",
        "rewrite" => " Formuliere den Text klarer und flüssiger um. Inhalt, Ton und Aufbau bleiben gleich.",
        "quiz" => " Erstelle 3 bis 5 Lernfragen zum Text als nummerierte Liste, jeweils mit einer kurzen Antwort darunter, eingeleitet mit „Antwort:“.",
        "summarize" => " Fasse den Text in drei bis fünf Stichpunkten zusammen.",
        _ => "",
    };
    format!("{base}{extra}")
}

pub fn replaces_selection(action: &str) -> bool {
    matches!(action, "simplify" | "rewrite")
}

pub fn action_prompt(text: &str, limit: usize) -> String {
    let cut: String = if text.chars().count() > limit { text.chars().take(limit).collect::<String>() + "\n[…]" } else { text.to_string() };
    format!("Text:\n\"\"\"\n{cut}\n\"\"\"")
}

// MARK: - Answers as blocks (#40)

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Segment {
    Text { text: String },
    Code { language: String, code: String },
    Table { header: Vec<String>, rows: Vec<Vec<String>> },
}

fn cells(line: &str) -> Vec<String> {
    let t = line.trim().trim_start_matches('|').trim_end_matches('|');
    t.split('|').map(|c| c.trim().to_string()).collect()
}

fn is_sep(line: &str) -> bool {
    let t = line.trim();
    t.contains('-') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

pub fn segments(markdown: &str) -> Vec<Segment> {
    let lines: Vec<&str> = markdown.split('\n').collect();
    let mut out = vec![];
    let mut text: Vec<&str> = vec![];
    let mut i = 0;
    let flush = |text: &mut Vec<&str>, out: &mut Vec<Segment>| {
        let j = text.join("\n");
        let j = j.trim_matches('\n');
        if !j.trim().is_empty() {
            out.push(Segment::Text { text: j.to_string() });
        }
        text.clear();
    };
    while i < lines.len() {
        let line = lines[i];
        if let Some(lang) = line.trim_start().strip_prefix("```") {
            let mut code = vec![];
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with("```") {
                code.push(lines[i]);
                i += 1;
            }
            i += 1;
            flush(&mut text, &mut out);
            out.push(Segment::Code { language: lang.trim().to_string(), code: code.join("\n") });
            continue;
        }
        if line.trim_start().starts_with('|') && i + 1 < lines.len() && is_sep(lines[i + 1]) {
            let header = cells(line);
            let mut rows = vec![];
            i += 2;
            while i < lines.len() && lines[i].trim_start().starts_with('|') {
                let mut r = cells(lines[i]);
                r.resize(header.len(), String::new());
                rows.push(r);
                i += 1;
            }
            flush(&mut text, &mut out);
            out.push(Segment::Table { header, rows });
            continue;
        }
        text.push(line);
        i += 1;
    }
    flush(&mut text, &mut out);
    out
}

// MARK: - Tag suggestions (#37)

const STOP: &[&str] = &["diese", "dieser", "dieses", "eine", "einer", "einem", "einen", "aber", "auch", "dann", "denn", "noch", "nach", "über", "unter", "wenn", "weil", "dass", "sind", "wird", "werden", "haben", "kann", "können", "muss", "notiz", "notizen", "text"];

pub fn suggest_tags(text: &str, existing: &[String], library_tags: &[(String, usize)], limit: usize) -> Vec<String> {
    let have: HashSet<String> = existing.iter().map(|t| t.to_lowercase()).collect();
    let words: HashSet<String> = text.to_lowercase().split(|c: char| !c.is_alphanumeric() && c != '-').filter(|w| !w.is_empty()).map(|w| w.to_string()).collect();
    let mut lib: Vec<&(String, usize)> = library_tags.iter().collect();
    lib.sort_by(|a, b| b.1.cmp(&a.1));
    let mut out: Vec<String> = lib
        .into_iter()
        .map(|(t, _)| t.to_lowercase())
        .filter(|key| {
            let leaf = key.rsplit('/').next().unwrap_or(key);
            !have.contains(key) && leaf.chars().count() >= 3 && (words.contains(leaf) || words.iter().any(|w| w.starts_with(leaf) && w.chars().count() <= leaf.chars().count() + 3))
        })
        .take(limit)
        .collect();
    let mut counts: HashMap<String, usize> = HashMap::new();
    let tokens: Vec<&str> = text.split_whitespace().collect();
    for (i, raw) in tokens.iter().enumerate() {
        let word = raw.trim_matches(|c: char| c.is_ascii_punctuation() || "„“”‚‘’«»…".contains(c));
        if word.chars().count() < 4 || !word.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) || !word.chars().all(|c| c.is_alphabetic() || c == '-') {
            continue;
        }
        let after_end = i == 0 || tokens[i - 1].chars().last().map(|c| ".!?:".contains(c)).unwrap_or(false);
        *counts.entry(word.to_lowercase()).or_default() += if after_end { 1 } else { 2 };
    }
    let mut nouns: Vec<(String, usize)> = counts.into_iter().filter(|(_, c)| *c >= 3).collect();
    nouns.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    for (w, _) in nouns {
        if out.len() >= limit {
            break;
        }
        if let Some(t) = crate::text::normalized_tag(&w) {
            if !STOP.contains(&t.as_str()) && !have.contains(&t) && !out.contains(&t) {
                out.push(t);
            }
        }
    }
    out
}

pub fn parse_tags(answer: &str, limit: usize) -> Vec<String> {
    let mut out = vec![];
    for part in answer.split([',', '\n', ';']) {
        let p = part.trim().trim_start_matches(|c: char| "-*•0123456789.) ".contains(c));
        if let Some(t) = crate::text::normalized_tag(p) {
            if !out.contains(&t) && t.chars().count() <= 30 {
                out.push(t);
            }
        }
        if out.len() >= limit {
            break;
        }
    }
    out
}

// MARK: - Library questions: passages and BM25 (#33)

#[derive(Debug, Clone, Serialize)]
pub struct Passage {
    pub note_id: String,
    pub title: String,
    pub index: usize,
    pub text: String,
}

/// Splits a note's text into passages of about `size` characters at paragraph borders.
pub fn passages(note_id: &str, title: &str, text: &str, size: usize) -> Vec<Passage> {
    let mut out = vec![];
    let mut cur = String::new();
    for para in text.split('\n') {
        if cur.chars().count() + para.chars().count() > size && !cur.trim().is_empty() {
            out.push(Passage { note_id: note_id.into(), title: title.into(), index: out.len(), text: std::mem::take(&mut cur) });
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(para);
    }
    if !cur.trim().is_empty() {
        out.push(Passage { note_id: note_id.into(), title: title.into(), index: out.len(), text: cur });
    }
    out
}

fn terms(s: &str) -> Vec<String> {
    crate::text::fold(s).split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 2).map(|w| w.to_string()).collect()
}

/// The best passages for a question (BM25, title words count double).
pub fn rank_passages(question: &str, all: &[Passage], k: usize) -> Vec<Passage> {
    let q: HashSet<String> = terms(question).into_iter().collect();
    if q.is_empty() || all.is_empty() {
        return vec![];
    }
    let docs: Vec<Vec<String>> = all.iter().map(|p| { let mut t = terms(&p.text); t.extend(terms(&p.title)); t.extend(terms(&p.title)); t }).collect();
    let n = docs.len() as f64;
    let avg = docs.iter().map(|d| d.len()).sum::<usize>() as f64 / n;
    let mut df: HashMap<&str, usize> = HashMap::new();
    for d in &docs {
        let set: HashSet<&str> = d.iter().map(|s| s.as_str()).collect();
        for t in set {
            if q.contains(t) {
                *df.entry(t).or_default() += 1;
            }
        }
    }
    let (k1, b) = (1.2, 0.75);
    let mut scored: Vec<(f64, usize)> = docs
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let mut tf: HashMap<&str, usize> = HashMap::new();
            for t in d {
                if q.contains(t.as_str()) {
                    *tf.entry(t.as_str()).or_default() += 1;
                }
            }
            let len = d.len() as f64;
            let s: f64 = tf
                .iter()
                .map(|(t, f)| {
                    let dfv = *df.get(t).unwrap_or(&0) as f64;
                    let idf = ((n - dfv + 0.5) / (dfv + 0.5) + 1.0).ln();
                    let f = *f as f64;
                    idf * f * (k1 + 1.0) / (f + k1 * (1.0 - b + b * len / avg.max(1.0)))
                })
                .sum();
            (s, i)
        })
        .filter(|(s, _)| *s > 0.0)
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    scored.into_iter().take(k).map(|(_, i)| all[i].clone()).collect()
}

pub const CHAT_SYSTEM: &str = "Du bist der Assistent der Notizen-App Folio auf dem Mac des Nutzers. Antworte auf Deutsch, knapp und sachlich, in Markdown. Stütze dich auf die mitgegebenen Notizen; erfinde nichts. Wenn du Notizen nutzt, nenne sie mit [1], [2] … entsprechend der Nummer der Quelle. Wenn die Notizen keine Antwort enthalten, sag das.";

/// The prompt for a question with numbered sources.
pub fn library_prompt(question: &str, sources: &[Passage]) -> String {
    let mut s = String::from("Quellen aus den Notizen:\n");
    for (i, p) in sources.iter().enumerate() {
        s.push_str(&format!("\n[{}] {}\n{}\n", i + 1, p.title, p.text.chars().take(1800).collect::<String>()));
    }
    s.push_str(&format!("\nFrage: {question}"));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_only() {
        assert!(Ollama::new("http://localhost:11434", "m").is_ok());
        assert!(Ollama::new("127.0.0.1:11434", "m").is_ok());
        assert!(Ollama::new("https://example.com", "m").is_err());
    }

    #[test]
    fn segments_split() {
        let s = segments("Hallo\n```py\nprint(1)\n```\n| a | b |\n|---|---|\n| 1 | 2 |\nEnde");
        assert_eq!(s.len(), 4);
        assert!(matches!(&s[2], Segment::Table { rows, .. } if rows[0] == vec!["1", "2"]));
    }

    #[test]
    fn bm25() {
        let ps = vec![
            Passage { note_id: "A".into(), title: "Rezept".into(), index: 0, text: "Tomaten und Nudeln kochen".into() },
            Passage { note_id: "B".into(), title: "Urlaub".into(), index: 0, text: "Strand und Sonne".into() },
        ];
        assert_eq!(rank_passages("Wie koche ich Nudeln?", &ps, 1)[0].note_id, "A");
    }
}
