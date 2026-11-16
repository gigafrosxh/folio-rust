//! `FOLIO_LIBRARY=<copy> folio-dev [port]`: serves the UI and commands on 127.0.0.1 for headless tests.
fn main() {
    if std::env::var("FOLIO_LIBRARY").is_err() {
        eprintln!("folio-dev braucht FOLIO_LIBRARY (eine Testbibliothek), damit nie die echte Bibliothek benutzt wird.");
        std::process::exit(78);
    }
    let port: u16 = std::env::args().nth(1).and_then(|p| p.parse().ok()).unwrap_or(0);
    let ui = std::env::var("FOLIO_UI").map(std::path::PathBuf::from).unwrap_or_else(|_| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui"));
    let app = std::sync::Arc::new(folio_app::App::new());
    let port = folio_app::server::serve(app, ui, port).expect("bind");
    println!("http://127.0.0.1:{port}/");
    loop {
        std::thread::park();
    }
}
