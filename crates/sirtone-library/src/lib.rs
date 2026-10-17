//! sirtone-library — SQLite library: migrations, repositories, import, layout, watcher, duplicates, export.
//! The only owner of SQLite connections (docs/03 §3.1).

pub mod collection;
pub mod covers;
pub mod db;
pub mod error;
pub mod import;
pub mod layout;
pub mod reconcile;
pub mod relocate;
pub mod repo;
pub mod watch;
pub mod wave;

pub use db::{Medium, SCHEMA_VERSION, open, open_existing, open_in_memory};
pub use error::LibraryError;
