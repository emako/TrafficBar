use std::collections::HashMap;
use std::sync::OnceLock;

use windows::Win32::Globalization::LocaleNameToLCID;
use windows::core::PCWSTR;

use crate::config::ini::Ini;

const LANG_SOURCES: &[(&str, &str)] = &[
    ("de-DE", include_str!("assets/lang/de-DE.ini")),
    ("en-US", include_str!("assets/lang/en-US.ini")),
    ("es-419", include_str!("assets/lang/es-419.ini")),
    ("es-ES", include_str!("assets/lang/es-ES.ini")),
    ("fr-FR", include_str!("assets/lang/fr-FR.ini")),
    ("he-IL", include_str!("assets/lang/he-IL.ini")),
    ("hu-HU", include_str!("assets/lang/hu-HU.ini")),
    ("it-IT", include_str!("assets/lang/it-IT.ini")),
    ("ja-JP", include_str!("assets/lang/ja-JP.ini")),
    ("ko-KR", include_str!("assets/lang/ko-KR.ini")),
    ("pl-PL", include_str!("assets/lang/pl-PL.ini")),
    ("pt-BR", include_str!("assets/lang/pt-BR.ini")),
    ("ru-RU", include_str!("assets/lang/ru-RU.ini")),
    ("sl-SI", include_str!("assets/lang/sl-SI.ini")),
    ("th-TH", include_str!("assets/lang/th-TH.ini")),
    ("tr-TR", include_str!("assets/lang/tr-TR.ini")),
    ("vi-VN", include_str!("assets/lang/vi-VN.ini")),
    ("zh-CN", include_str!("assets/lang/zh-CN.ini")),
    ("zh-TW", include_str!("assets/lang/zh-TW.ini")),
];

struct Loaded {
    bcp47: String,
    display_name: String,
    default_font: String,
    text: HashMap<String, String>,
    menu: HashMap<String, String>,
}

static ALL: OnceLock<Vec<Loaded>> = OnceLock::new();
static CURRENT: OnceLock<usize> = OnceLock::new();

fn load_source(src: &str) -> Loaded {
    let ini = Ini::from_str(src.trim_start_matches('\u{feff}'));
    Loaded {
        bcp47: ini.get_string("general", "BCP_47", "en-US"),
        display_name: ini.get_string("general", "DISPLAY_NAME", "English"),
        default_font: ini.get_string("general", "DEFAULT_FONT", "Segoe UI"),
        text: ini.section_keys("text").into_iter().collect(),
        menu: ini.section_keys("menu").into_iter().collect(),
    }
}

fn all() -> &'static [Loaded] {
    ALL.get_or_init(|| {
        let mut langs: Vec<Loaded> = LANG_SOURCES.iter().map(|(_, src)| load_source(src)).collect();
        langs.sort_by(|a, b| a.bcp47.cmp(&b.bcp47).then(a.display_name.cmp(&b.display_name)));
        langs
    })
}

fn english_index(langs: &[Loaded]) -> usize {
    langs.iter().position(|lang| lang.bcp47 == "en-US").unwrap_or(0)
}

fn match_tag(langs: &[Loaded], tag: &str) -> usize {
    let tag = tag.trim();
    if tag.is_empty() {
        return english_index(langs);
    }
    let exact: Vec<usize> = langs
        .iter()
        .enumerate()
        .filter(|(_, lang)| lang.bcp47.eq_ignore_ascii_case(tag))
        .map(|(index, _)| index)
        .collect();
    if exact.len() == 1 {
        return exact[0];
    }
    let lower = tag.to_ascii_lowercase().replace('_', "-");
    if lower.starts_with("zh") {
        let want = if lower.contains("hant") || lower.contains("-tw") || lower.contains("-hk") || lower.contains("-mo") {
            "zh-TW"
        } else {
            "zh-CN"
        };
        if let Some(index) = langs.iter().position(|lang| lang.bcp47.eq_ignore_ascii_case(want)) {
            return index;
        }
    }
    if lower.starts_with("es") && !lower.starts_with("es-es") {
        if let Some(index) = langs.iter().position(|lang| lang.bcp47 == "es-419") {
            return index;
        }
    }
    let primary = lower.split('-').next().unwrap_or(lower.as_str());
    let same_language: Vec<usize> = langs
        .iter()
        .enumerate()
        .filter(|(_, lang)| lang.bcp47.split('-').next().unwrap_or("").eq_ignore_ascii_case(primary))
        .map(|(index, _)| index)
        .collect();
    if same_language.len() == 1 {
        return same_language[0];
    }
    english_index(langs)
}

