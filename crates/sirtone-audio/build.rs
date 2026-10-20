// Signalsmith Stretch (MIT; T4.9c, D-100): the vendored C++ compiled with `cc`, the FFI is
// hand-written in `stretch.rs` — no bindgen, so no libclang on the build machine.
fn main() {
    println!("cargo::rerun-if-changed=vendor/signalsmith");
    cc::Build::new()
        .file("vendor/signalsmith/wrapper.cpp")
        .include("vendor/signalsmith/signalsmith-stretch")
        .include("vendor/signalsmith")
        .cpp(true)
        .std("c++14")
        .warnings(false)
        .compile("signalsmith-stretch");
}
