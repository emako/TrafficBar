use std::io::Write;
use std::path::PathBuf;

use chrono::{Datelike, Local, NaiveDate};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HistoryRecord {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub up_kbytes: u64,
    pub down_kbytes: u64,
    /// Older files only stored the total; it is kept in `down_kbytes`.
    pub mixed: bool,
}

impl HistoryRecord {
    pub fn kbytes(&self) -> u64 {
        self.up_kbytes + self.down_kbytes
    }

    fn key(&self) -> (i32, u32, u32) {
        (self.year, self.month, self.day)
    }

    pub fn date(&self) -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(self.year, self.month, self.day)
    }

    fn today() -> Self {
        let now = Local::now();
        HistoryRecord {
            year: now.year(),
            month: now.month(),
            day: now.day(),
            mixed: false,
            ..Default::default()
        }
    }
}

/// `CHistoryTrafficFile`: the first record is always today, the rest is sorted newest first.
pub struct HistoryTraffic {
    path: PathBuf,
    pub today: HistoryRecord,
    pub history: Vec<HistoryRecord>,
}

impl HistoryTraffic {
    pub fn load(path: PathBuf) -> Self {
        let mut h = HistoryTraffic {
            path,
            today: HistoryRecord::today(),
            history: Vec::new(),
        };
        let mut first = true;
        let mut today_from_file: Option<HistoryRecord> = None;
        if let Ok(text) = std::fs::read_to_string(&h.path) {
            for line in text.lines().skip(1) {
                let Some(r) = parse_line(line) else { continue };
                if r.kbytes() == 0 {
                    continue;
                }
                if first {
                    today_from_file = Some(r);
                    first = false;
                } else {
                    h.history.push(r);
                }
            }
        }
        if let Some(t) = today_from_file {
            h.history.push(t);
        }
        h.normalize();
        h
    }

    fn normalize(&mut self) {
        let today = HistoryRecord::today();
        self.history.sort_by(|a, b| b.key().cmp(&a.key()));
        let mut merged: Vec<HistoryRecord> = Vec::with_capacity(self.history.len());
        for r in self.history.drain(..) {
            if let Some(last) = merged.last_mut() {
                if last.key() == r.key() {
                    last.up_kbytes += r.up_kbytes;
                    last.down_kbytes += r.down_kbytes;
                    last.mixed = last.mixed && r.mixed;
                    continue;
                }
            }
            merged.push(r);
        }
        merged.retain(|r| r.key() <= today.key());
        if let Some(pos) = merged.iter().position(|r| r.key() == today.key()) {
            let r = merged.remove(pos);
            self.today.up_kbytes += r.up_kbytes;
            self.today.down_kbytes += r.down_kbytes;
        }
        self.today.mixed = false;
        self.history = merged;
    }

    pub fn today_bytes(&self) -> (u64, u64) {
        (self.today.up_kbytes * 1024, self.today.down_kbytes * 1024)
    }

    /// Returns true when the local date changed and today's record was rotated.
    pub fn check_date(&mut self) -> bool {
        let t = HistoryRecord::today();
        if t.key() == self.today.key() {
            return false;
        }
        if self.today.kbytes() > 0 {
            let old = self.today;
            self.history.insert(0, old);
            self.history.sort_by(|a, b| b.key().cmp(&a.key()));
        }
        self.today = t;
        true
    }

    pub fn set_today(&mut self, up_bytes: u64, down_bytes: u64) {
        self.today.up_kbytes = up_bytes / 1024;
        self.today.down_kbytes = down_bytes / 1024;
    }

    /// All records, newest first (today included).
    pub fn all(&self) -> Vec<HistoryRecord> {
        let mut v = Vec::with_capacity(self.history.len() + 1);
        v.push(self.today);
        v.extend_from_slice(&self.history);
        v
    }

    pub fn save(&self) {
        let mut out = String::new();
        out.push_str(&format!("lines: \"{}\"\n", 1 + self.history.len()));
        write_record(&mut out, &self.today);
        for r in &self.history {
            write_record(&mut out, r);
        }
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = self.path.with_extension("dat.tmp");
        if let Ok(mut f) = std::fs::File::create(&tmp) {
            if f.write_all(out.as_bytes()).is_ok() && f.sync_all().is_ok() {
                drop(f);
                let _ = std::fs::rename(&tmp, &self.path);
                return;
            }
        }
        let _ = std::fs::write(&self.path, out);
    }
}

fn write_record(out: &mut String, r: &HistoryRecord) {
    if r.mixed {
        out.push_str(&format!("{:04}/{:02}/{:02} {}\n", r.year, r.month, r.day, r.down_kbytes));
    } else {
        out.push_str(&format!(
            "{:04}/{:02}/{:02} {}/{}\n",
            r.year, r.month, r.day, r.up_kbytes, r.down_kbytes
        ));
    }
}

fn parse_line(line: &str) -> Option<HistoryRecord> {
    let line = line.trim_end();
    if line.len() < 12 || !line.is_char_boundary(11) {
        return None;
    }
    let year: i32 = line.get(0..4)?.parse().ok()?;
    let month: u32 = line.get(5..7)?.parse().ok()?;
    let day: u32 = line.get(8..10)?.parse().ok()?;
    if !(1900..=3000).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let rest = &line[11..];
    let mut r = HistoryRecord {
        year,
        month,
        day,
        ..Default::default()
    };
    if let Some((up, down)) = rest.split_once('/') {
        r.up_kbytes = up.trim().parse().unwrap_or(0);
        r.down_kbytes = down.trim().parse().unwrap_or(0);
        r.mixed = false;
    } else {
        r.down_kbytes = rest.trim().parse().unwrap_or(0);
        r.mixed = true;
    }
    Some(r)
}
