//! Folio's core: everything below the user interface.

pub mod ai;
pub mod archive;
pub mod capture;
pub mod html;
pub mod languages;
pub mod library;
pub mod links;
pub mod markdown;
pub mod model;
pub mod note;
pub mod rtf;
pub mod runner;
pub mod search;
pub mod text;

/// A new id in the form Swift's `UUID().uuidString` produces (uppercase).
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string().to_uppercase()
}
