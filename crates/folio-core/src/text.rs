//! Text helpers: tags, wiki links, titles, find in text, statistics, fuzzy matching, templates.

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashSet;
use unicode_normalization::UnicodeNormalization;

// MARK: - Tags

static TAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"#(\p{L}[\p{L}\p{N}_\-/]*)").unwrap());

#[derive(Debug, Clone, PartialEq)]
pub struct TagMatch {
    pub tag: String,
    /// Byte range of `#tag` in the text.
    pub start: usize,
    pub end: usize,
}

/// `#tags`: start with a letter; letters, digits, `_`, `-`, `/`; not glued to a preceding word (`C#`, `a#b`).
pub fn tag_matches(text: &str) -> Vec<TagMatch> {
    let mut out = vec![];
    for m in TAG_RE.captures_iter(text) {
        let whole = m.get(0).unwrap();
        if let Some(prev) = text[..whole.start()].chars().next_back() {
            if prev.is_alphanumeric() || prev == '_' || prev == '#' || prev == '&' || prev == '/' {
                continue;
            }
        }
        let mut tag = m.get(1).unwrap().as_str().to_string();
        while tag.ends_with('-') || tag.ends_with('/') {
            tag.pop();
        }
        if tag.is_empty() {
            continue;
        }
        let end = whole.start() + 1 + tag.len();
        out.push(TagMatch { tag, start: whole.start(), end });
    }
    out
}

/// Unique, lowercased tags in order of first appearance.
pub fn tags(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = vec![];
    for m in tag_matches(text) {
        let k = m.tag.to_lowercase();
        if seen.insert(k.clone()) {
            out.push(k);
        }
    }
    out
}

/// Renames (or with `None` removes) a tag and its nested children in plain text.
pub fn rename_tag(text: &str, tag: &str, new_tag: Option<&str>) -> String {
    let old = tag.to_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for m in tag_matches(text) {
        let t = m.tag.to_lowercase();
        if t != old && !t.starts_with(&format!("{old}/")) {
            continue;
        }
        out.push_str(&text[last..m.start]);
        let mut end = m.end;
        match new_tag {
            Some(n) => {
                out.push('#');
                out.push_str(n);
                out.push_str(&m.tag[old.len().min(m.tag.len())..]);
            }
            None => {
                if text[end..].starts_with(' ') {
                    end += 1;
                }
            }
        }
        last = end;
    }
    out.push_str(&text[last..]);
    out
}

/// A tag as typed ("#Neu Tag" → "neu-tag"), or None when unusable.
pub fn normalized_tag(input: &str) -> Option<String> {
    let s = input.trim().trim_start_matches('#').replace(' ', "-").to_lowercase();
    if !s.chars().next()?.is_alphabetic() {
        return None;
    }
    if tags(&format!("#{s}")) == vec![s.clone()] { Some(s) } else { None }
}

// MARK: - Wiki links

static WIKI_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(!?)\[\[([^\[\]\|\n]+?)(?:\|([^\[\]\n]+?))?\]\]").unwrap());

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct WikiLink {
    pub start: usize,
    pub end: usize,
    pub target: String,
    pub alias: Option<String>,
    pub is_embed: bool,
}

pub fn wiki_links(text: &str) -> Vec<WikiLink> {
    WIKI_RE
        .captures_iter(text)
        .filter_map(|c| {
            let target = c.get(2)?.as_str().trim().to_string();
            if target.is_empty() {
                return None;
            }
            let alias = c.get(3).map(|a| a.as_str().trim().to_string()).filter(|a| !a.is_empty());
            let m = c.get(0)?;
            Some(WikiLink { start: m.start(), end: m.end(), target, alias, is_embed: !c.get(1)?.as_str().is_empty() })
        })
        .collect()
}

/// Distinct link targets in order (embeds included).
pub fn wiki_targets(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    wiki_links(text).into_iter().map(|l| l.target).filter(|t| seen.insert(title_key(t))).collect()
}

/// How titles are compared: case-, accent- and surrounding-space-insensitive.
pub fn title_key(title: &str) -> String {
    title.trim().nfd().filter(|c| !unicode_normalization::char::is_combining_mark(*c)).collect::<String>().to_lowercase()
}

pub fn note_link(id: &str) -> String {
    format!("folio://note/{id}")
}

/// The note id of a `folio://note/<id>` link.
pub fn note_id_from_link(url: &str) -> Option<String> {
    let rest = url.strip_prefix("folio://note/").or_else(|| url.strip_prefix("FOLIO://NOTE/"))?;
    let id = rest.split(['?', '#', '/']).next()?.to_uppercase();
    if uuid::Uuid::parse_str(&id).is_ok() { Some(id) } else { None }
}

