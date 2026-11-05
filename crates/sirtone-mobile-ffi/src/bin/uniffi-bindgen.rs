//! Generates the Swift and Kotlin bindings (`scripts/build-ios-core.sh`, Android CI).
fn main() {
    uniffi::uniffi_bindgen_main()
}
