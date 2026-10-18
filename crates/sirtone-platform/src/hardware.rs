//! What the computer has, for the performance auto-detect (T2.4, FR-PERF-003, `docs/06` §6.1):
//! logical cores, RAM, the strongest GPU (DXGI: name, dedicated memory → integrated/discrete).
//! The other half of the estimate — a short WebGL benchmark — runs in the UI.

use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

/// Dedicated video memory from which a GPU counts as discrete (integrated ones report
/// 128–512 MB of carve-out).
const DISCRETE_VRAM: u64 = 1 << 30;

#[derive(Debug, Clone, PartialEq)]
pub struct Gpu {
    pub name: String,
    pub vram_mb: u64,
    pub discrete: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hardware {
    pub cores: u32,
    pub ram_mb: u64,
    pub gpu: Option<Gpu>,
}

fn ram_mb() -> u64 {
    let mut m = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `m` is a valid, sized MEMORYSTATUSEX.
    match unsafe { GlobalMemoryStatusEx(&mut m) } {
        Ok(()) => m.ullTotalPhys / (1024 * 1024),
        Err(_) => 0,
    }
}

/// The hardware adapter with the most dedicated memory (software adapters skipped).
fn gpu() -> Option<Gpu> {
    // SAFETY: plain COM factory creation; the interface is released on drop.
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.ok()?;
    let mut best: Option<Gpu> = None;
    for i in 0.. {
        // SAFETY: enumeration stops at the first error (DXGI_ERROR_NOT_FOUND).
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(i) }) else {
            break;
        };
        // SAFETY: valid adapter.
        let Ok(d) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };
        if d.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
            continue;
        }
        let len = d.Description.iter().position(|&c| c == 0).unwrap_or(128);
        let vram = d.DedicatedVideoMemory as u64;
        if best
            .as_ref()
            .is_none_or(|b| vram / (1024 * 1024) > b.vram_mb)
        {
            best = Some(Gpu {
                name: String::from_utf16_lossy(&d.Description[..len])
                    .trim()
                    .to_string(),
                vram_mb: vram / (1024 * 1024),
                discrete: vram >= DISCRETE_VRAM,
            });
        }
    }
    best
}

pub fn hardware() -> Hardware {
    Hardware {
        cores: std::thread::available_parallelism().map_or(1, |n| n.get() as u32),
        ram_mb: ram_mb(),
        gpu: gpu(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn this_machine_reports_something_sane() {
        let h = super::hardware();
        assert!(h.cores >= 1 && h.ram_mb > 512, "{h:?}");
    }
}
