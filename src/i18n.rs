use std::collections::HashMap;
use std::sync::OnceLock;

use crate::config::ini::Ini;

const ZH_CN: &str = include_str!("assets/lang/zh-CN.ini");
const EN_US: &str = include_str!("assets/lang/en-US.ini");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    ZhCn,
    EnUs,
}

impl Lang {
    pub fn display_name(self) -> &'static str {
        match self {
            Lang::ZhCn => "简体中文",
            Lang::EnUs => "English",
        }
    }

    pub fn from_bcp47(s: &str) -> Option<Lang> {
        let s = s.to_ascii_lowercase();
        if s.starts_with("zh") {
            Some(Lang::ZhCn)
        } else if s.starts_with("en") {
            Some(Lang::EnUs)
        } else {
            None
        }
    }
}

struct Table {
    lang: Lang,
    text: HashMap<String, String>,
    menu: HashMap<String, String>,
    default_font: String,
}

static TABLE: OnceLock<Table> = OnceLock::new();

/// `configured` is the bcp-47 tag saved in the config ("" follows the system language).
pub fn init(configured: &str) {
    let lang = Lang::from_bcp47(configured).unwrap_or_else(system_lang);
    let src = match lang {
        Lang::ZhCn => ZH_CN,
        Lang::EnUs => EN_US,
    };
    let ini = Ini::from_str(src.trim_start_matches('\u{feff}'));
    let mut text: HashMap<String, String> = ini.section_keys("text").into_iter().collect();
    let menu: HashMap<String, String> = ini.section_keys("menu").into_iter().collect();
    for (k, zh, en) in EXTRA {
        text.insert(
            (*k).to_string(),
            match lang {
                Lang::ZhCn => (*zh).to_string(),
                Lang::EnUs => (*en).to_string(),
            },
        );
    }
    let default_font = ini.get_string("general", "DEFAULT_FONT", "Segoe UI");
    let _ = TABLE.set(Table {
        lang,
        text,
        menu,
        default_font,
    });
}

fn table() -> &'static Table {
    TABLE.get_or_init(|| {
        let ini = Ini::from_str(EN_US.trim_start_matches('\u{feff}'));
        Table {
            lang: Lang::EnUs,
            text: ini.section_keys("text").into_iter().collect(),
            menu: ini.section_keys("menu").into_iter().collect(),
            default_font: "Segoe UI".into(),
        }
    })
}

pub fn system_lang() -> Lang {
    let name = crate::win::user_ui_language();
    Lang::from_bcp47(&name).unwrap_or(Lang::EnUs)
}

pub fn current() -> Lang {
    table().lang
}

pub fn is_zh() -> bool {
    current() == Lang::ZhCn
}

pub fn default_font() -> String {
    table().default_font.clone()
}

/// Dialog / string-table text. Escaped `\r\n` and `\n` sequences from the ini are already unescaped.
pub fn tr(key: &str) -> String {
    let t = table();
    t.text
        .get(key)
        .or_else(|| t.menu.get(key))
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

/// Menu text (keeps the `&` mnemonic markers).
pub fn trm(key: &str) -> String {
    let t = table();
    t.menu
        .get(key)
        .or_else(|| t.text.get(key))
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

/// Text without mnemonic markers like `(&O)` or `&`, for GPUI buttons.
pub fn tr_plain(key: &str) -> String {
    strip_mnemonic(&tr(key))
}

pub fn strip_mnemonic(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '(' && i + 3 < chars.len() && chars[i + 1] == '&' && chars[i + 3] == ')' {
            i += 4;
            continue;
        }
        if chars[i] == '&' {
            i += 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

const EXTRA: &[(&str, &str, &str)] = &[
    ("APP_NAME", "TrafficMonitorBar", "TrafficMonitorBar"),
    ("BAR_ADMIN_REQUIRED", "需要管理员权限", "Requires administrator privileges"),
    (
        "BAR_ADMIN_REQUIRED_TIP",
        "当前未以管理员身份运行，此功能不可用。请在右键菜单中选择“以管理员身份重新启动”。",
        "Not running as administrator, so this feature is unavailable. Choose \"Restart as administrator\" from the context menu.",
    ),
    ("BAR_RESTART_AS_ADMIN", "以管理员身份重新启动(&R)", "&Restart as administrator"),
    ("BAR_NO_TASKBAR", "未找到任务栏", "Taskbar not found"),
    (
        "BAR_D2D_FAILED",
        "Direct2D 渲染任务栏窗口失败，任务栏窗口已停止更新。错误代码：",
        "Direct2D failed to render the taskbar window and it has stopped updating. Error code: ",
    ),
    ("BAR_RENDER_MODE", "渲染方式：", "Rendering: "),
    ("BAR_RENDER_ULW", "Direct2D（分层窗口）", "Direct2D (layered window)"),
    ("BAR_RENDER_DCOMP", "Direct2D（DirectComposition）", "Direct2D (DirectComposition)"),
    ("BAR_PLUGIN_TEXT_ONLY", "插件项目仅显示文本，不支持插件自绘。", "Plugin items show text only; plugin custom drawing is not supported."),
    ("BAR_HARDWARE_LIBRARY_MISSING", "未找到硬件监控库 TrafficMonitorBar.Hardware.dll。", "The hardware library TrafficMonitorBar.Hardware.dll was not found."),
    ("BAR_FOLLOW_SYSTEM", "跟随系统", "Follow system"),
    ("BAR_DOWNLOAD", "下载并安装", "Download and install"),
    ("BAR_CHECKING", "正在检查更新...", "Checking for updates..."),
    ("BAR_CURRENT_VERSION", "当前版本：", "Current version: "),
    ("BAR_LATEST_VERSION", "最新版本：", "Latest version: "),
    (
        "BAR_UPDATE_NOT_INSTALLED",
        "当前不是通过安装包运行的版本，无法在程序内更新。请先运行 TrafficMonitorBar-Setup.exe 或 MSI 安装。",
        "This copy was not installed with the package, so in-app updates are unavailable. Run TrafficMonitorBar-Setup.exe or the MSI first.",
    ),
    ("BAR_UPDATE_DOWNLOADING", "正在下载 V%s… %s%", "Downloading V%s… %s%"),
    ("BAR_UPDATE_DOWNLOAD_FAILED", "下载或安装更新失败，请稍后重试。", "Failed to download or install the update. Please try again later."),
    ("BAR_BASED_ON", "基于 TrafficMonitor（作者 ZhongYang）复刻的任务栏版本", "Taskbar-only rewrite based on TrafficMonitor by ZhongYang"),
    ("BAR_SOURCE_CODE", "原版项目主页", "Original project"),
    ("BAR_HOVER_ITEM", "选中项目", "Selected item"),
    ("BAR_SHOW_ITEM", "显示", "Show"),
    ("BAR_LABEL", "标签", "Label"),
    ("BAR_NO_PLUGINS", "未加载任何插件。插件 dll 需放在程序所在目录的 plugins 文件夹下。", "No plugins loaded. Put plugin dlls in the plugins folder next to the program."),
    ("BAR_PLUGIN_COMMANDS", "插件命令", "Plugin commands"),
    ("BAR_UP_DOWN", "上传/下载", "Upload/Download"),
    ("BAR_OFF", "关闭", "Off"),
];
