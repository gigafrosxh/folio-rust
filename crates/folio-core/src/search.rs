//! Search with typed filters (`tag:`, `#tag`, `ordner:`, `typ:`, `seit:`, `name:wert`), sorting and
//! manual order. Same semantics as the Swift app's `SearchFilter` and `ManualOrder`.

use crate::model::NoteMetadata;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub const CONTENT_TYPES: &[(&str, &str, &[&str])] = &[
    ("code", "Code", &["code"]),
    ("bild", "Bilder", &["image", "drawing"]),
    ("pdf", "PDFs", &["pdf"]),
    ("datei", "Dateien & Tabellen", &["file"]),
    ("web", "Webseiten", &["web"]),
    ("audio", "Audio", &["audio"]),
    ("formel", "Formeln", &["math"]),
    ("diagramm", "Diagramme", &["diagram"]),
    ("datenbank", "Datenbank", &["database"]),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Period {
    #[default]
    Any,
    Today,
    Week,
    Month,
    Year,
}

impl Period {
    pub fn label(&self) -> &'static str {
        match self {
            Period::Any => "Beliebig",
            Period::Today => "Heute",
            Period::Week => "Letzte 7 Tage",
            Period::Month => "Letzte 30 Tage",
            Period::Year => "Letztes Jahr",
        }
    }
    /// Start as Swift date, or None.
    pub fn start(&self, now: f64) -> Option<f64> {
        match self {
            Period::Any => None,
            Period::Today => {
                let local = crate::model::to_chrono(now);
                let midnight = local.date_naive().and_hms_opt(0, 0, 0)?;
                let ts = midnight.and_local_timezone(chrono::Local).single()?.timestamp() as f64;
                Some(crate::model::from_unix(ts))
            }
            Period::Week => Some(now - 7.0 * 86_400.0),
            Period::Month => Some(now - 30.0 * 86_400.0),
            Period::Year => Some(now - 365.0 * 86_400.0),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SearchFilter {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub tags: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    #[serde(default)]
    pub period: Period,
    #[serde(default)]
    pub types: BTreeSet<String>,
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}

impl SearchFilter {
    pub fn has_filters(&self) -> bool {
        !self.tags.is_empty() || self.folder.is_some() || self.period != Period::Any || !self.types.is_empty() || !self.properties.is_empty()
    }

    pub fn is_empty(&self) -> bool {
        !self.has_filters() && self.text.trim().is_empty()
    }

    /// Words of the search; quotes keep spaces together and are removed (#224).
    pub fn words(input: &str) -> Vec<String> {
        let mut words = vec![];
        let mut cur = String::new();
        let mut quoted = false;
        for ch in input.chars() {
            if matches!(ch, '"' | '„' | '“' | '”') {
                quoted = !quoted;
                continue;
            }
            if ch == ' ' && !quoted {
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
            } else {
                cur.push(ch);
            }
        }
        if !cur.is_empty() {
            words.push(cur);
        }
        words
    }

    pub fn parse(input: &str) -> SearchFilter {
        let mut f = SearchFilter::default();
        let rest: Vec<String> = Self::words(input).into_iter().filter(|w| !f.apply(w)).collect();
        f.text = rest.join(" ");
        f
    }

    fn apply(&mut self, word: &str) -> bool {
        let lower = word.to_lowercase();
        if let Some(t) = lower.strip_prefix("tag:") {
            if !t.is_empty() {
                self.tags.insert(t.trim_matches('#').to_string());
                return true;
            }
        }
        if let Some(t) = lower.strip_prefix('#') {
            if t.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false) {
                self.tags.insert(t.to_string());
                return true;
            }
        }
        if lower.starts_with("ordner:") && word.chars().count() > 7 {
            self.folder = Some(word.chars().skip(7).collect());
            return true;
        }
        if let Some(t) = lower.strip_prefix("typ:") {
            if let Some((id, _, _)) = CONTENT_TYPES.iter().find(|(id, label, _)| *id == t || label.to_lowercase() == t) {
                self.types.insert(id.to_string());
                return true;
            }
        }
        if let Some(p) = lower.strip_prefix("seit:") {
            let period = match p {
                "heute" => Some(Period::Today),
                "woche" | "7" => Some(Period::Week),
                "monat" | "30" => Some(Period::Month),
                "jahr" | "365" => Some(Period::Year),
                _ => None,
            };
            return match period {
                Some(p) => {
                    self.period = p;
                    true
                }
                None => false,
            };
        }
        if let Some(colon) = word.find(':') {
            let name = &word[..colon];
            if colon > 0 && colon + 1 < word.len() && name.chars().all(|c| c.is_alphabetic() || c == '_' || c == '-') {
                let name_l = name.to_lowercase();
                if !["http", "https", "folio", "mailto", "file"].contains(&name_l.as_str()) {
                    self.properties.insert(name_l, lower[colon + 1..].to_string());
                    return true;
                }
            }
        }
        false
    }

    pub fn merged(&self, other: &SearchFilter) -> SearchFilter {
        let mut f = self.clone();
        f.text = [self.text.as_str(), other.text.as_str()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" ");
        f.tags.extend(other.tags.iter().cloned());
        if other.folder.is_some() {
            f.folder = other.folder.clone();
        }
        if other.period != Period::Any {
            f.period = other.period;
        }
        f.types.extend(other.types.iter().cloned());
        f.properties.extend(other.properties.clone());
        f
    }

    /// Title, text and properties of a note, folded, for repeated searches.
    pub fn haystack(meta: &NoteMetadata) -> String {
        let props: Vec<String> = meta.properties.iter().flatten().map(|p| format!("{}: {}", p.name, p.display_value())).collect();
        let mut s = String::with_capacity(meta.title.len() + meta.search_text.len() + 2);
        s.push_str(&meta.title);
        s.push('\n');
        s.push_str(&meta.search_text);
        if !props.is_empty() {
            s.push('\n');
            s.push_str(&props.join("\n"));
        }
        crate::text::fold(&s)
    }

    pub fn matches(&self, meta: &NoteMetadata, folder_path: &str, kinds: Option<&[String]>, now: f64, haystack: Option<&str>) -> bool {
        self.matches_tags(meta)
            && self.matches_folder(folder_path)
            && self.period.start(now).map(|s| meta.modified >= s).unwrap_or(true)
            && self.matches_types(kinds.or(meta.kinds.as_deref()).unwrap_or(&[]))
            && self.matches_properties(meta)
            && self.matches_text(meta, haystack)
    }

    fn matches_tags(&self, meta: &NoteMetadata) -> bool {
        let note: Vec<String> = meta.tags.iter().map(|t| t.to_lowercase()).collect();
        self.tags.iter().all(|tag| note.iter().any(|t| t == tag || t.starts_with(&format!("{tag}/"))))
    }

    fn matches_folder(&self, folder_path: &str) -> bool {
        let Some(f) = &self.folder else { return true };
        if f.is_empty() {
            return true;
        }
        let f = f.to_lowercase();
        let f = f.trim_matches('/');
        let p = folder_path.to_lowercase();
        p == f || p.starts_with(&format!("{f}/")) || format!("/{p}/").contains(&format!("/{f}/"))
    }

    fn matches_types(&self, kinds: &[String]) -> bool {
        let have: HashSet<&str> = kinds.iter().map(|s| s.as_str()).collect();
        self.types.iter().filter_map(|t| CONTENT_TYPES.iter().find(|(id, _, _)| id == t)).all(|(_, _, ks)| ks.iter().any(|k| have.contains(k)))
    }

    fn matches_properties(&self, meta: &NoteMetadata) -> bool {
        self.properties.iter().all(|(name, value)| {
            let Some(p) = meta.properties.iter().flatten().find(|p| p.name.to_lowercase() == *name) else { return false };
            value == "*" || p.display_value().to_lowercase().contains(value.as_str()) || p.value.to_lowercase().contains(value.as_str())
        })
    }

    fn matches_text(&self, meta: &NoteMetadata, haystack: Option<&str>) -> bool {
        let folded = crate::text::fold(&self.text);
        let words: Vec<&str> = folded.split(' ').filter(|w| !w.is_empty()).collect();
        if words.is_empty() {
            return true;
        }
        let owned;
        let hay = match haystack {
            Some(h) => h,
            None => {
                owned = Self::haystack(meta);
                &owned
            }
        };
        words.iter().all(|w| hay.contains(w))
    }

    /// „rezept · #kochen · Code · Letzte 7 Tage“
    pub fn summary(&self) -> String {
        let mut parts = vec![];
        if !self.text.is_empty() {
            parts.push(format!("„{}“", self.text));
        }
        parts.extend(self.tags.iter().map(|t| format!("#{t}")));
        if let Some(f) = &self.folder {
            parts.push(format!("Ordner {f}"));
        }
        parts.extend(self.types.iter().filter_map(|t| CONTENT_TYPES.iter().find(|(id, _, _)| id == t).map(|(_, l, _)| l.to_string())));
        if self.period != Period::Any {
            parts.push(self.period.label().to_string());
        }
        parts.extend(self.properties.iter().map(|(k, v)| format!("{k}: {}", if v == "*" { "vorhanden" } else { v })));
        if parts.is_empty() { "Alle Notizen".into() } else { parts.join(" · ") }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SortMode {
    #[default]
    Modified,
    Created,
    Title,
    Manual,
}

impl SortMode {
    pub fn groups_by_date(&self) -> bool {
        matches!(self, SortMode::Modified | SortMode::Created)
    }
}

pub mod manual_order {
    use std::collections::{HashMap, HashSet};
    use std::hash::Hash;

    pub fn apply<T, K: Hash + Eq + Clone>(items: Vec<T>, order: &[K], key: impl Fn(&T) -> K, new_first: bool) -> Vec<T> {
        let rank: HashMap<K, usize> = order.iter().enumerate().rev().map(|(i, k)| (k.clone(), i)).collect();
        let (mut known, new): (Vec<T>, Vec<T>) = items.into_iter().partition(|t| rank.contains_key(&key(t)));
        known.sort_by_key(|t| rank[&key(t)]);
        if new_first { new.into_iter().chain(known).collect() } else { known.into_iter().chain(new).collect() }
    }

    pub fn moving<K: PartialEq + Clone>(item: &K, before: Option<&K>, visible: &[K]) -> Vec<K> {
        if Some(item) == before {
            return visible.to_vec();
        }
        let mut list: Vec<K> = visible.iter().filter(|k| *k != item).cloned().collect();
        match before.and_then(|t| list.iter().position(|k| k == t)) {
            Some(i) => list.insert(i, item.clone()),
            None => list.push(item.clone()),
        }
        list
    }

    /// Puts a new order of some items back into the whole list (#170).
    pub fn merging<K: Hash + Eq + Clone>(part: &[K], full: &[K]) -> Vec<K> {
        let known: HashSet<&K> = full.iter().collect();
        let mut seen = HashSet::new();
        let mut queue: std::collections::VecDeque<K> = part.iter().filter(|k| known.contains(k) && seen.insert((*k).clone())).cloned().collect();
        let moved: HashSet<K> = queue.iter().cloned().collect();
        full.iter().map(|k| if moved.contains(k) { queue.pop_front().unwrap() } else { k.clone() }).collect()
    }

    pub fn unique<K: Hash + Eq + Clone>(items: &[K]) -> Vec<K> {
        let mut seen = HashSet::new();
        items.iter().filter(|k| seen.insert((*k).clone())).cloned().collect()
    }

    pub fn stepping<K: PartialEq + Clone>(item: &K, delta: i64, visible: &[K]) -> Vec<K> {
        let Some(i) = visible.iter().position(|k| k == item) else { return visible.to_vec() };
        let j = (i as i64 + delta).clamp(0, visible.len() as i64 - 1) as usize;
        let mut list = visible.to_vec();
        let x = list.remove(i);
        list.insert(j, x);
        list
    }
}

/// Ranks quick-switcher entries: "#…" only tags, "/…" only folders; notes first on equal score (#88).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SwitcherEntry {
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub key: String,
}

pub fn rank_switcher(entries: Vec<SwitcherEntry>, query: &str) -> Vec<SwitcherEntry> {
    let mut q = query.trim().to_string();
    let mut only = None;
    if let Some(r) = q.strip_prefix('#') {
        only = Some("tag");
        q = r.to_string();
    } else if let Some(r) = q.strip_prefix('/') {
        only = Some("folder");
        q = r.to_string();
    }
    let pool: Vec<SwitcherEntry> = entries.into_iter().filter(|e| only.map(|k| e.kind == k).unwrap_or(true)).collect();
    if q.is_empty() {
        return pool;
    }
    let order: HashMap<&str, i32> = [("note", 0), ("folder", 1), ("tag", 2)].into_iter().collect();
    let mut scored: Vec<(SwitcherEntry, i64)> = pool
        .into_iter()
        .filter_map(|e| {
            let text = if e.kind == "folder" && !e.detail.is_empty() { e.detail.clone() } else { e.title.clone() };
            crate::text::fuzzy_score(&q, &text).map(|s| (e, s))
        })
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(order.get(a.0.kind.as_str()).cmp(&order.get(b.0.kind.as_str()))));
    scored.into_iter().map(|(e, _)| e).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_filters() {
        let f = SearchFilter::parse(r#"rezept #kochen ordner:"Apple Notizen" typ:code seit:7 status:offen https://x.de"#);
        assert_eq!(f.text, "rezept https://x.de");
        assert!(f.tags.contains("kochen"));
        assert_eq!(f.folder.as_deref(), Some("Apple Notizen"));
        assert!(f.types.contains("code"));
        assert_eq!(f.period, Period::Week);
        assert_eq!(f.properties.get("status").map(|s| s.as_str()), Some("offen"));
    }

    #[test]
    fn folder_match() {
        let f = SearchFilter { folder: Some("Schule".into()), ..Default::default() };
        assert!(f.matches_folder("Apple Notizen/Schule/Deutsch"));
        assert!(!f.matches_folder("Schulen"));
    }

    #[test]
    fn manual() {
        let v = manual_order::moving(&3, Some(&1), &[1, 2, 3]);
        assert_eq!(v, vec![3, 1, 2]);
        assert_eq!(manual_order::merging(&[3, 1], &[1, 2, 3, 4]), vec![3, 2, 1, 4]);
    }
}
