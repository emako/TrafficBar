//! Native popup menu shared by the tray icon and the taskbar window (`IDR_TASK_BAR_MENU`
//! without the main-window, skin and notify-icon entries). Dark mode follows the approach of
//! NativeTray: `SetPreferredAppMode` + `FlushMenuThemes` from uxtheme before every popup.
//! Item bitmaps follow the original `CMenuIcon` assignments. Each icon is an ICO with the same
//! 16/20/24/28/32 frames, and the frame is chosen with `DPI(16)`.

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCSTR, PCWSTR, w};

use crate::app::{self, UiCommand};
use crate::config::DoubleClickAction;
use crate::i18n::{tr, trm};
use crate::monitor::{MonitorCmd, Selection};
use crate::state;
use crate::win::{self, wide};

const ID_AUTO_SELECT: u32 = 100;
const ID_SELECT_ALL: u32 = 101;
const ID_REFRESH_CONNECTIONS: u32 = 102;
const ID_NETWORK_INFO: u32 = 110;
const ID_TRAFFIC_HISTORY: u32 = 111;
const ID_DISPLAY_SETTINGS: u32 = 112;
const ID_TASK_MANAGER: u32 = 113;
const ID_PLUGIN_MANAGE: u32 = 114;
const ID_OPTIONS: u32 = 115;
const ID_CHANGE_NOTIFY_ICON: u32 = 125;
const ID_HELP: u32 = 120;
const ID_FAQ: u32 = 121;
const ID_UPDATE_LOG: u32 = 122;
const ID_ABOUT: u32 = 123;
const ID_CHECK_UPDATE: u32 = 124;
const ID_RESTART_AS_ADMIN: u32 = 130;
const ID_RESTART: u32 = 132;
const ID_EXIT: u32 = 131;
const ID_PLUGIN_OPTIONS: u32 = 140;
const ID_PLUGIN_DETAIL: u32 = 141;
const ID_CONNECTION_BASE: u32 = 1000;
const ID_CONNECTION_MAX: u32 = 1999;
const ID_PLUGIN_COMMAND_BASE: u32 = 2000;
const ID_PLUGIN_COMMAND_MAX: u32 = 2099;

type SetPreferredAppMode = unsafe extern "system" fn(i32) -> i32;
type FlushMenuThemes = unsafe extern "system" fn();

/// `PreferredAppMode`: 2 = ForceDark, 3 = ForceLight (uxtheme ordinal 135, Windows 10 1903+).
fn apply_menu_theme() {
    if win::windows_version().build < 18362 {
        return;
    }
    let dark = !win::apps_use_light_theme();
    unsafe {
        let Ok(ux) = LoadLibraryW(w!("uxtheme.dll")) else {
            return;
        };
        let set = GetProcAddress(ux, PCSTR(135usize as *const u8));
        let flush = GetProcAddress(ux, PCSTR(136usize as *const u8));
        if let Some(set) = set {
            let set: SetPreferredAppMode = std::mem::transmute(set);
            set(if dark { 2 } else { 3 });
        }
        if let Some(flush) = flush {
            let flush: FlushMenuThemes = std::mem::transmute(flush);
            flush();
        }
    }
}

fn append(
    menu: HMENU,
    id: u32,
    text: &str,
    checked: bool,
    disabled: bool,
    default: bool,
    radio: bool,
    icon: HBITMAP,
) {
    let t = wide(text);
    let mut state = MENU_ITEM_STATE(0);
    if checked {
        state |= MFS_CHECKED;
    }
    if disabled {
        state |= MFS_DISABLED;
    }
    if default {
        state |= MFS_DEFAULT;
    }
    let mut ftype = MFT_STRING;
    if radio {
        ftype |= MFT_RADIOCHECK;
    }
    let mut mask = MIIM_ID | MIIM_STRING | MIIM_STATE | MIIM_FTYPE;
    if !icon.is_invalid() {
        mask |= MIIM_BITMAP;
    }
    let info = MENUITEMINFOW {
        cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
        fMask: mask,
        fType: ftype,
        fState: state,
        wID: id,
        dwTypeData: windows::core::PWSTR(t.as_ptr() as *mut u16),
        hbmpItem: icon,
        ..Default::default()
    };
    unsafe {
        let n = GetMenuItemCount(Some(menu));
        let _ = InsertMenuItemW(menu, n.max(0) as u32, true, &info);
    }
}

