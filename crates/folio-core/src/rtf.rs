//! Rich text of text blocks: a small run model, read from and written to the RTF that Cocoa
//! (`NSAttributedString`) produces and understands.
//!
//! Folio's lists are plain-text prefixes ("• ", "☐ ", "1. "), so paragraphs, character runs and a few
//! paragraph attributes are all the editor needs. Attachments inside RTF are skipped.

use serde::{Deserialize, Serialize};

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub text: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub b: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub i: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub u: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub s: bool,
    /// Point size; None = body size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    /// "mono" for code fonts, else a font family; None = body font.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// "#rrggbb"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

impl Run {
    fn same_format(&self, o: &Run) -> bool {
        self.b == o.b && self.i == o.i && self.u == o.u && self.s == o.s && self.size == o.size && self.font == o.font
            && self.color == o.color && self.bg == o.bg && self.link == o.link
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Para {
    #[serde(default)]
    pub runs: Vec<Run>,
    /// "center", "right", "justify"; None = natural.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<String>,
    /// Left indent in points (quotes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent: Option<f32>,
}

impl Para {
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
    pub fn plain(text: &str) -> Para {
        Para { runs: if text.is_empty() { vec![] } else { vec![Run { text: text.into(), ..Default::default() }] }, ..Default::default() }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RichText {
    pub paras: Vec<Para>,
}

impl RichText {
    pub fn plain_text(&self) -> String {
        self.paras.iter().map(|p| p.text()).collect::<Vec<_>>().join("\n")
    }

    pub fn from_plain(text: &str) -> RichText {
        RichText { paras: text.split('\n').map(Para::plain).collect() }
    }

    pub fn is_empty(&self) -> bool {
        self.paras.iter().all(|p| p.runs.iter().all(|r| r.text.is_empty()))
    }

    /// Merges neighbouring runs with the same format and drops empty ones.
    pub fn normalize(&mut self) {
        for p in self.paras.iter_mut() {
            let mut out: Vec<Run> = Vec::with_capacity(p.runs.len());
            for r in p.runs.drain(..) {
                if r.text.is_empty() {
                    continue;
                }
                if let Some(last) = out.last_mut() {
                    if last.same_format(&r) {
                        last.text.push_str(&r.text);
                        continue;
                    }
                }
                out.push(r);
            }
            p.runs = out;
        }
        if self.paras.is_empty() {
            self.paras.push(Para::default());
        }
    }
}

// MARK: - Parsing

const CP1252_HIGH: [u16; 32] = [
    0x20AC, 0xFFFD, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0xFFFD, 0x017D, 0xFFFD,
    0xFFFD, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0xFFFD, 0x017E, 0x0178,
];

fn cp1252(b: u8) -> char {
    if (0x80..0xA0).contains(&b) {
        char::from_u32(CP1252_HIGH[(b - 0x80) as usize] as u32).unwrap_or('\u{FFFD}')
    } else {
        b as char
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Dest {
    Normal,
    Skip,
    FontTable,
    ColorTable,
    FieldInst,
}

#[derive(Clone)]
struct State {
    dest: Dest,
    font: i32,
    size_half: i32,
    b: bool,
    i: bool,
    u: bool,
    s: bool,
    cf: i32,
    cb: i32,
    uc: usize,
    link: Option<String>,
    in_fldrslt: bool,
}

struct Font {
    name: String,
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
    stack: Vec<State>,
    st: State,
    fonts: std::collections::HashMap<i32, Font>,
    font_num: i32,
    font_name: String,
    colors: Vec<Option<String>>,
    cr: u8,
    cg: u8,
    cbl: u8,
    color_set: bool,
    paras: Vec<Para>,
    cur: Para,
    para_align: Option<String>,
    para_indent: Option<f32>,
    fldinst: String,
    pending_link: Option<String>,
    skip_chars: usize,
    utf16_high: Option<u16>,
    ignorable_next: bool,
}

fn is_mono_font(name: &str) -> bool {
    let n = name.to_lowercase();
    ["mono", "menlo", "monaco", "courier", "consolas", "fixed"].iter().any(|k| n.contains(k))
}

fn is_system_font(name: &str) -> bool {
    name.starts_with('.') || name.starts_with("Helvetica") || name.starts_with("SF") || name.is_empty() || name.starts_with("Times")
}

fn family_of(name: &str) -> String {
    name.split('-').next().unwrap_or(name).to_string()
}

impl<'a> Parser<'a> {
    fn new(src: &'a [u8]) -> Self {
        let st = State { dest: Dest::Normal, font: 0, size_half: 24, b: false, i: false, u: false, s: false, cf: 0, cb: 0, uc: 1, link: None, in_fldrslt: false };
        Parser {
            src,
            pos: 0,
            stack: vec![],
            st,
            fonts: Default::default(),
            font_num: 0,
            font_name: String::new(),
            colors: vec![],
            cr: 0,
            cg: 0,
            cbl: 0,
            color_set: false,
            paras: vec![],
            cur: Para::default(),
            para_align: None,
            para_indent: None,
            fldinst: String::new(),
            pending_link: None,
            skip_chars: 0,
            utf16_high: None,
            ignorable_next: false,
        }
    }

    fn emit(&mut self, c: char) {
        if self.skip_chars > 0 {
            self.skip_chars -= 1;
            return;
        }
        match self.st.dest {
            Dest::Normal => {}
            Dest::FontTable => {
                if c == ';' {
                    self.fonts.insert(self.font_num, Font { name: self.font_name.trim().to_string() });
                    self.font_name.clear();
                } else {
                    self.font_name.push(c);
                }
                return;
            }
            Dest::FieldInst => {
                self.fldinst.push(c);
                return;
            }
            _ => return,
        }
        if c == '\u{FFFC}' {
            return; // attachment character: attachments are not kept in text blocks
        }
        let font_name = self.fonts.get(&self.st.font).map(|f| f.name.clone()).unwrap_or_default();
        let lower = font_name.to_lowercase();
        let bold = self.st.b || lower.contains("bold") || lower.contains("heavy") || lower.contains("black") || lower.contains("semibold");
        let italic = self.st.i || lower.contains("italic") || lower.contains("oblique");
        let font = if is_mono_font(&font_name) {
            Some("mono".to_string())
        } else if is_system_font(&font_name) {
            None
        } else {
            Some(family_of(&font_name))
        };
        let color = if self.st.cf > 0 { self.colors.get(self.st.cf as usize).cloned().flatten() } else { None };
        let bg = if self.st.cb > 0 { self.colors.get(self.st.cb as usize).cloned().flatten() } else { None };
        let run = Run {
            text: c.to_string(),
            b: bold,
            i: italic,
            u: self.st.u,
            s: self.st.s,
            size: Some(self.st.size_half as f32 / 2.0),
            font,
            color,
            bg,
            link: self.st.link.clone(),
        };
        if let Some(last) = self.cur.runs.last_mut() {
            if last.same_format(&run) {
                last.text.push(c);
                return;
            }
        }
        self.cur.runs.push(run);
    }

    fn end_para(&mut self) {
        let mut p = std::mem::take(&mut self.cur);
        p.align = self.para_align.clone();
        p.indent = self.para_indent;
        self.paras.push(p);
    }

    fn unicode(&mut self, n: i32) {
        let u = if n < 0 { (n + 65536) as u16 } else { n as u16 };
        if (0xD800..0xDC00).contains(&u) {
            self.utf16_high = Some(u);
        } else if (0xDC00..0xE000).contains(&u) {
            if let Some(h) = self.utf16_high.take() {
                let c = 0x10000 + (((h as u32) - 0xD800) << 10) + ((u as u32) - 0xDC00);
                if let Some(ch) = char::from_u32(c) {
                    self.emit_raw(ch);
                }
            }
        } else if let Some(ch) = char::from_u32(u as u32) {
            self.emit_raw(ch);
        }
        self.skip_chars = self.st.uc;
    }

    fn emit_raw(&mut self, c: char) {
        let saved = self.skip_chars;
        self.skip_chars = 0;
        self.emit(c);
        self.skip_chars = saved;
    }

    fn control(&mut self, word: &str, param: Option<i32>) {
        let p = param.unwrap_or(1);
        if self.ignorable_next {
            self.ignorable_next = false;
            if word == "fldinst" {
                self.st.dest = Dest::FieldInst;
                self.fldinst.clear();
            } else {
                self.st.dest = Dest::Skip;
            }
            return;
        }
        if self.st.dest == Dest::Skip {
            return;
        }
        if self.st.dest == Dest::FontTable {
            if word == "f" {
                self.font_num = p;
                self.font_name.clear();
            }
            return;
        }
        if self.st.dest == Dest::ColorTable {
            match word {
                "red" => { self.cr = p as u8; self.color_set = true; }
                "green" => { self.cg = p as u8; self.color_set = true; }
                "blue" => { self.cbl = p as u8; self.color_set = true; }
                _ => {}
            }
            return;
        }
        match word {
            "fonttbl" => self.st.dest = Dest::FontTable,
            "colortbl" => self.st.dest = Dest::ColorTable,
            "stylesheet" | "info" | "pict" | "listtable" | "listoverridetable" | "NeXTGraphic" | "header" | "footer" | "object"
            | "themedata" | "colorschememapping" | "latentstyles" | "datastore" | "xmlnstbl" | "rsidtbl" | "generator" => self.st.dest = Dest::Skip,
            "fldrslt" => {
                self.st.in_fldrslt = true;
                self.st.link = self.pending_link.clone();
            }
            "field" => self.pending_link = None,
            "par" => self.end_para(),
            "line" => self.end_para(),
            "sect" | "page" => self.end_para(),
            "tab" => self.emit('\t'),
            "pard" => {
                self.para_align = None;
                self.para_indent = None;
            }
            "qc" => self.para_align = Some("center".into()),
            "qr" => self.para_align = Some("right".into()),
            "qj" => self.para_align = Some("justify".into()),
            "ql" => self.para_align = None,
            "li" => self.para_indent = if p > 0 { Some(p as f32 / 20.0) } else { None },
            "plain" => {
                self.st.b = false;
                self.st.i = false;
                self.st.u = false;
                self.st.s = false;
                self.st.cf = 0;
                self.st.cb = 0;
                self.st.size_half = 24;
            }
            "b" => self.st.b = p != 0,
            "i" => self.st.i = p != 0,
            "ul" => self.st.u = p != 0,
            "ulnone" => self.st.u = false,
            "uld" | "uldb" | "ulth" | "ulw" | "ulwave" => self.st.u = true,
            "strike" | "striked" => self.st.s = p != 0,
            "f" => self.st.font = p,
            "fs" => self.st.size_half = p.max(2),
            "cf" => self.st.cf = p,
            "cb" | "highlight" => self.st.cb = p,
            "uc" => self.st.uc = p.max(0) as usize,
            "u" => self.unicode(p),
            "emdash" => self.emit('—'),
            "endash" => self.emit('–'),
            "bullet" => self.emit('•'),
            "lquote" => self.emit('‘'),
            "rquote" => self.emit('’'),
            "ldblquote" => self.emit('“'),
            "rdblquote" => self.emit('”'),
            _ => {}
        }
    }

    fn parse(mut self) -> RichText {
        let src = self.src;
        while self.pos < src.len() {
            let c = src[self.pos];
            match c {
                b'{' => {
                    self.pos += 1;
                    self.stack.push(self.st.clone());
                    if self.st.dest == Dest::ColorTable {
                        // nested groups in the color table are unusual; ignore
                    }
                }
                b'}' => {
                    self.pos += 1;
                    if self.st.dest == Dest::FieldInst {
                        if let Some(url) = parse_hyperlink(&self.fldinst) {
                            self.pending_link = Some(url);
                        }
                    }
                    if let Some(prev) = self.stack.pop() {
                        self.st = prev;
                    }
                    self.skip_chars = 0;
                }
                b'\\' => {
                    self.pos += 1;
                    if self.pos >= src.len() {
                        break;
                    }
                    let n = src[self.pos];
                    if n.is_ascii_alphabetic() {
                        let start = self.pos;
                        while self.pos < src.len() && src[self.pos].is_ascii_alphabetic() {
                            self.pos += 1;
                        }
                        let word = std::str::from_utf8(&src[start..self.pos]).unwrap_or("").to_string();
                        let mut param = None;
                        let ns = self.pos;
                        if self.pos < src.len() && (src[self.pos] == b'-' || src[self.pos].is_ascii_digit()) {
                            self.pos += 1;
                            while self.pos < src.len() && src[self.pos].is_ascii_digit() {
                                self.pos += 1;
                            }
                            param = std::str::from_utf8(&src[ns..self.pos]).ok().and_then(|s| s.parse::<i64>().ok()).map(|v| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32);
                        }
                        if self.pos < src.len() && src[self.pos] == b' ' {
                            self.pos += 1;
                        }
                        self.control(&word, param);
                    } else {
                        self.pos += 1;
                        match n {
                            b'\'' => {
                                if self.pos + 2 <= src.len() {
                                    let hex = std::str::from_utf8(&src[self.pos..self.pos + 2]).unwrap_or("3f");
                                    self.pos += 2;
                                    if let Ok(v) = u8::from_str_radix(hex, 16) {
                                        if self.st.dest == Dest::Normal || self.st.dest == Dest::FontTable || self.st.dest == Dest::FieldInst {
                                            self.emit(cp1252(v));
                                        }
                                    }
                                }
                            }
                            b'*' => self.ignorable_next = true,
                            b'\n' | b'\r' => {
                                if self.st.dest == Dest::Normal {
                                    self.end_para();
                                }
                            }
                            b'~' => self.emit('\u{a0}'),
                            b'_' => self.emit('‑'),
                            b'-' => {}
                            b'\\' | b'{' | b'}' => self.emit(n as char),
                            _ => {}
                        }
                    }
                }
                b'\r' | b'\n' => self.pos += 1,
                _ => {
                    // Raw text: UTF-8 is not valid RTF but tolerated; bytes ≥ 0x80 are read as UTF-8 when they form a character.
                    if c >= 0x80 {
                        let end = (self.pos + 4).min(src.len());
                        let mut taken = false;
                        for l in (2..=end - self.pos).rev() {
                            if let Ok(s) = std::str::from_utf8(&src[self.pos..self.pos + l]) {
                                if let Some(ch) = s.chars().next() {
                                    if ch.len_utf8() == l {
                                        self.pos += l;
                                        self.text_char(ch);
                                        taken = true;
                                        break;
                                    }
                                }
                            }
                        }
                        if !taken {
                            self.pos += 1;
                            self.text_char(cp1252(c));
                        }
                    } else {
                        self.pos += 1;
                        self.text_char(c as char);
                    }
                }
            }
        }
        // The text after the last \par is the last paragraph, even when empty (text ending in a newline).
        self.end_para();
        let mut rt = RichText { paras: self.paras };
        rt.normalize();
        rt
    }

    fn text_char(&mut self, c: char) {
        if self.st.dest == Dest::ColorTable {
            if c == ';' {
                self.colors.push(if self.color_set { Some(format!("#{:02x}{:02x}{:02x}", self.cr, self.cg, self.cbl)) } else { None });
                self.color_set = false;
                self.cr = 0;
                self.cg = 0;
                self.cbl = 0;
            }
            return;
        }
        self.emit(c);
    }
}

fn parse_hyperlink(inst: &str) -> Option<String> {
    let t = inst.trim();
    let rest = t.strip_prefix("HYPERLINK")?.trim();
    let rest = rest.trim_start_matches("\\l").trim();
    if let Some(r) = rest.strip_prefix('"') {
        Some(r.split('"').next()?.to_string())
    } else {
        rest.split_whitespace().next().map(|s| s.to_string())
    }
}

/// Parses RTF into paragraphs and runs. Sizes are kept as found; `normalize_sizes` relates them to the body size.
pub fn parse(rtf: &[u8]) -> RichText {
    if rtf.is_empty() {
        return RichText { paras: vec![Para::default()] };
    }
    if !rtf.starts_with(b"{\\rtf") {
        return RichText::from_plain(&String::from_utf8_lossy(rtf));
    }
    Parser::new(rtf).parse()
}

/// Parses and drops sizes equal to `body_size` (± 0.5 pt), so text follows the body size setting.
pub fn parse_for_editor(rtf: &[u8], body_size: f32) -> RichText {
    let mut rt = parse(rtf);
    for p in rt.paras.iter_mut() {
        for r in p.runs.iter_mut() {
            if let Some(s) = r.size {
                if (s - body_size).abs() < 0.6 || (s - 12.0).abs() < 0.01 && r.font.is_none() && !r.b {
                    r.size = None;
                }
            }
        }
    }
    rt.normalize();
    rt
}

pub fn plain_text(rtf: &[u8]) -> String {
    parse(rtf).plain_text()
}

// MARK: - Writing

pub struct WriteOptions {
    pub body_size: f32,
    /// PostScript name of the body font (".AppleSystemUIFont" for the system font).
    pub body_font: String,
    pub mono_font: String,
}

impl Default for WriteOptions {
    fn default() -> Self {
        WriteOptions { body_size: 15.0, body_font: ".AppleSystemUIFont".into(), mono_font: "Menlo-Regular".into() }
    }
}

fn escape_into(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\t' => out.push_str("\\tab "),
            '\n' | '\r' => out.push_str("\\\n"),
            c if (c as u32) < 0x80 => out.push(c),
            c => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    let v = *unit as i32;
                    let v = if v > 32767 { v - 65536 } else { v };
                    out.push_str(&format!("\\u{v} "));
                }
            }
        }
    }
}

fn parse_hex(c: &str) -> Option<(u8, u8, u8)> {
    let h = c.trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    Some((u8::from_str_radix(&h[0..2], 16).ok()?, u8::from_str_radix(&h[2..4], 16).ok()?, u8::from_str_radix(&h[4..6], 16).ok()?))
}

/// Writes Cocoa-compatible RTF. Runs without color are written without one, so they stay adaptive in dark mode.
pub fn write(rt: &RichText, opts: &WriteOptions) -> Vec<u8> {
    let mut fonts: Vec<String> = vec![opts.body_font.clone(), opts.mono_font.clone()];
    let mut colors: Vec<String> = vec![];
    for p in &rt.paras {
        for r in &p.runs {
            if let Some(f) = &r.font {
                if f != "mono" && !fonts.contains(f) {
                    fonts.push(f.clone());
                }
            }
            for c in [&r.color, &r.bg].into_iter().flatten() {
                if parse_hex(c).is_some() && !colors.contains(c) {
                    colors.push(c.clone());
                }
            }
        }
    }
    let mut out = String::with_capacity(256 + rt.paras.len() * 32);
    out.push_str("{\\rtf1\\ansi\\ansicpg1252\\uc0\\cocoartf2822\n\\cocoatextscaling0\\cocoaplatform0{\\fonttbl");
    for (i, f) in fonts.iter().enumerate() {
        let family = if i == 1 { "\\fmodern" } else { "\\fnil" };
        out.push_str(&format!("\\f{i}{family}\\fcharset0 "));
        escape_into(&mut out, f);
        out.push(';');
    }
    out.push_str("}\n{\\colortbl;\\red255\\green255\\blue255;");
    for c in &colors {
        let (r, g, b) = parse_hex(c).unwrap();
        out.push_str(&format!("\\red{r}\\green{g}\\blue{b};"));
    }
    out.push_str("\n}\n");
    let color_index = |c: &Option<String>| -> usize { c.as_ref().and_then(|c| colors.iter().position(|x| x == c)).map(|i| i + 2).unwrap_or(0) };
    let body_half = (opts.body_size * 2.0).round() as i32;

    let mut last_para: Option<(Option<String>, Option<f32>)> = None;
    // Current character state, to write only changes.
    let mut f_cur = -1i32;
    let mut fs_cur = -1i32;
    let (mut b_cur, mut i_cur, mut u_cur, mut s_cur) = (false, false, false, false);
    let (mut cf_cur, mut cb_cur) = (usize::MAX, usize::MAX);
    for (pi, p) in rt.paras.iter().enumerate() {
        let props = (p.align.clone(), p.indent);
        if last_para.as_ref() != Some(&props) {
            out.push_str("\\pard\\tx560\\tx1120\\tx1680\\tx2240\\tx2800\\tx3360\\tx3920\\tx4480\\tx5040\\tx5600\\tx6160\\tx6720");
            if let Some(ind) = p.indent {
                let tw = (ind * 20.0).round() as i32;
                out.push_str(&format!("\\li{tw}\\fi0"));
            }
            out.push_str("\\pardirnatural\\partightenfactor0");
            match p.align.as_deref() {
                Some("center") => out.push_str("\\qc"),
                Some("right") => out.push_str("\\qr"),
                Some("justify") => out.push_str("\\qj"),
                _ => {}
            }
            out.push_str("\n");
            last_para = Some(props);
        }
        let empty_run = Run::default();
        let runs: Vec<&Run> = if p.runs.is_empty() { vec![&empty_run] } else { p.runs.iter().collect() };
        for r in runs {
            let f = match r.font.as_deref() {
                None => 0,
                Some("mono") => 1,
                Some(name) => fonts.iter().position(|x| x == name).unwrap_or(0) as i32,
            };
            let fs = r.size.map(|s| (s * 2.0).round() as i32).unwrap_or(body_half).max(2);
            let mut ctl = String::new();
            if f != f_cur {
                ctl.push_str(&format!("\\f{f}"));
                f_cur = f;
            }
            if r.b != b_cur {
                ctl.push_str(if r.b { "\\b" } else { "\\b0" });
                b_cur = r.b;
            }
            if r.i != i_cur {
                ctl.push_str(if r.i { "\\i" } else { "\\i0" });
                i_cur = r.i;
            }
            if fs != fs_cur {
                ctl.push_str(&format!("\\fs{fs}"));
                fs_cur = fs;
            }
            let cf = color_index(&r.color);
            if cf != cf_cur {
                ctl.push_str(&format!("\\cf{cf}"));
                cf_cur = cf;
            }
            let cb = color_index(&r.bg);
            if cb != cb_cur {
                ctl.push_str(&format!("\\cb{cb}"));
                cb_cur = cb;
            }
            if r.u != u_cur {
                ctl.push_str(if r.u { "\\ul\\ulc0" } else { "\\ulnone" });
                u_cur = r.u;
            }
            if r.s != s_cur {
                ctl.push_str(if r.s { "\\strike\\strikec0" } else { "\\strike0\\striked0" });
                s_cur = r.s;
            }
            if !ctl.is_empty() {
                out.push_str(&ctl);
                out.push(' ');
            }
            if let Some(link) = &r.link {
                out.push_str("{\\field{\\*\\fldinst{HYPERLINK \"");
                escape_into(&mut out, &link.replace('"', "%22"));
                out.push_str("\"}}{\\fldrslt ");
                escape_into(&mut out, &r.text);
                out.push_str("}}");
            } else {
                escape_into(&mut out, &r.text);
            }
        }
        if pi + 1 < rt.paras.len() {
            out.push_str("\\\n");
        }
    }
    out.push('}');
    out.into_bytes()
}

/// Plain text as RTF in the body style.
pub fn from_plain(text: &str, opts: &WriteOptions) -> Vec<u8> {
    write(&RichText::from_plain(text), opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cocoa_rtf() {
        let src = br#"{\rtf1\ansi\ansicpg1252\cocoartf2908
\cocoatextscaling0\cocoaplatform0{\fonttbl\f0\fnil\fcharset0 HelveticaNeue;\f1\fnil\fcharset0 HelveticaNeue-Bold;\f3\fnil\fcharset0 Menlo-Regular;}
{\colortbl;\red255\green255\blue255;\red0\green0\blue0;\red221\green149\blue13;
}
{\*\expandedcolortbl;;\cssrgb\c0\c0\c0;\cssrgb\c89804\c64706\c3922;
}
\pard\pardeftab720\sa300\partightenfactor0

\f0\fs30 \cf0 Hallo
\f1\b fett
\f0\b0  \ul unter\ulnone  {\field{\*\fldinst{HYPERLINK "folio://note/ABC"}}{\fldrslt Link}} \uc0\u8226 \'fcber\
\pard\qc
\f3\fs26 \cf3 code\
}"#;
        let rt = parse(src);
        assert_eq!(rt.plain_text(), "Hallofett unter Link •über\ncode\n");
        let runs = &rt.paras[0].runs;
        assert!(runs.iter().any(|r| r.text == "fett" && r.b));
        assert!(runs.iter().any(|r| r.text == "unter" && r.u));
        assert!(runs.iter().any(|r| r.text == "Link" && r.link.as_deref() == Some("folio://note/ABC")));
        assert_eq!(rt.paras[1].align.as_deref(), Some("center"));
        assert_eq!(rt.paras[1].runs[0].font.as_deref(), Some("mono"));
        assert_eq!(rt.paras[1].runs[0].color.as_deref(), Some("#dd950d"));
    }

    #[test]
    fn roundtrip() {
        let rt = RichText {
            paras: vec![
                Para { runs: vec![Run { text: "Titel 😀".into(), b: true, size: Some(27.75), ..Default::default() }], ..Default::default() },
                Para {
                    runs: vec![
                        Run { text: "a {b} \\ c ".into(), ..Default::default() },
                        Run { text: "link".into(), link: Some("https://x.de/?a=1".into()), u: true, color: Some("#112233".into()), ..Default::default() },
                    ],
                    align: Some("center".into()),
                    indent: Some(18.0),
                },
                Para::plain("☐ Aufgabe\tTab"),
                Para::default(),
            ],
        };
        let bytes = write(&rt, &WriteOptions::default());
        let back = parse_for_editor(&bytes, 15.0);
        assert_eq!(back.plain_text(), rt.plain_text());
        assert_eq!(back.paras[0].runs[0].size, Some(28.0)); // half points
        assert!(back.paras[0].runs[0].b);
        assert_eq!(back.paras[1].runs[1].link.as_deref(), Some("https://x.de/?a=1"));
        assert_eq!(back.paras[1].align.as_deref(), Some("center"));
        assert_eq!(back.paras[1].indent, Some(18.0));
        assert_eq!(back.paras[1].runs[1].color.as_deref(), Some("#112233"));
        assert!(back.paras[1].runs[0].size.is_none());
    }

    #[test]
    fn empty_and_plain() {
        assert_eq!(plain_text(b""), "");
        assert_eq!(plain_text(br"{\rtf1\ansi{\fonttbl\f0 Helvetica;}\f0\fs30 Testnotiz\par Inhalt}"), "Testnotiz\nInhalt");
    }
}
