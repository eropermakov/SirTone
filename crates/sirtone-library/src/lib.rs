//! sirtone-library — SQLite library: migrations, repositories, import, layout, watcher, duplicates, export.
//! The only owner of SQLite connections (docs/03 §3.1).

pub mod collection;
pub mod db;
pub mod error;
pub mod layout;
pub mod repo;

pub use db::{Medium, SCHEMA_VERSION, open, open_in_memory};
pub use error::LibraryError;