fn item(menu: HMENU, id: u32, text: &str, icon: HBITMAP) {
    append(menu, id, text, false, false, false, false, icon);
}

fn separator(menu: HMENU) {
    unsafe {
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    }
}

fn submenu(menu: HMENU, sub: HMENU, text: &str, icon: HBITMAP) {
    let t = wide(text);
    unsafe {
        let _ = AppendMenuW(
            menu,
            MF_POPUP | MF_STRING,
            sub.0 as usize,
            PCWSTR(t.as_ptr()),
        );
        if icon.is_invalid() {
            return;
        }
        let n = GetMenuItemCount(Some(menu));
        if n <= 0 {
            return;
        }
        let info = MENUITEMINFOW {
            cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_BITMAP,
            hbmpItem: icon,
            ..Default::default()
        };
        let _ = SetMenuItemInfoW(menu, (n - 1) as u32, true, &info);
    }
}

struct MenuModel {
    connections: Vec<String>,
    selection: Selection,
    selected: usize,
    show_all_interface: bool,
    double_click: DoubleClickAction,
    checking_update: bool,
    elevated: bool,
    plugin: Option<(usize, String, Vec<(String, bool)>)>,
}

fn build_model(plugin: Option<usize>) -> Option<MenuModel> {
    state::try_with(|s| MenuModel {
        connections: s
            .connections
            .iter()
            .map(|c| c.description.clone())
            .collect(),
        selection: s.selection.clone(),
        selected: s.selected_connection,
        show_all_interface: s.settings.general.show_all_interface,
        double_click: s.settings.taskbar.double_click_action,
        checking_update: s.checking_update,
        elevated: s.rt.elevated,
        plugin: plugin.and_then(|i| {
            let p = s.plugins.plugins.get(i)?;
            Some((i, p.name(), s.plugins.commands(i)))
        }),
    })
}

