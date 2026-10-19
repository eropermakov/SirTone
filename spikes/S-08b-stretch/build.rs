// Signalsmith Stretch (MIT) compiled from the vendored C++ with a hand-written FFI: no bindgen,
// so no libclang on the build machine.
fn main() {
    println!("cargo::rerun-if-changed=vendor");
    cc::Build::new()
        .file("vendor/wrapper.cpp")
        .include("vendor/signalsmith-stretch")
        .include("vendor")
        .cpp(true)
        .std("c++14")
        .compile("signalsmith-stretch");
}
