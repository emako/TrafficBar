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
/// The installer records `KitsInstallationFolder` under
/// `HKLM\SOFTWARE\Microsoft\Microsoft SDKs\NETFXSDK\<version>` (32-bit and 64-bit views).
/// `%ProgramFiles(x86)%\Windows Kits\NETFXSDK` covers a default layout when that key is absent.
fn netfx_um_include() -> PathBuf {
    println!("cargo:rerun-if-env-changed=ProgramFiles(x86)");
    println!("cargo:rerun-if-env-changed=ProgramFiles");

    let mut found = netfx_registry_includes();
    for var in ["ProgramFiles(x86)", "ProgramFiles"] {
        let Ok(dir) = std::env::var(var) else {
            continue;
        };
        let Ok(entries) = std::fs::read_dir(PathBuf::from(dir).join(r"Windows Kits\NETFXSDK"))
        else {
            continue;
        };
        for entry in entries.flatten() {
            push_um_include(entry.path(), &mut found);
        }
    }
    found.sort_by(|left, right| version_key(left).cmp(&version_key(right)));
    found.pop().unwrap_or_else(|| {
        panic!(
            "metahost.h was not found. Install the .NET Framework developer pack, or check KitsInstallationFolder under HKLM\\SOFTWARE\\Microsoft\\Microsoft SDKs\\NETFXSDK"
        )
    })
}

fn push_um_include(version_dir: PathBuf, found: &mut Vec<PathBuf>) {
    let include = version_dir.join("Include").join("um");
    if include.join("metahost.h").is_file() && include.join("mscoree.h").is_file() {
        found.push(include);
    }
}

fn netfx_registry_includes() -> Vec<PathBuf> {
    const NETFX: &str = r"SOFTWARE\Microsoft\Microsoft SDKs\NETFXSDK";
    let mut found = Vec::new();
    for access in [KEY_READ | KEY_WOW64_32KEY, KEY_READ | KEY_WOW64_64KEY] {
        let Some(root) = open_key(HKEY_LOCAL_MACHINE, NETFX, access) else {
            continue;
        };
        for name in enum_subkeys(root) {
            let Some(version) = open_key(root, &name, KEY_READ) else {
                continue;
            };
            if let Some(folder) = query_sz(version, "KitsInstallationFolder") {
                push_um_include(PathBuf::from(folder), &mut found);
            }
            close_key(version);
        }
        close_key(root);
    }
    found
}

const HKEY_LOCAL_MACHINE: isize = 0x8000_0002u32 as isize;
const KEY_READ: u32 = 0x20019;
const KEY_WOW64_32KEY: u32 = 0x0200;
const KEY_WOW64_64KEY: u32 = 0x0100;
const ERROR_SUCCESS: i32 = 0;
const ERROR_MORE_DATA: i32 = 234;
const REG_SZ: u32 = 1;
const REG_EXPAND_SZ: u32 = 2;

#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegOpenKeyExW(
        key: isize,
        sub_key: *const u16,
        options: u32,
        sam: u32,
        result: *mut isize,
    ) -> i32;
    fn RegEnumKeyExW(
        key: isize,
        index: u32,
        name: *mut u16,
        name_len: *mut u32,
        reserved: *mut u32,
        class: *mut u16,
        class_len: *mut u32,
        last_write: *mut u8,
    ) -> i32;
    fn RegQueryValueExW(
        key: isize,
        value: *const u16,
        reserved: *mut u32,
        kind: *mut u32,
        data: *mut u8,
        data_len: *mut u32,
    ) -> i32;
    fn RegCloseKey(key: isize) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn ExpandEnvironmentStringsW(src: *const u16, dst: *mut u16, size: u32) -> u32;
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

fn open_key(parent: isize, sub_key: &str, access: u32) -> Option<isize> {
    let sub_key = wide(sub_key);
    let mut key = 0isize;
    let rc = unsafe { RegOpenKeyExW(parent, sub_key.as_ptr(), 0, access, &mut key) };
    if rc == ERROR_SUCCESS { Some(key) } else { None }
}

fn close_key(key: isize) {
    unsafe {
        RegCloseKey(key);
    }
}

fn enum_subkeys(key: isize) -> Vec<String> {
    let mut names = Vec::new();
    let mut index = 0u32;
    loop {
        let mut name = [0u16; 256];
        let mut len = name.len() as u32;
        let rc = unsafe {
            RegEnumKeyExW(
                key,
                index,
                name.as_mut_ptr(),
                &mut len,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if rc != ERROR_SUCCESS {
            break;
        }
        names.push(String::from_utf16_lossy(&name[..len as usize]));
        index += 1;
    }
    names
}

fn query_sz(key: isize, name: &str) -> Option<String> {
    let name = wide(name);
    let mut buf = vec![0u16; 1024];
    loop {
        let mut size = (buf.len() * std::mem::size_of::<u16>()) as u32;
        let mut kind = 0u32;
        let rc = unsafe {
            RegQueryValueExW(
                key,
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut kind,
                buf.as_mut_ptr() as *mut u8,
                &mut size,
            )
        };
        if rc == ERROR_MORE_DATA {
            let chars = (size as usize / 2).saturating_add(1);
            if chars <= buf.len() {
                return None;
            }
            buf.resize(chars, 0);
            continue;
        }
        if rc != ERROR_SUCCESS || (kind != REG_SZ && kind != REG_EXPAND_SZ) {
            return None;
        }
        let value = from_wide(&buf);
        if value.is_empty() {
            return None;
        }
        if kind == REG_EXPAND_SZ {
            return Some(expand_env(&value));
        }
        return Some(value);
    }
}

fn expand_env(value: &str) -> String {
    if !value.contains('%') {
        return value.to_string();
    }
    let src = wide(value);
    let mut buf = vec![0u16; 1024];
    let needed =
        unsafe { ExpandEnvironmentStringsW(src.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    if needed == 0 {
        return value.to_string();
    }
    if needed as usize > buf.len() {
        buf.resize(needed as usize, 0);
        let written =
            unsafe { ExpandEnvironmentStringsW(src.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
        if written == 0 || written as usize > buf.len() {
            return value.to_string();
        }
    }
    from_wide(&buf)
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
