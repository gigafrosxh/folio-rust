//! A small HTML reader for imported text (Apple Notes bodies, Markdown output): paragraphs, character
//! formats, links, headings and lists become `RichText` with Folio's plain-text list markers.

use crate::rtf::{Para, RichText, Run};

#[derive(Clone, Default)]
struct Fmt {
    b: bool,
    i: bool,
    u: bool,
    s: bool,
    mono: bool,
    size: Option<f32>,
    link: Option<String>,
}

enum ListKind {
    Bullet,
    Numbered(usize),
    Check,
}

pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        if let Some(end) = rest.find(';').filter(|e| *e < 12) {
            let ent = &rest[1..end];
            let c = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" | "#39" => Some('\''),
                "nbsp" => Some(' '),
                "ndash" => Some('–'),
                "mdash" => Some('—'),
                "hellip" => Some('…'),
                "bull" => Some('•'),
                "auml" => Some('ä'),
                "ouml" => Some('ö'),
                "uuml" => Some('ü'),
                "Auml" => Some('Ä'),
                "Ouml" => Some('Ö'),
                "Uuml" => Some('Ü'),
                "szlig" => Some('ß'),
                "euro" => Some('€'),
                _ if ent.starts_with("#x") || ent.starts_with("#X") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
                _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
                _ => None,
            };
            if let Some(c) = c {
                out.push(c);
                rest = &rest[end + 1..];
                continue;
            }
        }
        out.push('&');
        rest = &rest[1..];
    }
    out.push_str(rest);
    out
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let i = lower.find(&format!("{name}="))?;
    let rest = &tag[i + name.len() + 1..];
    let (q, rest) = match rest.chars().next()? {
        '"' => ('"', &rest[1..]),
        '\'' => ('\'', &rest[1..]),
        _ => (' ', rest),
    };
    let end = rest.find(|c| c == q || (q == ' ' && c == '>')).unwrap_or(rest.len());
    Some(decode_entities(&rest[..end]))
}

