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

/// Builds `TrafficBar.Hardware.dll` next to the main executable. The main process hosts
/// the CLR and loads this library. LibreHardwareMonitor is embedded by the csproj build.
fn build_hardware_library() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let Some(target_dir) = out_dir.ancestors().nth(3).map(Path::to_path_buf) else {
        return;
    };
    let stale = target_dir.join("TrafficBar.Hardware.exe");
    if let Err(err) = std::fs::remove_file(&stale) {
        if stale.exists() {
            println!("cargo:warning=could not remove stale {}: {err}", stale.display());
        }
    }
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let project = manifest.join("src").join("hardware_bridge").join("HardwareBridge.csproj");
    let configuration = if std::env::var("PROFILE").ok().as_deref() == Some("release") {
        "Release"
    } else {
        "Debug"
    };
    let output = Command::new("dotnet")
        .args(["build", "--nologo", "-v", "q", "-c", configuration, "-p:Platform=x64"])
        .arg(&project)
        .output();
    let output = match output {
        Ok(output) => output,
        Err(err) => {
            println!("cargo:warning=failed to start dotnet: {err}");
            return;
        }
    };
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let out = String::from_utf8_lossy(&output.stdout);
        println!("cargo:warning=failed to build TrafficBar.Hardware.dll: {err}{out}");
        return;
    }
    let built = manifest
        .join("src")
        .join("hardware_bridge")
        .join("bin")
        .join(configuration)
        .join("TrafficBar.Hardware.dll");
    let dll = target_dir.join("TrafficBar.Hardware.dll");
    if let Err(err) = std::fs::copy(&built, &dll) {
        println!(
            "cargo:warning=failed to copy {} to {}: {err}",
            built.display(),
            dll.display()
        );
    }
}
