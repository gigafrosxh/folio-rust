//! Markdown import and export (#4). Export writes Markdown plus the attachments (put in a folder
//! `<Titel> Dateien/` by the caller); import reads headings, lists, checklists, bold/italic/strike,
//! inline code, links, fenced code (```mermaid → diagram, ```math / $$ → formula) and local images.

use crate::model::{kind, BlockRecord, NoteFile};
use crate::rtf::{Para, RichText, Run, WriteOptions};
use once_cell::sync::Lazy;
use regex::Regex;

/// Links that may stay clickable in imported text (#161).
pub fn is_safe_link(link: &str) -> bool {
    let l = link.trim().to_lowercase();
    ["http:", "https:", "mailto:", "folio:"].iter().any(|s| l.starts_with(s))
}

pub fn safe_file_name(name: &str) -> String {
    crate::text::sanitize_filename(name)
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '*' | '_' | '`' | '[' | ']') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn heading_prefix(p: &Para, base: f32) -> &'static str {
    let size = p.runs.iter().filter(|r| !r.text.trim().is_empty()).filter_map(|r| r.size).fold(0.0f32, f32::max);
    let bold = p.runs.iter().filter(|r| !r.text.trim().is_empty()).all(|r| r.b);
    if size >= base * 1.6 {
        "# "
    } else if size >= base * 1.3 {
        "## "
    } else if size >= base * 1.08 && bold {
        "### "
    } else {
        ""
    }
}

fn inline(r: &Run, in_heading: bool) -> String {
    if r.text.is_empty() {
        return String::new();
    }
    if r.font.as_deref() == Some("mono") {
        return format!("`{}`", r.text.replace('`', "'"));
    }
    let lead = r.text.len() - r.text.trim_start().len();
    let trail = r.text.len() - r.text.trim_end().len();
    let core = r.text.trim();
    if core.is_empty() {
        return r.text.clone();
    }
    let mut s = escape(core);
    if r.b && !in_heading {
        s = format!("**{s}**");
    }
    if r.i {
        s = format!("*{s}*");
    }
    if r.s {
        s = format!("~~{s}~~");
    }
    if let Some(l) = &r.link {
        s = format!("[{s}]({})", l.replace(' ', "%20"));
    }
    format!("{}{}{}", &r.text[..lead], s, &r.text[r.text.len() - trail..])
}

/// One paragraph as a Markdown line; lists use Markdown markers, tabs become indentation.
fn para_line(p: &Para, base: f32) -> String {
    let h = heading_prefix(p, base);
    let mut line: String = p.runs.iter().map(|r| inline(r, !h.is_empty())).collect();
    let tabs = line.chars().take_while(|c| *c == '\t').count();
    let rest = line[tabs..].to_string();
    let indent = "  ".repeat(tabs);
    let list = if let Some(r) = rest.strip_prefix("• ").or_else(|| rest.strip_prefix("– ")) {
        Some(format!("- {r}"))
    } else if let Some(r) = rest.strip_prefix("☐ ") {
        Some(format!("- [ ] {r}"))
    } else if let Some(r) = rest.strip_prefix("☑ ") {
        Some(format!("- [x] {r}"))
    } else {
        None
    };
    if let Some(l) = list {
        return format!("{indent}{l}");
    }
    if tabs > 0 {
        line = format!("{indent}{rest}");
    }
    if line.trim().is_empty() {
        return String::new();
    }
    format!("{h}{line}")
}

pub fn rich_text_to_markdown(rt: &RichText, base: f32) -> String {
    rt.paras.iter().map(|p| para_line(p, base)).collect::<Vec<_>>().join("\n")
}

