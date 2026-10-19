use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src/assets/app.rc");
    println!("cargo:rerun-if-changed=src/assets/icons");
    println!("cargo:rerun-if-changed=src/plugin_abi/bridge.cpp");
    println!("cargo:rerun-if-changed=src/plugin_abi/PluginInterface.h");
    println!("cargo:rerun-if-changed=src/hardware_bridge/HardwareBridge.cs");
    println!("cargo:rerun-if-changed=src/hardware_bridge/HardwareBridge.csproj");
    println!("cargo:rerun-if-changed=src/hardware_bridge/FodyWeavers.xml");
    println!("cargo:rerun-if-changed=src/hardware_bridge/clr_host.cpp");
    println!("cargo:rustc-link-lib=ole32");
    println!("cargo:rustc-link-lib=oleaut32");

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

    let framework = framework_dir();
    let tlb = framework.join("mscorlib.tlb");
    if !tlb.is_file() {
        panic!("mscorlib.tlb was not found at {}", tlb.display());
    }
    cc::Build::new()
        .cpp(true)
        .file("src/hardware_bridge/clr_host.cpp")
        .include(&framework)
        .include(netfx_um_include())
        .flag_if_supported("/EHsc")
        .flag_if_supported("/utf-8")
        .define("UNICODE", None)
        .define("_UNICODE", None)
        .compile("hardware_clr");

    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    remove_tlb_artifacts(&manifest);
    remove_tlb_artifacts(&out_dir);
    remove_tlb_artifacts(&manifest.join("src").join("hardware_bridge"));

    build_hardware_library(&out_dir);
}

fn framework_dir() -> PathBuf {
    let windows = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
    PathBuf::from(windows).join(r"Microsoft.NET\Framework64\v4.0.30319")
}

/// `metahost.h` and `mscoree.h` ship in the .NET Framework SDK, not the regular Windows SDK.
fn netfx_um_include() -> PathBuf {
    let mut roots = Vec::new();
    if let Ok(program_files) = std::env::var("ProgramFiles(x86)") {
        roots.push(PathBuf::from(program_files).join(r"Windows Kits\NETFXSDK"));
    }
    roots.push(PathBuf::from(
        r"C:\Program Files (x86)\Windows Kits\NETFXSDK",
    ));
    let mut found = Vec::new();
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let include = entry.path().join("Include").join("um");
            if include.join("metahost.h").is_file() && include.join("mscoree.h").is_file() {
                found.push(include);
            }
        }
    }
    found.sort_by(|left, right| version_key(left).cmp(&version_key(right)));
    found.pop().unwrap_or_else(|| {
        panic!("metahost.h was not found under the NETFXSDK include directories")
    })
}

fn version_key(include_um: &Path) -> Vec<u32> {
    let Some(version) = include_um
        .ancestors()
        .nth(2)
        .and_then(|path| path.file_name())
    else {
        return Vec::new();
    };
    version
        .to_string_lossy()
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

fn remove_tlb_artifacts(dir: &Path) {
    for name in ["mscorlib.tlh", "mscorlib.tli"] {
        let path = dir.join(name);
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Builds `TrafficBar.Hardware.dll` and copies it into OUT_DIR for `include_bytes!`.
/// The bytes are loaded from memory; the DLL is not shipped next to the executable.
fn build_hardware_library(out_dir: &Path) {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    if let Some(target_dir) = out_dir.ancestors().nth(3) {
        for name in ["TrafficBar.Hardware.exe", "TrafficBar.Hardware.dll"] {
            let stale = target_dir.join(name);
            if stale.exists() {
                if let Err(err) = std::fs::remove_file(&stale) {
                    println!(
                        "cargo:warning=could not remove stale {}: {err}",
                        stale.display()
                    );
                }
            }
        }
    }
    let project = manifest
        .join("src")
        .join("hardware_bridge")
        .join("HardwareBridge.csproj");
    let configuration = if std::env::var("PROFILE").ok().as_deref() == Some("release") {
        "Release"
    } else {
        "Debug"
    };
    let output = Command::new("dotnet")
        .args([
            "build",
            "--nologo",
            "-v",
            "q",
            "-c",
            configuration,
            "-p:Platform=x64",
        ])
        .arg(&project)
        .output();
    let output = match output {
        Ok(output) => output,
        Err(err) => panic!("failed to start dotnet: {err}"),
    };
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let out = String::from_utf8_lossy(&output.stdout);
        panic!("failed to build TrafficBar.Hardware.dll: {err}{out}");
    }
    let built = manifest
        .join("src")
        .join("hardware_bridge")
        .join("bin")
        .join(configuration)
        .join("TrafficBar.Hardware.dll");
    if !built.is_file() {
        panic!(
            "TrafficBar.Hardware.dll was not produced at {}",
            built.display()
        );
    }
    let embedded = out_dir.join("TrafficBar.Hardware.dll");
    if let Err(err) = std::fs::copy(&built, &embedded) {
        panic!(
            "failed to copy {} to {}: {err}",
            built.display(),
            embedded.display()
        );
    }
}
