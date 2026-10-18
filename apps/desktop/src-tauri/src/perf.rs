//! Hardware facts for the performance auto-detect (T2.4, FR-PERF-003, `docs/06` §6.1); the UI
//! adds its WebGL benchmark and picks the level.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HardwareInfo {
    pub cores: u32,
    pub ram_mb: u32,
    pub gpu_name: Option<String>,
    pub vram_mb: u32,
    pub discrete_gpu: bool,
}

#[tauri::command(async)]
#[specta::specta]
pub fn perf_hardware() -> HardwareInfo {
    let h = sirtone_platform::hardware::hardware();
    HardwareInfo {
        cores: h.cores,
        ram_mb: u32::try_from(h.ram_mb).unwrap_or(u32::MAX),
        gpu_name: h.gpu.as_ref().map(|g| g.name.clone()),
        vram_mb: h
            .gpu
            .as_ref()
            .map_or(0, |g| u32::try_from(g.vram_mb).unwrap_or(u32::MAX)),
        discrete_gpu: h.gpu.as_ref().is_some_and(|g| g.discrete),
    }
}
