use folio_core::{model::*, rtf::*};
fn main() {
    let out = std::env::args().nth(1).expect("path");
    let rt = RichText { paras: vec![
        Para { runs: vec![Run { text: "Rust-Notiz Grüße 😀".into(), b: true, size: Some(27.5), ..Default::default() }], ..Default::default() },
        Para { runs: vec![Run { text: "fett ".into(), b: true, ..Default::default() }, Run { text: "kursiv".into(), i: true, ..Default::default() }, Run { text: " link".into(), link: Some("folio://note/ABC".into()), ..Default::default() }], ..Default::default() },
        Para::plain("☐ Aufgabe #rust"),
    ]};
    let rtf = write(&rt, &WriteOptions::default());
    std::fs::write(format!("{out}.rtf"), &rtf).unwrap();
    let mut b = BlockRecord::new(kind::TEXT); b.set_rtf(&rtf);
    let mut c = BlockRecord::new(kind::CODE); c.code = Some("print(1)".into()); c.language = Some("python".into());
    let mut f = NoteFile::new(NoteMetadata::titled("x"), vec![b, c]);
    f.attachments.push(folio_core::archive::Attachment::new("a.txt", "public.plain-text", b"hallo".to_vec()));
    f.body.blocks[1].attachment_id = None;
    folio_core::note::refresh_meta(&mut f, true);
    f.write(std::path::Path::new(&out)).unwrap();
}