/// `configured` is the BCP-47 tag saved in the config. An empty string follows the system language.
pub fn init(configured: &str) {
    let langs = all();
    let index = if configured.trim().is_empty() {
        match_tag(langs, &crate::win::user_ui_language())
    } else {
        langs
            .iter()
            .position(|lang| lang.bcp47.eq_ignore_ascii_case(configured.trim()))
            .unwrap_or_else(|| match_tag(langs, configured))
    };
    let _ = CURRENT.set(index);
}

fn table() -> &'static Loaded {
    let langs = all();
    &langs[CURRENT.get().copied().unwrap_or_else(|| english_index(langs))]
}

pub fn current_bcp47() -> &'static str {
    table().bcp47.as_str()
}

pub fn is_zh() -> bool {
    current_bcp47().eq_ignore_ascii_case("zh-CN")
}

/// Locale passed to gpui-component, which only ships `en`, `zh-CN`, `zh-HK`, and `it`.
pub fn component_locale() -> &'static str {
    match current_bcp47() {
        "zh-CN" => "zh-CN",
        "zh-TW" => "zh-HK",
        "it-IT" => "it",
        _ => "en",
    }
}

pub fn lang_id() -> u16 {
    let wide = crate::win::wide(current_bcp47());
    let id = unsafe { LocaleNameToLCID(PCWSTR(wide.as_ptr()), 0) };
    if id == 0 { 0x0409 } else { id as u16 }
}

/// `(bcp47, display name)` in BCP-47 order, for the language picker.
pub fn choices() -> Vec<(String, String)> {
    all().iter().map(|lang| (lang.bcp47.clone(), lang.display_name.clone())).collect()
}

/// Index in the picker, where 0 is "follow system".
pub fn choice_index(configured: &str) -> usize {
    if configured.trim().is_empty() {
        return 0;
    }
    choices()
        .iter()
        .position(|(id, _)| id.eq_ignore_ascii_case(configured.trim()))
        .map(|index| index + 1)
        .unwrap_or(0)
}

pub fn default_font() -> String {
    table().default_font.clone()
}

/// Dialog / string-table text. Escaped `\r\n` and `\n` sequences from the ini are already unescaped.
pub fn tr(key: &str) -> String {
    let t = table();
    t.text.get(key).or_else(|| t.menu.get(key)).cloned().unwrap_or_else(|| key.to_string())
}

/// Menu text (keeps the `&` mnemonic markers).
pub fn trm(key: &str) -> String {
    let t = table();
    t.menu.get(key).or_else(|| t.text.get(key)).cloned().unwrap_or_else(|| key.to_string())
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn section_keys(src: &str) -> HashSet<(String, String)> {
        let ini = Ini::from_str(src.trim_start_matches('\u{feff}'));
        let mut keys = HashSet::new();
        for section in ["general", "text", "menu"] {
            for (key, _) in ini.section_keys(section) {
                keys.insert((section.to_string(), key));
            }
        }
        keys
    }

    #[test]
    fn every_language_has_the_english_keys() {
        let english = section_keys(include_str!("assets/lang/en-US.ini"));
        for (name, src) in LANG_SOURCES {
            let keys = section_keys(src);
            let missing: Vec<_> = english.difference(&keys).collect();
            assert!(missing.is_empty(), "{name} is missing {missing:?}");
        }
    }

    #[test]
    fn thai_follows_system_wording() {
        let ini = Ini::from_str(include_str!("assets/lang/th-TH.ini").trim_start_matches('\u{feff}'));
        assert_eq!(ini.get_string("general", "BCP_47", ""), "th-TH");
        assert_eq!(ini.get_string("general", "DISPLAY_NAME", ""), "ไทย");
        assert_eq!(ini.get_string("text", "BAR_FOLLOW_SYSTEM", ""), "ตามระบบ");
        assert_eq!(ini.get_string("text", "TXT_OK", ""), "ตกลง");
    }
}
