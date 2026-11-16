//! `cargo run --release -p folio-core --example bench -- <empty dir> [notes]`: library scan and search timings.
use folio_core::{library::Scanner, model::*, rtf, search::SearchFilter};
use std::time::Instant;
fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("dir"));
    let n: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(5000);
    let t = Instant::now();
    for i in 0..n {
        let folder = dir.join(format!("Ordner {}", i % 20));
        std::fs::create_dir_all(&folder).unwrap();
        let text = format!("Notiz {i}\nEin Absatz über Rust und Swift mit #tag{} und [[Notiz {}]].\n{}", i % 50, (i * 7) % n, "Lorem ipsum dolor sit amet. ".repeat(40));
        let mut b = BlockRecord::new(kind::TEXT);
        b.set_rtf(&rtf::from_plain(&text, &rtf::WriteOptions::default()));
        let mut f = NoteFile::new(NoteMetadata::default(), vec![b]);
        folio_core::note::refresh_meta(&mut f, true);
        f.write(&folder.join(format!("Notiz {i}.folio"))).unwrap();
    }
    println!("{n} Notizen geschrieben: {:?}", t.elapsed());
    let s = Scanner::default();
    let t = Instant::now();
    let r = s.scan(&dir);
    println!("Scan kalt: {:?} ({} Notizen)", t.elapsed(), r.notes.len());
    let t = Instant::now();
    let r = s.scan(&dir);
    println!("Scan warm: {:?} ({} aus dem Cache)", t.elapsed(), r.cache_hits);
    let hay: Vec<String> = r.notes.iter().map(|n| SearchFilter::haystack(&n.meta)).collect();
    let f = SearchFilter::parse("rust absatz #tag7");
    let t = Instant::now();
    let hits = r.notes.iter().zip(&hay).filter(|(n, h)| f.matches(&n.meta, "", None, now(), Some(h))).count();
    println!("Suche über alle: {:?} ({hits} Treffer)", t.elapsed());
    let mut ix = folio_core::links::LinkIndex::default();
    let t = Instant::now();
    for n in &r.notes { ix.update(&n.meta, ""); }
    let e = ix.edges().len();
    println!("Link-Index: {:?} ({e} Kanten)", t.elapsed());
}
