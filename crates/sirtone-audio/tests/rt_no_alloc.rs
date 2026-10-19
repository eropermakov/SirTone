//! T1.3: the render thread's real-time loop never allocates (`assert_no_alloc` style): a counting
//! global allocator, armed on the render thread through `output::RT_ENTER`, while real audio
//! plays, seeks, pauses and changes volume on the default device.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use sirtone_audio::{EngineCmd, EngineEvent};

thread_local!(static IN_RT: Cell<bool> = const { Cell::new(false) });
static RT_ALLOCS: AtomicUsize = AtomicUsize::new(0);
/// The hook really ran on the render thread (otherwise the check would pass vacuously).
static ARMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if IN_RT.with(Cell::get) {
            RT_ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        if IN_RT.with(Cell::get) {
            RT_ALLOCS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new) }
    }
}

#[global_allocator]
static A: Counting = Counting;

#[test]
fn render_loop_never_allocates() {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/generated/formats/flac-16-44.flac");
    if !file.exists() {
        return;
    }
    sirtone_audio::output::RT_ENTER
        .set(|| {
            ARMED.store(true, Ordering::SeqCst);
            IN_RT.with(|c| c.set(true));
        })
        .unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let Ok(engine) = sirtone_audio::start(Box::new(move |ev| {
        let _ = tx.send(ev);
    })) else {
        return; // no audio device (CI)
    };
    engine.send(EngineCmd::SetVolume(0.0));
    engine.send(EngineCmd::Load {
        path: file,
        autoplay: true,
        gain_db: 0.0,
    });
    let steps = [
        EngineCmd::Seek(4000),
        EngineCmd::SetVolume(0.0),
        EngineCmd::Pause,
        EngineCmd::Play,
        EngineCmd::Seek(1000),
        EngineCmd::Seek(8000),
    ];
    for cmd in steps {
        std::thread::sleep(Duration::from_millis(400));
        engine.send(cmd);
    }
    std::thread::sleep(Duration::from_millis(600));
    let device_ok = rx
        .try_iter()
        .all(|ev| !matches!(ev, EngineEvent::Error { .. }));
    engine.send(EngineCmd::Shutdown);
    std::thread::sleep(Duration::from_millis(300));
    if device_ok {
        assert!(
            ARMED.load(Ordering::SeqCst),
            "render thread never reached its loop"
        );
        assert_eq!(
            RT_ALLOCS.load(Ordering::Relaxed),
            0,
            "the real-time loop allocated"
        );
    }
}