// MARK: - Titles

static LIST_PREFIX: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[\t ]*(?:[•◦▪‣☐☑✓✔–\-*]\u{FE0E}?[\t ]+|\d{1,3}[.)][\t ]+|\d{1,3}\t+)").unwrap());
static SPACES: Lazy<Regex> = Lazy::new(|| Regex::new(r"[\t ]+").unwrap());

/// A text line without its list marker and with single spaces (#96).
pub fn plain_line(line: &str) -> String {
    let s = LIST_PREFIX.replace(line, "");
    SPACES.replace_all(&s, " ").trim().to_string()
}

/// Quick capture's date lines ("— 20. Okt. 2026, 19:31 —") separate entries; they are no title (#95).
pub fn is_capture_separator(line: &str) -> bool {
    line.starts_with("— ") && line.ends_with(" —") && line.chars().count() < 60
}

/// Title (first real line, ≤ 120 chars) and preview (next three lines, ≤ 200 chars) of a note's text.
pub fn title_and_preview(full_text: &str) -> (Option<String>, String) {
    let lines: Vec<String> = full_text
        .split(['\n', '\r', '\u{2028}', '\u{2029}'])
        .map(plain_line)
        .filter(|l| !l.is_empty() && !is_capture_separator(l))
        .collect();
    let title = lines.first().map(|l| l.chars().take(120).collect());
    let preview: String = lines.iter().skip(1).take(3).cloned().collect::<Vec<_>>().join(" ").chars().take(200).collect();
    (title, preview)
}

pub fn sanitize_filename(name: &str) -> String {
    let mut s: String = name.chars().map(|c| if matches!(c, '/' | ':' | '\\' | '\0') || c.is_control() { '-' } else { c }).collect();
    s = s.trim().to_string();
    while s.starts_with('.') {
        s.remove(0);
    }
    if s.is_empty() {
        s = "Ohne Titel".into();
    }
    s.chars().take(80).collect()
}

// MARK: - Find in text

/// Byte ranges of `query` in `text`; case- and accent-insensitive unless `case_sensitive`.
pub fn find_ranges(query: &str, text: &str, case_sensitive: bool, whole_word: bool) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return vec![];
    }
    let fold = |s: &str| -> Vec<(usize, char)> {
        s.char_indices()
            .map(|(i, c)| {
                let c = if case_sensitive { c } else { fold_char(c) };
                (i, c)
            })
            .collect()
    };
    let t = fold(text);
    let q: Vec<char> = fold(query).into_iter().map(|(_, c)| c).collect();
    let mut out = vec![];
    let mut i = 0;
    while i + q.len() <= t.len() {
        if (0..q.len()).all(|k| t[i + k].1 == q[k]) {
            let start = t[i].0;
            let end = if i + q.len() < t.len() { t[i + q.len()].0 } else { text.len() };
            let ok = !whole_word || {
                let before = text[..start].chars().next_back();
                let after = text[end..].chars().next();
                !before.map(|c| c.is_alphanumeric() || c == '_').unwrap_or(false) && !after.map(|c| c.is_alphanumeric() || c == '_').unwrap_or(false)
            };
            if ok {
                out.push((start, end));
                i += q.len().max(1);
                continue;
            }
        }
        i += 1;
    }
    out
}

pub fn fold_char(c: char) -> char {
    let base = c.to_string().nfd().next().unwrap_or(c);
    base.to_lowercase().next().unwrap_or(base)
}

pub fn fold(s: &str) -> String {
    s.nfd().filter(|c| !unicode_normalization::char::is_combining_mark(*c)).collect::<String>().to_lowercase()
}

// MARK: - Statistics (#19)

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteStatistics {
    pub characters: usize,
    pub characters_without_spaces: usize,
    pub words: usize,
    pub paragraphs: usize,
    pub code_blocks: usize,
    pub code_lines: usize,
    pub reading_minutes: usize,
}

pub fn word_count(text: &str) -> usize {
    text.lines()
        .map(|line| {
            let mut words: Vec<&str> = line.split_whitespace().filter(|w| w.chars().any(|c| c.is_alphanumeric())).collect();
            if let Some(first) = words.first() {
                let is_num = (first.ends_with('.') || first.ends_with(')')) && {
                    let d = &first[..first.len() - 1];
                    !d.is_empty() && d.len() <= 3 && d.chars().all(|c| c.is_ascii_digit())
                };
                if is_num && (words.len() > 1 || line.contains(char::is_whitespace)) {
                    words.remove(0);
                }
            }
            words.len()
        })
        .sum()
}

