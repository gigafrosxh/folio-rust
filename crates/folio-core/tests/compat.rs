//! Notes written by the Swift app (fixtures from several versions) open, and survive a round trip.
use folio_core::model::NoteFile;

#[test]
fn swift_fixtures_open_and_roundtrip() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut n = 0;
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().map(|x| x != "folio").unwrap_or(true) { continue; }
        let f = NoteFile::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        let meta = NoteFile::read_metadata(&p).unwrap();
        assert_eq!(meta.id, f.meta.id);
        for b in &f.body.blocks {
            if b.kind == "text" { let _ = b.text(); }
        }
        let enc = f.encode().unwrap();
        let back = NoteFile::decode(&enc).unwrap();
        assert_eq!(back.body, f.body, "{}", p.display());
        assert_eq!(back.meta, f.meta);
        println!("{}: {} – {} Blöcke, {} Anhänge, Text: {:?}", p.file_name().unwrap().to_string_lossy(), f.meta.title, f.body.blocks.len(), f.attachments.len(),
            f.body.blocks.iter().filter(|b| b.kind == "text").map(|b| b.text()).collect::<Vec<_>>().join(" | ").chars().take(120).collect::<String>());
        n += 1;
    }
    assert!(n >= 5);
}
