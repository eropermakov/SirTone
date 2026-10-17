//! Media Foundation decoder for what symphonia cannot read (T1.2, docs/04 §2: WMA/ASF). The
//! source reader decodes to interleaved `f32` at the file's native rate; everything above
//! (resampler, gapless chain) treats it like any other source.

use std::cell::Cell;
use std::path::Path;
use std::sync::OnceLock;

use sirtone_core::error::{AppError, ErrorCode};
use windows::Win32::Media::MediaFoundation::{
    IMFSourceReader, MF_MT_AUDIO_CHANNEL_MASK, MF_MT_AUDIO_NUM_CHANNELS,
    MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_PD_DURATION,
    MF_SOURCE_READER_FIRST_AUDIO_STREAM, MF_SOURCE_READER_MEDIASOURCE,
    MF_SOURCE_READERF_ENDOFSTREAM, MF_VERSION, MFAudioFormat_Float, MFAudioFormat_WMAudio_Lossless,
    MFCreateMediaType, MFCreateSourceReaderFromURL, MFMediaType_Audio, MFSTARTUP_NOSOCKET,
    MFStartup,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::core::{GUID, HSTRING};

/// 100-ns units per second (MF time).
const HNS: i64 = 10_000_000;

thread_local!(static COM: Cell<bool> = const { Cell::new(false) });

fn ensure_com() {
    COM.with(|c| {
        if !c.get() {
            // S_FALSE / RPC_E_CHANGED_MODE are fine: COM is usable on this thread either way.
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            c.set(true);
        }
    });
}

fn startup() -> Result<(), AppError> {
    static STARTED: OnceLock<bool> = OnceLock::new();
    ensure_com();
    if *STARTED.get_or_init(|| unsafe { MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) }.is_ok()) {
        Ok(())
    } else {
        Err(AppError::new(ErrorCode::UnsupportedFormat)
            .with_details("Media Foundation unavailable"))
    }
}

fn failed(e: windows::core::Error) -> AppError {
    AppError::new(ErrorCode::DecodeFailed).with_details(e)
}

const STREAM: u32 = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;

pub struct MfReader {
    reader: IMFSourceReader,
    pub rate: u32,
    pub channels: usize,
    pub duration_ms: Option<u64>,
    /// WAVEFORMATEXTENSIBLE speaker mask (same bits as symphonia's positions); 0 = unknown.
    pub channel_mask: u64,
    /// `wmal` for WMA Lossless, `wma` otherwise.
    pub codec: &'static str,
    /// After a seek: drop decoded frames before this time (100 ns).
    skip_until: Option<i64>,
}

// The source reader is created in the MTA and is free-threaded; every thread that touches it
// joins the MTA first (`ensure_com`).
unsafe impl Send for MfReader {}

impl MfReader {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        startup()?;
        unsafe {
            let reader = MFCreateSourceReaderFromURL(&HSTRING::from(path.as_os_str()), None)
                .map_err(|e| AppError::new(ErrorCode::UnsupportedFormat).with_details(e))?;
            let lossless = reader
                .GetNativeMediaType(STREAM, 0)
                .and_then(|t| t.GetGUID(&MF_MT_SUBTYPE))
                .is_ok_and(|g| g == MFAudioFormat_WMAudio_Lossless);
            let want = MFCreateMediaType().map_err(failed)?;
            want.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
                .map_err(failed)?;
            want.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Float)
                .map_err(failed)?;
            reader
                .SetCurrentMediaType(STREAM, None, &want)
                .map_err(|e| AppError::new(ErrorCode::UnsupportedFormat).with_details(e))?;
            let got = reader.GetCurrentMediaType(STREAM).map_err(failed)?;
            let rate = got
                .GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND)
                .map_err(failed)?;
            let channels = got.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS).map_err(failed)? as usize;
            let channel_mask = got
                .GetUINT32(&MF_MT_AUDIO_CHANNEL_MASK)
                .map_or(0, u64::from);
            if rate == 0 || channels == 0 {
                return Err(AppError::new(ErrorCode::UnsupportedFormat));
            }
            let duration_ms = reader
                .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
                .ok()
                .and_then(|v| u64::try_from(&v).ok())
                .map(|hns| hns / 10_000);
            Ok(Self {
                reader,
                rate,
                channels,
                duration_ms,
                channel_mask,
                codec: if lossless { "wmal" } else { "wma" },
                skip_until: None,
            })
        }
    }

    /// Appends the next decoded block to `out`; `Ok(false)` at end of stream.
    pub fn next_into(&mut self, out: &mut Vec<f32>) -> Result<bool, AppError> {
        ensure_com();
        loop {
            let mut flags = 0u32;
            let mut ts = 0i64;
            let mut sample = None;
            unsafe {
                self.reader
                    .ReadSample(
                        STREAM,
                        0,
                        None,
                        Some(&mut flags),
                        Some(&mut ts),
                        Some(&mut sample),
                    )
                    .map_err(failed)?;
            }
            let Some(sample) = sample else {
                if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                    return Ok(false);
                }
                continue; // a gap or a format notice without data
            };
            let ch = self.channels;
            let start = out.len();
            unsafe {
                let buf = sample.ConvertToContiguousBuffer().map_err(failed)?;
                let mut ptr = std::ptr::null_mut();
                let mut len = 0u32;
                buf.Lock(&mut ptr, None, Some(&mut len)).map_err(failed)?;
                let n = len as usize / 4;
                if !ptr.is_null() && n > 0 {
                    // Lock gives a byte pointer with no alignment promise: copy byte-wise.
                    let bytes = std::slice::from_raw_parts(ptr, n * 4);
                    out.extend(
                        bytes
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .map(|b| f32::from_le_bytes(*b)),
                    );
                }
                let _ = buf.Unlock();
            }
            out.truncate(start + (out.len() - start) / ch * ch);
            if let Some(target) = self.skip_until {
                let frames = ((out.len() - start) / ch) as i64;
                let skip = ((target - ts) * i64::from(self.rate) / HNS).clamp(0, frames);
                if skip >= frames {
                    out.truncate(start);
                    continue;
                }
                out.drain(start..start + skip as usize * ch);
                self.skip_until = None;
            }
            if out.len() == start {
                continue;
            }
            return Ok(true);
        }
    }

    /// Seeks to `ms`; MF lands on a packet at or before it, the rest is dropped by timestamp.
    /// MF timestamps after a seek are approximate: within ~60 ms of the target (D-68).
    pub fn seek(&mut self, ms: u64) -> Result<(), AppError> {
        ensure_com();
        let target = ms as i64 * 10_000;
        let pos = PROPVARIANT::from(target);
        unsafe { self.reader.SetCurrentPosition(&GUID::zeroed(), &pos) }.map_err(failed)?;
        self.skip_until = Some(target);
        Ok(())
    }
}
