//! `cargo setup` can only expand to another Cargo subcommand. This binary's
//! only job is to launch `setup.ps1` and forward arguments.

use std::path::Path;
use std::process::Command;

/// Drops the `--` that `cargo setup -- -Target …` leaves in front of the script args.
/// PowerShell treats a leading `--` as an ambiguous parameter name.
fn script_args(args: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut args: Vec<String> = args.into_iter().collect();
    if args.first().map(String::as_str) == Some("--") {
        args.remove(0);
    }
    args
}

fn main() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../setup.ps1");
    let args = script_args(std::env::args().skip(1));
    let status = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .args(&args)
        .status();
    let code = match status {
        Ok(status) => status.code().unwrap_or(1),
        Err(err) => {
            eprintln!("failed to start {}: {err}", script.display());
            1
        }
    };
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::script_args;

    #[test]
    fn drops_cargo_separator_only() {
        assert_eq!(
            script_args(["--".into(), "-Target".into(), "aarch64-pc-windows-msvc".into()]),
            vec!["-Target".to_string(), "aarch64-pc-windows-msvc".to_string()]
        );
        assert_eq!(
            script_args(["-Target".into(), "x86_64-pc-windows-msvc".into()]),
            vec!["-Target".to_string(), "x86_64-pc-windows-msvc".to_string()]
        );
        assert!(script_args(std::iter::empty::<String>()).is_empty());
    }
}
