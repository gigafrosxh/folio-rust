//! Derived metadata of a note: title, preview, tags, search text, links, block kinds. The same rules as
//! `NoteDocument.makeFile()` in the Swift app.

use crate::model::{self, kind, BlockRecord, NoteFile};
use std::collections::BTreeSet;

/// The text a block contributes to the search (#43, #46).
pub fn block_plain_text(b: &BlockRecord) -> String {
    let join = |parts: &[Option<&String>]| parts.iter().flatten().filter(|s| !s.is_empty()).map(|s| s.as_str()).collect::<Vec<_>>().join("\n");
    match b.kind.as_str() {
        kind::TEXT => b.text(),
        kind::CODE | kind::DATABASE | kind::MATH | kind::DIAGRAM => b.code.clone().unwrap_or_default(),
        kind::WEB => b.url.clone().unwrap_or_default(),
        kind::AUDIO => join(&[b.caption.as_ref(), b.output.as_ref()]),
        kind::EMBED => format!("![[{}]]", b.code.clone().unwrap_or_default()),
        kind::IMAGE => join(&[b.caption.as_ref(), b.alt_text.as_ref()]),
        kind::PDF => join(&[b.caption.as_ref(), b.pdf_text.as_ref()]),
        _ => b.caption.clone().unwrap_or_default(),
    }
}

/// Refreshes title, preview, tags, search text, wiki links, kinds and note links from the blocks.
/// `touch` sets the modification date to now.
pub fn refresh_meta(file: &mut NoteFile, touch: bool) {
    let mut texts: Vec<String> = vec![];
    let mut links: Vec<String> = vec![];
    let mut kinds = BTreeSet::new();
    let mut search: Vec<String> = vec![];
    for b in &file.body.blocks {
        if b.kind == kind::TEXT {
            let rt = crate::rtf::parse(&b.rtf_bytes().unwrap_or_default());
            let t = rt.plain_text();
            for p in &rt.paras {
                for r in &p.runs {
                    if let Some(id) = r.link.as_deref().and_then(crate::text::note_id_from_link) {
                        if id != file.meta.id && !links.contains(&id) {
                            links.push(id);
                        }
                    }
                }
            }
            if !t.is_empty() {
                kinds.insert(b.kind.clone());
            }
            search.push(t.clone());
            texts.push(t);
        } else {
            kinds.insert(b.kind.clone());
            search.push(block_plain_text(b));
        }
    }
    let full = texts.join("\n");
    let (title, preview) = crate::text::title_and_preview(&full);
    if let Some(t) = title {
        file.meta.title = t;
    }
    file.meta.preview = preview;
    file.meta.tags = crate::text::tags(&full);
    file.meta.search_text = search.join("\n");
    file.meta.wiki_links = Some(crate::text::wiki_targets(&file.meta.search_text));
    file.meta.kinds = Some(kinds.into_iter().collect());
    file.meta.links = links;
    if touch {
        file.meta.modified = model::now();
    }
}

/// Drops attachments no block refers to (like the Swift app on save).
pub fn prune_attachments(file: &mut NoteFile) {
    let used: std::collections::HashSet<&String> = file.body.blocks.iter().filter_map(|b| b.attachment_id.as_ref()).collect();
    let keep: Vec<_> = file.attachments.iter().filter(|a| used.contains(&a.header.id)).cloned().collect();
    file.attachments = keep;
}

/// A copy of a note with new ids, „Kopie“ appended to the title's first line and fresh dates.
pub fn duplicated(file: &NoteFile) -> NoteFile {
    let mut c = model::renewed(file);
    let now = model::now();
    c.meta.created = now;
    c.meta.modified = now;
    c.meta.pinned = false;
    c.meta.title = format!("{} Kopie", if file.meta.title.is_empty() { "Notiz" } else { &file.meta.title });
    c
}
