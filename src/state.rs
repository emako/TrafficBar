//! UI-thread application state shared by the Win32 windows and the GPUI views.
//! Access is single threaded; callers must not hold a borrow across calls that pump messages
//! (menus and plugin dialogs).

use std::cell::RefCell;
use std::sync::Arc;

use crate::config::{Runtime, Settings};
use crate::monitor::hardware::HardwareState;
use crate::monitor::net::Connection;
use crate::monitor::{MonitorHandle, Selection, Snapshot};
use crate::plugin::PluginHost;
use crate::taskbar::render::RenderMode;

pub struct AppState {
    pub settings: Settings,
    pub rt: Runtime,
    pub snapshot: Snapshot,
    pub plugins: Arc<PluginHost>,
    pub connections: Vec<Connection>,
    pub selected_connection: usize,
    pub selection: Selection,
    pub hardware_state: HardwareState,
    pub disk_names: Vec<String>,
    pub cpu_core_names: Vec<String>,
    pub main_hwnd: isize,
    pub taskbar_hwnd: isize,
    pub start_time: chrono::DateTime<chrono::Local>,
    pub exe_dir_writable: bool,
    pub render_mode: RenderMode,
    pub checking_update: bool,
    pub monitor: Option<MonitorHandle>,
    /// Bumped whenever data shown by GPUI windows changes.
    pub revision: u64,
}

impl AppState {
    pub fn plugin_ids(&self) -> Vec<String> {
        self.plugins.item_ids()
    }

    pub fn save_settings(&self) -> bool {
        self.settings.save(&self.plugin_ids())
    }

    pub fn connection_name(&self) -> String {
        self.connections
            .get(self.selected_connection)
            .map(|c| c.description.clone())
            .unwrap_or_default()
    }
}

thread_local! {
    static STATE: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

pub fn init(s: AppState) {
    STATE.with(|c| *c.borrow_mut() = Some(s));
}

pub fn with<R>(f: impl FnOnce(&AppState) -> R) -> R {
    STATE.with(|c| f(c.borrow().as_ref().expect("app state not initialized")))
}

pub fn try_with<R>(f: impl FnOnce(&AppState) -> R) -> Option<R> {
    STATE.with(|c| match c.try_borrow() {
        Ok(s) => s.as_ref().map(f),
        Err(_) => None,
    })
}

pub fn with_mut<R>(f: impl FnOnce(&mut AppState) -> R) -> R {
    STATE.with(|c| f(c.borrow_mut().as_mut().expect("app state not initialized")))
}

pub fn try_with_mut<R>(f: impl FnOnce(&mut AppState) -> R) -> Option<R> {
    STATE.with(|c| match c.try_borrow_mut() {
        Ok(mut s) => s.as_mut().map(f),
        Err(_) => None,
    })
}