/// Shows the menu at the cursor and executes the chosen command.
/// `plugin` is the index of the plugin whose item was right-clicked on the taskbar.
pub fn show_context_menu(owner: HWND, plugin: Option<usize>) {
    let Some(m) = build_model(plugin) else { return };
    apply_menu_theme();
    let dark = !win::apps_use_light_theme();
    let dpi = unsafe { GetDpiForWindow(owner) };
    let icons = crate::tray::icons::load(dark, crate::tray::icons::pixel_size(dpi));
    let none = HBITMAP::default();
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let conn = CreatePopupMenu().unwrap_or_default();
        let all = m.selection == Selection::All;
        let auto = m.selection == Selection::Auto;
        append(
            conn,
            ID_AUTO_SELECT,
            &trm("TXT_AUTO_SELECT"),
            auto,
            false,
            false,
            true,
            none,
        );
        append(
            conn,
            ID_SELECT_ALL,
            &trm("TXT_SELECT_ALL"),
            all,
            m.show_all_interface,
            false,
            true,
            none,
        );
        for (i, name) in m
            .connections
            .iter()
            .enumerate()
            .take((ID_CONNECTION_MAX - ID_CONNECTION_BASE) as usize)
        {
            let checked = !auto && !all && i == m.selected;
            append(
                conn,
                ID_CONNECTION_BASE + i as u32,
                name,
                checked,
                false,
                !all && i == m.selected,
                true,
                none,
            );
        }
        separator(conn);
        item(
            conn,
            ID_REFRESH_CONNECTIONS,
            &tr("IDS_REFRESH_CONNECTION_LIST"),
            none,
        );
        submenu(menu, conn, &trm("TXT_SELECT_CONNECTIONS"), icons.connection);
        append(
            menu,
            ID_NETWORK_INFO,
            &trm("TXT_CONNECTION_DETAILS"),
            false,
            false,
            m.double_click == DoubleClickAction::ConnectionInfo,
            false,
            icons.info,
        );
        separator(menu);
        append(
            menu,
            ID_TRAFFIC_HISTORY,
            &trm("TXT_HISTORY_TRAFFIC_STATISTICS"),
            false,
            false,
            m.double_click == DoubleClickAction::HistoryTraffic,
            false,
            icons.statistics,
        );
        separator(menu);
        item(
            menu,
            ID_DISPLAY_SETTINGS,
            &trm("TXT_DISPLAY_SETTINGS"),
            icons.display,
        );
        separator(menu);
        append(
            menu,
            ID_TASK_MANAGER,
            &trm("TXT_TASK_MANAGER"),
            false,
            false,
            m.double_click == DoubleClickAction::TaskManager,
            false,
            icons.task,
        );
        item(
            menu,
            ID_PLUGIN_MANAGE,
            &trm("TXT_PLUGIN_MANAGE"),
            icons.plugins,
        );
        item(
            menu,
            ID_CHANGE_NOTIFY_ICON,
            &trm("TXT_CHANGE_NOTIFY_ICON"),
            none,
        );
        append(
            menu,
            ID_OPTIONS,
            &trm("TXT_OPTIONS"),
            false,
            false,
            m.double_click == DoubleClickAction::Options,
            false,
            icons.settings,
        );
        separator(menu);
        let help = CreatePopupMenu().unwrap_or_default();
        item(help, ID_HELP, &trm("TXT_HELP"), icons.help);
        item(help, ID_FAQ, &trm("TXT_FAQ"), none);
        item(help, ID_UPDATE_LOG, &trm("TXT_UPDATE_LOG"), none);
        item(help, ID_ABOUT, &trm("TXT_ABOUT"), icons.about);
        append(
            help,
            ID_CHECK_UPDATE,
            &trm("TXT_CHECK_UPDATE"),
            false,
            m.checking_update,
            false,
            false,
            none,
        );
        submenu(menu, help, &trm("TXT_HELP_MENU"), icons.help);
        if m.elevated {
            item(menu, ID_RESTART, &tr("BAR_RESTART"), icons.restart);
        } else {
            item(
                menu,
                ID_RESTART_AS_ADMIN,
                &tr("BAR_RESTART_AS_ADMIN"),
                icons.admin,
            );
        }
        item(menu, ID_EXIT, &trm("TXT_EXIT"), icons.exit);
        if let Some((_, name, commands)) = &m.plugin {
            separator(menu);
            let sub = CreatePopupMenu().unwrap_or_default();
            for (i, (cmd, checked)) in commands
                .iter()
                .enumerate()
                .take((ID_PLUGIN_COMMAND_MAX - ID_PLUGIN_COMMAND_BASE) as usize)
            {
                append(
                    sub,
                    ID_PLUGIN_COMMAND_BASE + i as u32,
                    cmd,
                    *checked,
                    false,
                    false,
                    false,
                    none,
                );
            }
            if !commands.is_empty() {
                separator(sub);
            }
            item(
                sub,
                ID_PLUGIN_OPTIONS,
                &trm("TXT_PLUGIN_OPTIONS"),
                icons.settings,
            );
            item(sub, ID_PLUGIN_DETAIL, &trm("TXT_PLUGIN_DETAIL"), icons.info);
            submenu(menu, sub, name, none);
        }

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(owner);
        let cmd = TrackPopupMenuEx(
            menu,
            (TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY).0,
            pt.x,
            pt.y,
            owner,
            None,
        );
        let _ = PostMessageW(Some(owner), WM_NULL, Default::default(), Default::default());
        let _ = DestroyMenu(menu);
        if cmd.0 != 0 {
            execute(owner, cmd.0 as u32, m.plugin.map(|p| p.0));
        }
    }
}

pub fn on_command(owner: HWND, id: u32) {
    execute(owner, id, None);
}