/// Converts HTML to rich text. `body_size` is used for heading sizes.
pub fn to_rich_text(html: &str, body_size: f32) -> RichText {
    let mut paras: Vec<Para> = vec![];
    let mut cur = Para::default();
    let mut stack: Vec<(String, Fmt)> = vec![];
    let mut fmt = Fmt::default();
    let mut lists: Vec<ListKind> = vec![];
    let mut pre = false;
    let mut skip_depth = 0usize;
    let mut i = 0;
    let bytes = html.as_bytes();
    let flush = |paras: &mut Vec<Para>, cur: &mut Para, force: bool| {
        if force || !cur.runs.is_empty() {
            paras.push(std::mem::take(cur));
        }
    };
    while i < bytes.len() {
        if bytes[i] == b'<' {
            let end = match html[i..].find('>') {
                Some(e) => i + e,
                None => break,
            };
            let tag = &html[i + 1..end];
            i = end + 1;
            if tag.starts_with("!--") {
                if let Some(e) = html[i - tag.len() - 2..].find("-->") {
                    i = (i - tag.len() - 2 + e + 3).max(i);
                }
                continue;
            }
            let closing = tag.starts_with('/');
            let name = tag.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/' || c == '>').next().unwrap_or("").to_lowercase();
            if matches!(name.as_str(), "script" | "style" | "head" | "title") {
                if closing {
                    skip_depth = skip_depth.saturating_sub(1);
                } else if !tag.ends_with('/') {
                    skip_depth += 1;
                }
                continue;
            }
            if skip_depth > 0 {
                continue;
            }
            if closing {
                match name.as_str() {
                    "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "blockquote" | "tr" => {
                        let force = name != "div" || !cur.runs.is_empty();
                        flush(&mut paras, &mut cur, force)
                    }
                    "ul" | "ol" => {
                        lists.pop();
                    }
                    "pre" => {
                        pre = false;
                        flush(&mut paras, &mut cur, false);
                    }
                    _ => {}
                }
                if let Some(pos) = stack.iter().rposition(|(n, _)| *n == name) {
                    fmt = stack[pos].1.clone();
                    stack.truncate(pos);
                }
                continue;
            }
            let self_closing = tag.ends_with('/') || matches!(name.as_str(), "br" | "img" | "hr" | "input" | "meta" | "link");
            if !self_closing {
                stack.push((name.clone(), fmt.clone()));
            }
            match name.as_str() {
                "b" | "strong" => fmt.b = true,
                "i" | "em" => fmt.i = true,
                "u" => fmt.u = true,
                "s" | "strike" | "del" => fmt.s = true,
                "code" | "tt" | "kbd" => fmt.mono = true,
                "pre" => {
                    flush(&mut paras, &mut cur, false);
                    fmt.mono = true;
                    pre = true;
                }
                "a" => fmt.link = attr(tag, "href").filter(|h| crate::markdown::is_safe_link(h)),
                "h1" => { flush(&mut paras, &mut cur, false); fmt.b = true; fmt.size = Some(body_size * 1.85); }
                "h2" => { flush(&mut paras, &mut cur, false); fmt.b = true; fmt.size = Some(body_size * 1.4); }
                "h3" | "h4" | "h5" | "h6" => { flush(&mut paras, &mut cur, false); fmt.b = true; fmt.size = Some(body_size * 1.15); }
                "p" | "div" | "blockquote" | "tr" => flush(&mut paras, &mut cur, false),
                "br" => flush(&mut paras, &mut cur, true),
                "hr" => {
                    flush(&mut paras, &mut cur, false);
                    paras.push(Para::plain("———"));
                }
                "ul" => {
                    let class = attr(tag, "class").unwrap_or_default();
                    lists.push(if class.contains("checklist") || class.contains("check") { ListKind::Check } else { ListKind::Bullet });
                }
                "ol" => lists.push(ListKind::Numbered(attr(tag, "start").and_then(|s| s.parse().ok()).unwrap_or(1))),
                "li" => {
                    flush(&mut paras, &mut cur, false);
                    let depth = lists.len().saturating_sub(1);
                    let checked = attr(tag, "class").map(|c| c.contains("checked") || c.contains("done")).unwrap_or(false);
                    let marker = match lists.last_mut() {
                        Some(ListKind::Numbered(n)) => {
                            let m = format!("{n}. ");
                            *n += 1;
                            m
                        }
                        Some(ListKind::Check) => if checked { "☑ ".into() } else { "☐ ".into() },
                        _ => "• ".into(),
                    };
                    cur.runs.push(Run { text: format!("{}{}", "\t".repeat(depth), marker), ..Default::default() });
                }
                "td" | "th" => {
                    if !cur.runs.is_empty() {
                        cur.runs.push(Run { text: "\t".into(), ..Default::default() });
                    }
                }
                "input" => {
                    if attr(tag, "type").as_deref() == Some("checkbox") {
                        let checked = tag.to_lowercase().contains("checked");
                        cur.runs.push(Run { text: if checked { "☑ ".into() } else { "☐ ".into() }, ..Default::default() });
                    }
                }
                _ => {}
            }
            continue;
        }
        let next = html[i..].find('<').map(|n| i + n).unwrap_or(html.len());
        let raw = &html[i..next];
        i = next;
        if skip_depth > 0 {
            continue;
        }
        let text = decode_entities(raw);
        if pre {
            for (k, line) in text.split('\n').enumerate() {
                if k > 0 {
                    flush(&mut paras, &mut cur, true);
                }
                push_text(&mut cur, line, &fmt);
            }
        } else {
            let collapsed: String = {
                let mut s = String::with_capacity(text.len());
                let mut space = false;
                for c in text.chars() {
                    if c.is_whitespace() && c != '\u{a0}' {
                        if !space {
                            s.push(' ');
                        }
                        space = true;
                    } else {
                        s.push(if c == '\u{a0}' { ' ' } else { c });
                        space = false;
                    }
                }
                s
            };
            let t = if cur.runs.is_empty() { collapsed.trim_start().to_string() } else { collapsed };
            if !t.is_empty() {
                push_text(&mut cur, &t, &fmt);
            }
        }
    }
    flush(&mut paras, &mut cur, false);
    // Trailing spaces at paragraph ends come from source formatting.
    for p in paras.iter_mut() {
        if let Some(last) = p.runs.last_mut() {
            let t = last.text.trim_end_matches(' ').to_string();
            last.text = t;
        }
    }
    let mut rt = RichText { paras };
    rt.normalize();
    rt
}

fn push_text(cur: &mut Para, text: &str, f: &Fmt) {
    cur.runs.push(Run {
        text: text.to_string(),
        b: f.b,
        i: f.i,
        u: f.u || f.link.is_some(),
        s: f.s,
        size: f.size,
        font: if f.mono { Some("mono".into()) } else { None },
        color: None,
        bg: None,
        link: f.link.clone(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let rt = to_rich_text("<div><h1>Titel</h1></div><div>Hallo <b>fett</b> &amp; <a href=\"https://x.de\">link</a></div><ul><li>eins</li><li>zwei</li></ul><ol><li>a</li></ol><div><br></div><div>Ende</div>", 15.0);
        assert_eq!(rt.plain_text(), "Titel\nHallo fett & link\n• eins\n• zwei\n1. a\n\nEnde");
        assert!(rt.paras[0].runs[0].b);
    }
}
