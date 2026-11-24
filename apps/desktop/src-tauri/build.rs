fn main() {
    println!("cargo:rerun-if-env-changed=SIRTONE_BUILD_COMMIT");
    println!("cargo:rerun-if-env-changed=SIRTONE_RELEASE_CHANNEL");
    println!("cargo:rerun-if-changed=../../../.git/HEAD");
    let channel =
        std::env::var("SIRTONE_RELEASE_CHANNEL").unwrap_or_else(|_| "development".to_owned());
    let commit = std::env::var("SIRTONE_BUILD_COMMIT").unwrap_or_else(|_| {
        std::process::Command::new("git")
            .args(["rev-parse", "--short=12", "HEAD"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|hash| hash.trim().to_owned())
            .filter(|hash| !hash.is_empty())
            .unwrap_or_else(|| "unknown".to_owned())
    });
    let build_id = format!("{}-{channel}-{commit}", env!("CARGO_PKG_VERSION"));
    println!("cargo:rustc-env=SIRTONE_BUILD_ID={build_id}");
    println!("cargo:rustc-env=SIRTONE_RELEASE_CHANNEL={channel}");
    tauri_build::build();
}