fn help_url(kind: u32) -> String {
    let zh = crate::i18n::is_zh();
    let gitee = state::try_with(|s| s.settings.general.update_source == 1).unwrap_or(zh);
    let host = if gitee {
        "https://gitee.com/zhongyang219/TrafficMonitor/blob/master/"
    } else {
        "https://github.com/zhongyang219/TrafficMonitor/blob/master/"
    };
    match kind {
        ID_HELP => {
            if zh {
                "https://github.com/zhongyang219/TrafficMonitor/wiki".into()
            } else {
                "https://github.com/zhongyang219/TrafficMonitor/wiki/Home_en".into()
            }
        }
        ID_FAQ => format!("{host}{}", if zh { "Help.md" } else { "Help_en-us.md" }),
        _ => format!(
            "{host}UpdateLog/{}",
            if zh {
                "update_log.md"
            } else {
                "update_log_en-us.md"
            }
        ),
    }
}

fn select_connection(sel: Selection) {
    state::with_mut(|s| {
        match &sel {
            Selection::Auto => {
                s.settings.cfg.auto_select = true;
                s.settings.cfg.select_all = false;
            }
            Selection::All => {
                s.settings.cfg.auto_select = false;
                s.settings.cfg.select_all = true;
            }
            Selection::Index(i) => {
                s.settings.cfg.auto_select = false;
                s.settings.cfg.select_all = false;
                if let Some(c) = s.connections.get(*i) {
                    s.settings.cfg.connection_name = c.description.clone();
                }
            }
            Selection::Named(_) => {}
        }
        s.selection = sel.clone();
        if let Some(m) = &s.monitor {
            m.send(MonitorCmd::Select(sel));
        }
        s.save_settings();
    });
}

fn execute(owner: HWND, id: u32, plugin: Option<usize>) {
    match id {
        ID_AUTO_SELECT => select_connection(Selection::Auto),
        ID_SELECT_ALL => select_connection(Selection::All),
        ID_CONNECTION_BASE..=ID_CONNECTION_MAX => {
            select_connection(Selection::Index((id - ID_CONNECTION_BASE) as usize))
        }
        ID_REFRESH_CONNECTIONS => {
            state::with(|s| {
                if let Some(m) = &s.monitor {
                    m.send(MonitorCmd::Refresh);
                }
            });
        }
        ID_NETWORK_INFO => app::send_ui(UiCommand::OpenNetworkInfo),
        ID_TRAFFIC_HISTORY => app::send_ui(UiCommand::OpenHistory),
        ID_DISPLAY_SETTINGS => app::send_ui(UiCommand::OpenDisplaySettings),
        ID_TASK_MANAGER => app::open_task_manager(),
        ID_PLUGIN_MANAGE => app::send_ui(UiCommand::OpenPluginManager),
        ID_OPTIONS => app::send_ui(UiCommand::OpenOptions(0)),
        ID_CHANGE_NOTIFY_ICON => app::send_ui(UiCommand::OpenIconSelect),
        ID_HELP | ID_FAQ | ID_UPDATE_LOG => win::shell_open(&help_url(id)),
        ID_ABOUT => app::send_ui(UiCommand::OpenAbout),
        ID_CHECK_UPDATE => app::send_ui(UiCommand::CheckUpdate { silent: false }),
        ID_RESTART_AS_ADMIN => app::restart_as_admin(),
        ID_RESTART => app::restart(),
        ID_EXIT => app::exit(),
        ID_PLUGIN_OPTIONS => {
            if let Some(p) = plugin {
                app::show_plugin_options(p, owner.0 as isize);
            }
        }
        ID_PLUGIN_DETAIL => {
            if let Some(p) = plugin {
                app::send_ui(UiCommand::OpenPluginInfo(p));
            }
        }
        ID_PLUGIN_COMMAND_BASE..=ID_PLUGIN_COMMAND_MAX => {
            if let Some(p) = plugin {
                let plugins = state::with(|s| s.plugins.clone());
                plugins.run_command(p, (id - ID_PLUGIN_COMMAND_BASE) as i32, owner.0 as isize);
            }
        }
        _ => {}
    }
}
