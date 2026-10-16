//! sirtone-recognize — tags, name parser, fingerprints, AcoustID/MusicBrainz, identity decisions.

pub mod identify;
pub mod tags;

pub use identify::{TagIdentify, split_artists};
pub use tags::LoftyTags;