pub fn statistics(texts: &[String], code: &[String]) -> NoteStatistics {
    let mut s = NoteStatistics::default();
    for t in texts {
        s.characters += t.chars().count();
        s.characters_without_spaces += t.chars().filter(|c| !c.is_whitespace()).count();
        s.words += word_count(t);
        s.paragraphs += t.lines().filter(|l| !l.trim().is_empty()).count();
    }
    s.code_blocks = code.iter().filter(|c| !c.trim().is_empty()).count();
    s.code_lines = code.iter().map(|c| c.lines().filter(|l| !l.trim().is_empty()).count()).sum();
    s.reading_minutes = if s.words == 0 { 0 } else { ((s.words as f64 / 200.0).round() as usize).max(1) };
    s
}

// MARK: - Fuzzy matching (#20)

pub fn fuzzy_score(query: &str, text: &str) -> Option<i64> {
    let q: Vec<char> = query.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let best = [fuzzy_pass(&q, &t, false), fuzzy_pass(&q, &t, true)].into_iter().flatten().max()?;
    Some(best * 10 - (t.len().min(200) / 10) as i64)
}

fn is_word_start(t: &[char], i: usize) -> bool {
    i == 0 || !t[i - 1].is_alphanumeric()
}

fn fuzzy_pass(q: &[char], t: &[char], prefer_word_starts: bool) -> Option<i64> {
    let (mut score, mut last, mut pos) = (0i64, -2i64, 0usize);
    for &c in q {
        let mut found = None;
        if prefer_word_starts {
            found = (pos..t.len()).find(|&i| t[i] == c && is_word_start(t, i));
        }
        if found.is_none() {
            found = (pos..t.len()).find(|&i| t[i] == c);
        }
        let i = found?;
        let mut s = 1;
        if i as i64 == last + 1 {
            s += 5;
        }
        if i == 0 {
            s += 8;
        } else if is_word_start(t, i) {
            s += 4;
        }
        score += s;
        last = i as i64;
        pos = i + 1;
    }
    Some(score)
}

// MARK: - Template variables (#118)

static VAR_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\{\{\s*([A-Za-zäöüÄÖÜ]+)\s*(?::([^}]+))?\}\}").unwrap());

const WEEKDAYS: [&str; 7] = ["Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag", "Sonntag"];
const MONTHS: [&str; 12] = ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"];
const MONTHS_SHORT: [&str; 12] = ["Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sept.", "Okt.", "Nov.", "Dez."];

pub fn german_date_short(d: chrono::NaiveDate) -> String {
    use chrono::Datelike;
    format!("{}. {} {}", d.day(), MONTHS_SHORT[d.month0() as usize], d.year())
}

/// Formats with a Unicode date pattern subset (yyyy, yy, MMMM, MMM, MM, M, dd, d, EEEE, EEE, HH, H, mm, ss).
pub fn format_date(dt: &chrono::DateTime<chrono::Local>, pattern: &str) -> String {
    use chrono::{Datelike, Timelike};
    let mut out = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    let mut quoted = false;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            quoted = !quoted;
            i += 1;
            continue;
        }
        if quoted || !c.is_ascii_alphabetic() {
            out.push(c);
            i += 1;
            continue;
        }
        let mut n = 1;
        while i + n < chars.len() && chars[i + n] == c {
            n += 1;
        }
        let s = match (c, n) {
            ('y', 2) => format!("{:02}", dt.year() % 100),
            ('y', _) => format!("{}", dt.year()),
            ('M', 1) => format!("{}", dt.month()),
            ('M', 2) => format!("{:02}", dt.month()),
            ('M', 3) => MONTHS_SHORT[dt.month0() as usize].to_string(),
            ('M', _) => MONTHS[dt.month0() as usize].to_string(),
            ('d', 1) => format!("{}", dt.day()),
            ('d', _) => format!("{:02}", dt.day()),
            ('E', n) if n >= 4 => WEEKDAYS[dt.weekday().num_days_from_monday() as usize].to_string(),
            ('E', _) => WEEKDAYS[dt.weekday().num_days_from_monday() as usize][..2].to_string(),
            ('H', 1) => format!("{}", dt.hour()),
            ('H', _) => format!("{:02}", dt.hour()),
            ('m', _) => format!("{:02}", dt.minute()),
            ('s', _) => format!("{:02}", dt.second()),
            _ => std::iter::repeat(c).take(n).collect(),
        };
        out.push_str(&s);
        i += n;
    }
    out
}

