use std::path::{Path, PathBuf};

/// Line-preserving ini document compatible with the original `CIniHelper` format
/// (`key = value`, quoted strings, comma separated lists).
#[derive(Default, Clone)]
pub struct Ini {
    path: PathBuf,
    sections: Vec<Section>,
}

#[derive(Default, Clone)]
struct Section {
    name: String,
    entries: Vec<(String, String)>,
}

impl Ini {
    pub fn load(path: &Path) -> Self {
        let mut ini = Ini {
            path: path.to_path_buf(),
            sections: Vec::new(),
        };
        if let Ok(bytes) = std::fs::read(path) {
            let text = decode(&bytes);
            ini.parse(&text);
        }
        ini
    }

    pub fn from_str(text: &str) -> Self {
        let mut ini = Ini::default();
        ini.parse(text);
        ini
    }

    fn parse(&mut self, text: &str) {
        let mut current: Option<usize> = None;
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                let name = line[1..line.len() - 1].trim().to_string();
                current = Some(self.section_index_or_insert(&name));
                continue;
            }
            let Some(idx) = current else { continue };
            if let Some(pos) = line.find('=') {
                let key = line[..pos].trim().to_string();
                let value = line[pos + 1..].trim().to_string();
                if !key.is_empty() {
                    let section = &mut self.sections[idx];
                    if let Some(e) = section.entries.iter_mut().find(|(k, _)| *k == key) {
                        e.1 = value;
                    } else {
                        section.entries.push((key, value));
                    }
                }
            }
        }
    }

    fn section_index_or_insert(&mut self, name: &str) -> usize {
        if let Some(i) = self.sections.iter().position(|s| s.name == name) {
            return i;
        }
        self.sections.push(Section {
            name: name.to_string(),
            entries: Vec::new(),
        });
        self.sections.len() - 1
    }

    fn raw(&self, section: &str, key: &str) -> Option<&str> {
        self.sections
            .iter()
            .find(|s| s.name == section)?
            .entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    fn set_raw(&mut self, section: &str, key: &str, value: String) {
        let idx = self.section_index_or_insert(section);
        let s = &mut self.sections[idx];
        if let Some(e) = s.entries.iter_mut().find(|(k, _)| k == key) {
            e.1 = value;
        } else {
            s.entries.push((key.to_string(), value));
        }
    }

    pub fn contains(&self, section: &str, key: &str) -> bool {
        self.raw(section, key).is_some()
    }

    pub fn get_string(&self, section: &str, key: &str, default: &str) -> String {
        match self.raw(section, key) {
            Some(v) => unquote(v),
            None => default.to_string(),
        }
    }

    pub fn set_string(&mut self, section: &str, key: &str, value: &str) {
        let v = if !value.is_empty() && (value.starts_with(' ') || value.ends_with(' ')) {
            format!("\"{value}\"")
        } else {
            value.to_string()
        };
        self.set_raw(section, key, v);
    }

    pub fn get_int(&self, section: &str, key: &str, default: i32) -> i32 {
        match self.raw(section, key) {
            Some(v) => atoi(v),
            None => default,
        }
    }

    pub fn set_int(&mut self, section: &str, key: &str, value: i32) {
        self.set_raw(section, key, value.to_string());
    }

    pub fn get_u32(&self, section: &str, key: &str, default: u32) -> u32 {
        match self.raw(section, key) {
            Some(v) => atoi64(v) as u32,
            None => default,
        }
    }

    pub fn set_u32(&mut self, section: &str, key: &str, value: u32) {
        self.set_raw(section, key, value.to_string());
    }

    pub fn get_bool(&self, section: &str, key: &str, default: bool) -> bool {
        match self.raw(section, key) {
            Some("true") => true,
            Some("false") => false,
            Some(v) => atoi(v) != 0,
            None => default,
        }
    }

    pub fn set_bool(&mut self, section: &str, key: &str, value: bool) {
        self.set_raw(section, key, if value { "true" } else { "false" }.to_string());
    }

    pub fn get_string_list(&self, section: &str, key: &str) -> Vec<String> {
        let Some(v) = self.raw(section, key) else {
            return Vec::new();
        };
        split_string_list(v)
    }

    pub fn set_string_list(&mut self, section: &str, key: &str, values: &[String]) {
        let merged = values
            .iter()
            .map(|v| format!("\"{v}\""))
            .collect::<Vec<_>>()
            .join(",");
        self.set_raw(section, key, merged);
    }

    pub fn section_keys(&self, section: &str) -> Vec<(String, String)> {
        self.sections
            .iter()
            .find(|s| s.name == section)
            .map(|s| {
                s.entries
                    .iter()
                    .map(|(k, v)| (k.clone(), unquote(v)))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut out = String::from("\u{feff}");
        for (i, s) in self.sections.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push('[');
            out.push_str(&s.name);
            out.push_str("]\n");
            for (k, v) in &s.entries {
                out.push_str(k);
                out.push_str(" = ");
                out.push_str(v);
                out.push('\n');
            }
        }
        std::fs::write(&self.path, out)
    }
}

fn decode(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let wide: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&wide);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => crate::win::ansi_to_string(bytes),
    }
}

fn unquote(v: &str) -> String {
    let mut s = v.trim();
    if s.starts_with('"') || s.starts_with('$') {
        s = &s[1..];
    }
    if s.ends_with('"') || s.ends_with('$') {
        s = &s[..s.len() - 1];
    }
    unescape(s)
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some(';') => out.push(';'),
                Some('#') => out.push('#'),
                Some('\\') => out.push('\\'),
                Some(o) => {
                    out.push('\\');
                    out.push(o);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn split_string_list(v: &str) -> Vec<String> {
    let v = v.trim();
    if v.is_empty() {
        return Vec::new();
    }
    let mut items: Vec<String> = v.split("\",\"").map(|s| s.to_string()).collect();
    if let Some(first) = items.first_mut() {
        if first.starts_with('"') {
            first.remove(0);
        }
    }
    if let Some(last) = items.last_mut() {
        if last.ends_with('"') {
            last.pop();
        }
    }
    items.retain(|s| !s.is_empty());
    items
}

pub fn atoi(s: &str) -> i32 {
    atoi64(s) as i32
}

pub fn atoi64(s: &str) -> i64 {
    let s = s.trim().trim_matches('"');
    let mut chars = s.chars().peekable();
    let mut neg = false;
    if let Some(&c) = chars.peek() {
        if c == '-' || c == '+' {
            neg = c == '-';
            chars.next();
        }
    }
    let mut v: i64 = 0;
    for c in chars {
        match c.to_digit(10) {
            Some(d) => v = v.saturating_mul(10).saturating_add(d as i64),
            None => break,
        }
    }
    if neg { -v } else { v }
}
