use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src/assets/app.rc");
    println!("cargo:rerun-if-changed=src/assets/icons");
    println!("cargo:rerun-if-changed=src/plugin_abi/bridge.cpp");
    println!("cargo:rerun-if-changed=src/plugin_abi/PluginInterface.h");
    println!("cargo:rerun-if-changed=src/hardware_bridge/HardwareBridge.cs");

    // gpui embeds the application manifest (PerMonitorV2, Common Controls 6), so app.rc must not.
    embed_resource::compile("src/assets/app.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("compile app.rc");

    cc::Build::new()
        .cpp(true)
        .file("src/plugin_abi/bridge.cpp")
        .include("src/plugin_abi")
        .flag_if_supported("/EHsc")
        .flag_if_supported("/utf-8")
        .define("UNICODE", None)
        .define("_UNICODE", None)
        .compile("tm_plugin_bridge");

    build_hardware_helper();
}

/// Builds `TrafficMonitorBar.Hardware.exe` next to the main executable with the C# compiler that
/// ships with .NET Framework, so no extra SDK is needed.
fn build_hardware_helper() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let Some(target_dir) = out_dir.ancestors().nth(3).map(Path::to_path_buf) else {
        return;
    };
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let csc = Path::new(&windir).join(r"Microsoft.NET\Framework64\v4.0.30319\csc.exe");
    if !csc.exists() {
        println!("cargo:warning=csc.exe not found, temperature monitoring helper was not built");
        return;
    }
    let exe = target_dir.join("TrafficMonitorBar.Hardware.exe");
    // csc parses forward slashes as option prefixes, so the source needs a backslash path.
    let source = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("src")
        .join("hardware_bridge")
        .join("HardwareBridge.cs");
    let status = Command::new(&csc)
        .args(["/nologo", "/target:exe", "/platform:anycpu", "/optimize+"])
        .arg(format!("/out:{}", exe.display()))
        .arg(source)
        .status();
    match status {
        Ok(s) if s.success() => {}
        _ => println!("cargo:warning=failed to build TrafficMonitorBar.Hardware.exe"),
    }
}