/// Fills `{{datum}}`, `{{uhrzeit}}`, `{{wochentag}}`, `{{kw}}`, `{{jahr}}`, `{{monat}}`, `{{titel}}`
/// (and English names, `{{datum:yyyy-MM-dd}}`); returns the text and the byte offset of `{{cursor}}`.
pub fn fill_template(text: &str, date: &chrono::DateTime<chrono::Local>, title: &str) -> (String, Option<usize>) {
    use chrono::Datelike;
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut cursor = None;
    for c in VAR_RE.captures_iter(text) {
        let m = c.get(0).unwrap();
        let name = c.get(1).unwrap().as_str().to_lowercase();
        let fmt = c.get(2).map(|f| f.as_str().trim().to_string());
        let value = match name.as_str() {
            "datum" | "date" => Some(format_date(date, fmt.as_deref().unwrap_or("dd.MM.yyyy"))),
            "uhrzeit" | "time" => Some(format_date(date, fmt.as_deref().unwrap_or("HH:mm"))),
            "wochentag" | "weekday" => Some(format_date(date, "EEEE")),
            "kw" | "week" => Some(format!("{}", date.iso_week().week())),
            "jahr" | "year" => Some(format_date(date, "yyyy")),
            "monat" | "month" => Some(format_date(date, "MMMM")),
            "titel" | "title" => Some(title.to_string()),
            "cursor" => Some(String::new()),
            _ => None,
        };
        let Some(v) = value else { continue };
        out.push_str(&text[last..m.start()]);
        if name == "cursor" && cursor.is_none() {
            cursor = Some(out.len());
        }
        out.push_str(&v);
        last = m.end();
    }
    out.push_str(&text[last..]);
    (out, cursor)
}

/// "2026-10-21": the title of a day's note.
pub fn daily_title(date: chrono::NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Unlinked mentions: does `title` appear as a whole word in `text`, outside `[[…]]`?
pub fn mentions(title: &str, text: &str) -> usize {
    let t = title.trim();
    if t.chars().count() < 3 {
        return 0;
    }
    let pattern = format!(r"(?i)(?:^|[^\p{{L}}\p{{N}}]){}(?:$|[^\p{{L}}\p{{N}}])", regex::escape(t));
    let Ok(re) = Regex::new(&pattern) else { return 0 };
    let linked: Vec<(usize, usize)> = wiki_links(text).iter().map(|l| (l.start, l.end)).collect();
    re.find_iter(text).filter(|m| !linked.iter().any(|(s, e)| m.start() < *e && *s < m.end())).count()
}

/// Capture date line like "— 20. Okt. 2026, 19:31 —".
pub fn capture_date_text(now: &chrono::DateTime<chrono::Local>) -> String {
    format!("{}, {}", german_date_short(now.date_naive()), format_date(now, "HH:mm"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_rules() {
        assert_eq!(tags("#Rezept und #projekt/folio, C# a#b #1x #ok-"), vec!["rezept", "projekt/folio", "ok"]);
        assert_eq!(rename_tag("x #projekt/alpha #projekt y", "projekt", Some("arbeit")), "x #arbeit/alpha #arbeit y");
        assert_eq!(rename_tag("a #weg b", "weg", None), "a b");
        assert_eq!(normalized_tag("#Neu Tag").as_deref(), Some("neu-tag"));
    }

    #[test]
    fn wiki() {
        let l = wiki_links("Siehe [[Rezepte|hier]] und ![[Plan]]");
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].alias.as_deref(), Some("hier"));
        assert!(l[1].is_embed);
        assert_eq!(title_key(" Äpfel "), "apfel");
    }

    #[test]
    fn titles() {
        assert_eq!(plain_line("  • Hallo   Welt"), "Hallo Welt");
        assert_eq!(plain_line("10 km/h"), "10 km/h");
        let (t, p) = title_and_preview("— 20. Okt. 2026, 19:31 —\n☐ Einkaufen\nMilch\n\nBrot");
        assert_eq!(t.as_deref(), Some("Einkaufen"));
        assert_eq!(p, "Milch Brot");
    }

    #[test]
    fn find() {
        assert_eq!(find_ranges("apfel", "Äpfel und apfelbaum", false, false).len(), 2);
        assert_eq!(find_ranges("apfel", "Äpfel und apfelbaum", false, true).len(), 1);
    }

    #[test]
    fn templates() {
        let d = chrono::Local::now();
        let (t, c) = fill_template("A {{titel}} {{cursor}}B {{unbekannt}}", &d, "X");
        assert_eq!(t, "A X B {{unbekannt}}");
        assert_eq!(c, Some(4));
    }

    #[test]
    fn words() {
        assert_eq!(word_count("1. E-Mail an Anna\n– 1.000 Euro"), 5);
    }
}