/// Markdown for a note and the attachments to write next to it (file name, attachment index).
pub fn export(file: &NoteFile, files_folder: &str, base: f32) -> (String, Vec<(String, usize)>) {
    let mut parts: Vec<String> = vec![];
    let mut files: Vec<(String, usize)> = vec![];
    let mut used_names: Vec<String> = vec![];
    let mut link_for = |id: &Option<String>, files: &mut Vec<(String, usize)>| -> Option<(String, String)> {
        let id = id.as_ref()?;
        let idx = file.attachments.iter().position(|a| &a.header.id == id)?;
        let mut name = safe_file_name(&file.attachments[idx].header.filename);
        let mut n = 2;
        while used_names.contains(&name) {
            let p = std::path::Path::new(&file.attachments[idx].header.filename);
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("Datei");
            let ext = p.extension().and_then(|s| s.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
            name = format!("{stem} {n}{ext}");
            n += 1;
        }
        used_names.push(name.clone());
        files.push((name.clone(), idx));
        Some((name.clone(), format!("{}/{}", files_folder.replace(' ', "%20"), name.replace(' ', "%20"))))
    };
    for b in &file.body.blocks {
        let code = b.code.clone().unwrap_or_default();
        match b.kind.as_str() {
            kind::TEXT => {
                let rt = crate::rtf::parse(&b.rtf_bytes().unwrap_or_default());
                let md = rich_text_to_markdown(&rt, base);
                if !md.trim().is_empty() {
                    parts.push(md.trim_end().to_string());
                }
            }
            kind::EMBED => parts.push(format!("![[{code}]]")),
            kind::CODE => {
                parts.push(format!("```{}\n{}\n```", b.language.clone().unwrap_or_default(), code));
                if let Some(o) = b.output.as_ref().filter(|o| !o.is_empty()) {
                    parts.push(format!("```\n{}\n```", o.trim_end()));
                }
            }
            kind::DATABASE => parts.push(format!("```sql\n{code}\n```")),
            kind::MATH => parts.push(format!("$$\n{code}\n$$")),
            kind::DIAGRAM => parts.push(format!("```mermaid\n{code}\n```")),
            kind::IMAGE => {
                if let Some((_, path)) = link_for(&b.attachment_id, &mut files) {
                    let alt = b.alt_text.clone().or(b.caption.clone()).unwrap_or_default();
                    parts.push(format!("![{}]({path})", alt.replace(']', "")));
                }
            }
            kind::DRAWING => {
                if let Some((_, path)) = link_for(&b.attachment_id, &mut files) {
                    parts.push(format!("[Zeichnung]({path})"));
                }
            }
            kind::PDF | kind::FILE | kind::AUDIO => {
                if let Some((name, path)) = link_for(&b.attachment_id, &mut files) {
                    parts.push(format!("[{name}]({path})"));
                }
                if b.kind == kind::AUDIO {
                    if let Some(t) = b.output.as_ref().filter(|t| !t.is_empty()) {
                        parts.push(format!("> {}", t.replace('\n', "\n> ")));
                    }
                }
            }
            kind::WEB => {
                if let Some(u) = &b.url {
                    let title = b.caption.clone().filter(|c| !c.is_empty()).unwrap_or_else(|| u.clone());
                    parts.push(format!("[{title}]({u})"));
                }
            }
            _ => {}
        }
    }
    (parts.join("\n\n") + "\n", files)
}

static INLINE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(\*\*|__)(?P<b>.+?)(\*\*|__)|~~(?P<s>.+?)~~|`(?P<c>[^`]+)`|\[(?P<lt>[^\]]+)\]\((?P<lu>[^)\s]+)\)|(?:\*|_)(?P<i>[^*_\s][^*_]*?)(?:\*|_)").unwrap()
});

fn inline_runs(text: &str, base: Run) -> Vec<Run> {
    let mut out = vec![];
    let mut last = 0;
    for c in INLINE.captures_iter(text) {
        let m = c.get(0).unwrap();
        if m.start() > last {
            out.push(Run { text: unescape(&text[last..m.start()]), ..base.clone() });
        }
        if let Some(b) = c.name("b") {
            out.extend(inline_runs(b.as_str(), Run { b: true, ..base.clone() }));
        } else if let Some(s) = c.name("s") {
            out.extend(inline_runs(s.as_str(), Run { s: true, ..base.clone() }));
        } else if let Some(code) = c.name("c") {
            out.push(Run { text: code.as_str().to_string(), font: Some("mono".into()), ..base.clone() });
        } else if let (Some(lt), Some(lu)) = (c.name("lt"), c.name("lu")) {
            let link = if is_safe_link(lu.as_str()) { Some(lu.as_str().replace("%20", " ")) } else { None };
            let u = link.is_some();
            out.extend(inline_runs(lt.as_str(), Run { link, u, ..base.clone() }));
        } else if let Some(i) = c.name("i") {
            out.extend(inline_runs(i.as_str(), Run { i: true, ..base.clone() }));
        }
        last = m.end();
    }
    if last < text.len() {
        out.push(Run { text: unescape(&text[last..]), ..base });
    }
    out
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&n) = chars.peek() {
                if "*_`[]\\#~".contains(n) {
                    out.push(n);
                    chars.next();
                    continue;
                }
            }
        }
        out.push(c);
    }
    out
}

