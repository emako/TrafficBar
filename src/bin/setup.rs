//! `cargo setup` → repo-root `setup.ps1`.
use std::env;
use std::path::Path;
use std::process::{self, Command};

fn main() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("setup.ps1");

    let status = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .args(env::args().skip(1))
        .status();

    match status {
        Ok(s) => process::exit(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("failed to run {}: {e}", script.display());
            process::exit(1);
        }
    }
}
