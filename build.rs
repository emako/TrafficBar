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

    build_hardware_library();
}

/// Builds `TrafficMonitorBar.Hardware.dll` next to the main executable. The main process hosts
/// the CLR and loads this library, the same role as the original OpenHardwareMonitorApi.dll.
fn build_hardware_library() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let Some(target_dir) = out_dir.ancestors().nth(3).map(Path::to_path_buf) else {
        return;
    };
    let stale = target_dir.join("TrafficMonitorBar.Hardware.exe");
    if let Err(err) = std::fs::remove_file(&stale) {
        if stale.exists() {
            println!("cargo:warning=could not remove stale {}: {err}", stale.display());
        }
    }
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let csc = Path::new(&windir).join(r"Microsoft.NET\Framework64\v4.0.30319\csc.exe");
    if !csc.exists() {
        println!("cargo:warning=csc.exe not found, in-process hardware library was not built");
        return;
    }
    let dll = target_dir.join("TrafficMonitorBar.Hardware.dll");
    // csc parses forward slashes as option prefixes, so the source needs a backslash path.
    let source = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("src")
        .join("hardware_bridge")
        .join("HardwareBridge.cs");
    let output = Command::new(&csc)
        .args(["/nologo", "/target:library", "/platform:anycpu", "/optimize+"])
        .arg(format!("/out:{}", dll.display()))
        .arg(source)
        .output();
    match output {
        Ok(output) if output.status.success() => {}
        Ok(output) => {
            let err = String::from_utf8_lossy(&output.stderr);
            let out = String::from_utf8_lossy(&output.stdout);
            println!("cargo:warning=failed to build TrafficMonitorBar.Hardware.dll: {err}{out}");
        }
        Err(err) => println!("cargo:warning=failed to start csc.exe: {err}"),
    }
}
