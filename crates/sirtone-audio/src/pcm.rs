//! Streaming PCM for analysis (T3.6): the playback decoders (symphonia, Media Foundation for
//! WMA) handed out packet by packet at the native rate.

use std::path::Path;

use sirtone_core::error::AppError;
use sirtone_core::ports::PcmDecoder;

use crate::decode::Source;

/// The analysis decoder (T3.6): native rate and channels, packet by packet.
pub struct PcmFiles;

impl PcmDecoder for PcmFiles {
    fn decode(
        &self,
        path: &Path,
        sink: &mut dyn FnMut(&[f32], usize, u32) -> bool,
    ) -> Result<(), AppError> {
        let mut src = Source::open(path)?;
        let mut buf = Vec::with_capacity(8192);
        loop {
            buf.clear();
            if !src.next_into(&mut buf)? {
                return Ok(());
            }
            if !buf.is_empty() && !sink(&buf, src.channels, src.rate) {
                return Ok(());
            }
        }
    }
}
