#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod display_item;
mod format;
mod http;
mod i18n;
mod monitor;
mod notify_icon;
mod plugin;
mod state;
mod taskbar;
mod tray;
mod ui;
mod update_source;
mod win;

use std::io::Write;
use std::sync::Mutex;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, LPARAM, WPARAM,
};
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcess, TerminateProcess};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW};
use windows::core::{PCWSTR, w};

struct FileLogger {
    file: Mutex<Option<std::fs::File>>,
}

impl log::Log for FileLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
            && metadata.target().starts_with(env!("CARGO_CRATE_NAME"))
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        if let Ok(mut f) = self.file.lock() {
            if let Some(f) = f.as_mut() {
                let _ = writeln!(
                    f,
                    "{} [{}] {}",
                    chrono::Local::now().format("%Y/%m/%d %H:%M:%S"),
                    record.level(),
                    record.args()
                );
            }
        }
    }

    fn flush(&self) {}
}

fn init_logging(dir: &std::path::Path) {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("error.log"))
        .ok();
    let logger = Box::leak(Box::new(FileLogger {
        file: Mutex::new(file),
    }));
    let _ = log::set_logger(logger);
    log::set_max_level(log::LevelFilter::Warn);
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("{info}\n{}", std::backtrace::Backtrace::force_capture());
        default_hook(info);
    }));
}

/// Returns the mutex handle when this is the only instance. When another instance is running it
/// is asked to show itself, unless we were restarted (e.g. elevated) and must wait for it to exit.
fn single_instance(restarted: bool) -> Option<HANDLE> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let handle = unsafe { CreateMutexW(None, true, w!("TrafficBar_SingleInstance")) }.ok()?;
        if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
            return Some(handle);
        }
        unsafe {
            let _ = CloseHandle(handle);
        }
        if !restarted || std::time::Instant::now() > deadline {
            unsafe {
                if let Ok(hwnd) = FindWindowW(app::MAIN_CLASS, PCWSTR::null()) {
                    let _ = PostMessageW(Some(hwnd), app::WM_APP_ACTIVATE, WPARAM(0), LPARAM(0));
                }
            }
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

fn main() {
    // Setup.exe repair renames `%LocalAppData%\TrafficBar` and deletes that copy after
    // `--veloapp-install`. Rescue plugins before `VelopackApp::run` handles the hook and exits.
    let (early_config, _, _) = config::resolve_config_dir();
    plugin::preserve_installed_plugins(&early_config);

    // Must run before the single-instance mutex: install/update hooks start this exe and exit.
    velopack::VelopackApp::build().run();

    let restarted = std::env::args().any(|a| a == "--restarted");
    let elevate_attempted = std::env::args().any(|a| a == "--elevate-attempted");
    let Some(_instance) = single_instance(restarted) else {
        return;
    };
    let (config_dir, _, _) = config::resolve_config_dir();
    // Hold the mutex across the UAC prompt. On allow, the elevated copy waits on `--restarted`
    // until this handle is closed. On deny, keep running unelevated.
    if !elevate_attempted
        && !win::is_elevated()
        && config::read_always_run_as_admin(&config_dir)
        && win::elevate_at_startup()
    {
        unsafe {
            let _ = CloseHandle(_instance);
        }
        return;
    }
    init_logging(&config_dir);
    i18n::init(&config::read_language(&config_dir));
    ui::run();
    // Settings and history are saved by now. Some GPU drivers (seen with Intel D3D11) spin for
    // minutes in DLL_PROCESS_DETACH while D3D devices are alive, keeping the instance mutex.
    unsafe {
        let _ = CloseHandle(_instance);
        let _ = TerminateProcess(GetCurrentProcess(), 0);
    }
}
