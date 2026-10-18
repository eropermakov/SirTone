//! sirtone-recognize — tags, name parser, fingerprints, AcoustID/MusicBrainz, identity decisions.

pub mod covers;
pub mod identify;
pub mod name;
pub mod tags;

pub use covers::LoftyCovers;
pub use identify::{TagIdentify, similarity, split_artists};
pub use tags::LoftyTags;