static LIST: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(?P<ind>[ \t]*)(?:(?P<task>[-*+] \[(?P<x>[ xX])\] )|(?P<bul>[-*+] )|(?P<num>\d{1,3})[.)] )").unwrap());

fn md_line_para(line: &str, base: f32) -> Para {
    let trimmed = line.trim_start();
    for (prefix, scale) in [("### ", 1.15f32), ("#### ", 1.15), ("## ", 1.4), ("# ", 1.85)] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let mut r = Run::default();
            r.b = true;
            r.size = Some((base * scale * 2.0).round() / 2.0);
            return Para { runs: inline_runs(rest.trim_end_matches(" #").trim(), r), ..Default::default() };
        }
    }
    if let Some(rest) = trimmed.strip_prefix("> ") {
        let r = Run { i: true, ..Default::default() };
        return Para { runs: inline_runs(rest, r), indent: Some(18.0), ..Default::default() };
    }
    if let Some(c) = LIST.captures(line) {
        let ind = c.name("ind").map(|m| m.as_str()).unwrap_or("");
        let tabs = ind.replace('\t', "  ").len() / 2;
        let marker = if c.name("task").is_some() {
            if c.name("x").map(|x| x.as_str() != " ").unwrap_or(false) { "☑ ".to_string() } else { "☐ ".to_string() }
        } else if c.name("bul").is_some() {
            "• ".to_string()
        } else {
            format!("{}. ", c.name("num").unwrap().as_str())
        };
        let rest = &line[c.get(0).unwrap().end()..];
        let mut runs = vec![Run { text: format!("{}{}", "\t".repeat(tabs), marker), ..Default::default() }];
        runs.extend(inline_runs(rest, Run::default()));
        return Para { runs, ..Default::default() };
    }
    if trimmed == "---" || trimmed == "***" {
        return Para::plain("———");
    }
    Para { runs: inline_runs(line, Run::default()), ..Default::default() }
}

