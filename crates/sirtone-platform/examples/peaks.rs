//! Prints the foreign sessions' peaks every 20 ms for a few seconds (T4.14 diagnostics):
//! `cargo run -p sirtone-platform --example peaks -- <seconds>`.
#![allow(clippy::print_stdout)] // a console tool: printing is its job

fn main() {
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(3);
    sirtone_platform::sessions::com_init();
    let own = sirtone_platform::sessions::own_tree();
    let t0 = std::time::Instant::now();
    while t0.elapsed().as_secs() < secs {
        let f = sirtone_platform::sessions::foreign(&own);
        let line: Vec<String> = f
            .iter()
            .map(|s| format!("{} {:.1}", s.exe, 20.0 * s.peak.max(1e-7).log10()))
            .collect();
        println!("{:5} {}", t0.elapsed().as_millis(), line.join(" | "));
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
