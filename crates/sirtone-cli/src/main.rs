//! sirtone-cli — developer harness (docs/03 §3). Console program, so stdout is its interface.
#![allow(clippy::print_stdout)]

fn main() {
    println!("sirtone-cli {}", env!("CARGO_PKG_VERSION"));
}