/// Blocks for a Markdown text. `image` loads a local image path (relative to the file) into an attachment.
pub fn import(text: &str, base: f32, opts: &WriteOptions, mut image: impl FnMut(&str) -> Option<crate::archive::Attachment>) -> NoteFile {
    let mut blocks: Vec<BlockRecord> = vec![];
    let mut attachments = vec![];
    let mut paras: Vec<Para> = vec![];
    let flush_text = |paras: &mut Vec<Para>, blocks: &mut Vec<BlockRecord>| {
        while paras.last().map(|p| p.text().trim().is_empty()).unwrap_or(false) {
            paras.pop();
        }
        if paras.is_empty() {
            return;
        }
        let mut b = BlockRecord::new(kind::TEXT);
        let mut rt = RichText { paras: std::mem::take(paras) };
        rt.normalize();
        b.set_rtf(&crate::rtf::write(&rt, opts));
        blocks.push(b);
    };
    let text = text.replace("\r\n", "\n");
    // Front matter is not part of the note's text.
    let body = if let Some(rest) = text.strip_prefix("---\n") { rest.split_once("\n---\n").map(|(_, b)| b.to_string()).unwrap_or(text.clone()) } else { text.clone() };
    let lines: Vec<&str> = body.split('\n').collect();
    let mut i = 0;
    let mut blank = 0;
    static IMG: Lazy<Regex> = Lazy::new(|| Regex::new(r"^!\[(?P<alt>[^\]]*)\]\((?P<src>[^)]+)\)\s*$").unwrap());
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim();
        if let Some(fence) = t.strip_prefix("```") {
            let lang = fence.trim().to_lowercase();
            let mut code = vec![];
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with("```") {
                code.push(lines[i]);
                i += 1;
            }
            i += 1;
            flush_text(&mut paras, &mut blocks);
            let src = code.join("\n");
            let mut b = match lang.as_str() {
                "mermaid" => BlockRecord::new(kind::DIAGRAM),
                "math" | "latex" | "tex" => BlockRecord::new(kind::MATH),
                _ => {
                    let mut b = BlockRecord::new(kind::CODE);
                    b.language = Some(if lang.is_empty() { "plain".into() } else { crate::languages::alias(&lang).to_string() });
                    b
                }
            };
            b.code = Some(src);
            blocks.push(b);
            blank = 0;
            continue;
        }
        if t == "$$" || (t.starts_with("$$") && t.ends_with("$$") && t.len() > 4) {
            flush_text(&mut paras, &mut blocks);
            let mut b = BlockRecord::new(kind::MATH);
            if t.len() > 4 {
                b.code = Some(t[2..t.len() - 2].trim().to_string());
                i += 1;
            } else {
                let mut code = vec![];
                i += 1;
                while i < lines.len() && lines[i].trim() != "$$" {
                    code.push(lines[i]);
                    i += 1;
                }
                i += 1;
                b.code = Some(code.join("\n"));
            }
            blocks.push(b);
            continue;
        }
        if let Some(c) = IMG.captures(t) {
            let src = c.name("src").unwrap().as_str().replace("%20", " ");
            if let Some(att) = image(&src) {
                flush_text(&mut paras, &mut blocks);
                let mut b = BlockRecord::new(if att.header.type_identifier == "com.adobe.pdf" { kind::PDF } else { kind::IMAGE });
                b.attachment_id = Some(att.header.id.clone());
                let alt = c.name("alt").unwrap().as_str();
                if !alt.is_empty() {
                    b.alt_text = Some(alt.to_string());
                }
                b.width = Some(1.0);
                attachments.push(att);
                blocks.push(b);
                i += 1;
                continue;
            }
        }
        if t.starts_with("![[") && t.ends_with("]]") && !t[3..t.len() - 2].contains("]]") {
            flush_text(&mut paras, &mut blocks);
            let mut b = BlockRecord::new(kind::EMBED);
            b.code = Some(t[3..t.len() - 2].split('|').next().unwrap_or("").trim().to_string());
            blocks.push(b);
            i += 1;
            continue;
        }
        if t.is_empty() {
            blank += 1;
            // Markdown separates paragraphs with a blank line; keep one empty line for two or more.
            if blank == 2 {
                paras.push(Para::default());
            }
            i += 1;
            continue;
        }
        blank = 0;
        paras.push(md_line_para(line, base));
        i += 1;
    }
    flush_text(&mut paras, &mut blocks);
    let mut meta = crate::model::NoteMetadata::default();
    let mut f = NoteFile::new(meta.clone(), blocks);
    f.attachments = attachments;
    let full: String = f.body.blocks.iter().filter(|b| b.kind == kind::TEXT).map(|b| b.text()).collect::<Vec<_>>().join("\n");
    meta.title = crate::text::title_and_preview(&full).0.unwrap_or_default();
    f.meta = meta;
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_markdown() {
        let md = "# Titel\n\nHallo **fett** und *kursiv* mit [Link](https://x.de) und `code`\n\n- eins\n  - zwei\n- [ ] offen\n- [x] fertig\n1. erstens\n\n```python\nprint(1)\n```\n\n$$\nE=mc^2\n$$\n\n```mermaid\ngraph TD\n```\n";
        let f = import(md, 15.0, &WriteOptions::default(), |_| None);
        assert_eq!(f.meta.title, "Titel");
        let kinds: Vec<&str> = f.body.blocks.iter().map(|b| b.kind.as_str()).collect();
        assert_eq!(kinds, vec!["text", "code", "math", "diagram"]);
        let text = f.body.blocks[0].text();
        assert!(text.contains("• eins\n\t• zwei\n☐ offen\n☑ fertig\n1. erstens"));
        let (out, files) = export(&f, "Titel Dateien", 15.0);
        assert!(files.is_empty());
        assert!(out.starts_with("# Titel\nHallo **fett** und *kursiv* mit [Link](https://x.de) und `code`"));
        assert!(out.contains("- [ ] offen"));
        assert!(out.contains("```python\nprint(1)\n```"));
    }
}
