//! sirtone-library — SQLite library: migrations, repositories, import, layout, watcher, duplicates, export.
//! The only owner of SQLite connections (docs/03 §3.1).

pub mod analysis;
pub mod art;
pub mod artists;
pub mod collection;
pub mod continuation;
pub mod covers;
pub mod db;
pub mod duplicates;
pub mod error;
pub mod feedback;
pub mod import;
pub mod layout;
pub mod online;
pub mod playlists;
pub mod popularity;
pub mod reconcile;
pub mod relocate;
pub mod repo;
pub mod search;
pub mod search_text;
pub mod semantic;
pub mod smart;
pub mod stats;
pub mod unknowns;
pub mod watch;
pub mod wave;

pub use db::{Medium, SCHEMA_VERSION, open, open_existing, open_in_memory};
pub use error::LibraryError;
