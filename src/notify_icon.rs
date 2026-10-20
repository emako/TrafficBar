//! Tray icons embedded from `src/assets/icons/*.ico` (not subdirectories).
//!
//! A frame is either a PNG or a 32-bit DIB, the same two layouts as the menu icons.
//! `*_dark.ico` pairs with `*_light.ico` the way the original swaps icon 0 with 4.

include!(concat!(env!("OUT_DIR"), "/notify_icons.rs"));

const DEFAULT: &str = "notify_dark.ico";

pub fn names() -> Vec<String> {
    CATALOG.iter().map(|(name, _)| (*name).to_string()).collect()
}

pub fn contains(name: &str) -> bool {
    CATALOG.iter().any(|(n, _)| *n == name)
}

pub fn bytes(name: &str) -> Option<&'static [u8]> {
    CATALOG
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, bytes)| *bytes)
}

/// `CTrafficMonitorApp::AutoSelectNotifyIcon`, keyed by file name instead of a fixed index.
pub fn adapt(name: &str, light: bool) -> String {
    let name = if contains(name) { name } else { DEFAULT };
    let Some(other) = pair(name) else {
        return name.to_string();
    };
    if !contains(&other) {
        return name.to_string();
    }
    let dark_art = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name).ends_with("_dark");
    let light_art = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name).ends_with("_light");
    if light && dark_art {
        return other;
    }
    if !light && light_art {
        return other;
    }
    name.to_string()
}

fn pair(name: &str) -> Option<String> {
    let stem = name.strip_suffix(".ico").unwrap_or(name);
    if let Some(base) = stem.strip_suffix("_dark") {
        Some(format!("{base}_light.ico"))
    } else {
        stem.strip_suffix("_light")
            .map(|base| format!("{base}_dark.ico"))
    }
}

pub fn load(name: &str, size: i32) -> windows::Win32::UI::WindowsAndMessaging::HICON {
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateIconFromResourceEx, HICON, LR_DEFAULTCOLOR,
    };
    let Some(file) = bytes(name).or_else(|| bytes(DEFAULT)) else {
        return HICON::default();
    };
    let Some(frame) = frame(file, size.max(1)) else {
        return HICON::default();
    };
    unsafe {
        CreateIconFromResourceEx(
            frame,
            true,
            0x0003_0000,
            size.max(1),
            size.max(1),
            LR_DEFAULTCOLOR,
        )
        .unwrap_or_default()
    }
}

/// PNG bytes of the frame closest to 32px, for the GPUI preview.
pub fn preview_png(name: &str) -> Option<Vec<u8>> {
    let file = bytes(name)?;
    let frame = frame(file, 32)?;
    if frame.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some(frame.to_vec());
    }
    dib_png(frame)
}

fn frame(file: &[u8], px: i32) -> Option<&[u8]> {
    if file.len() < 6 || u16::from_le_bytes([file[2], file[3]]) != 1 {
        return None;
    }
    let count = u16::from_le_bytes([file[4], file[5]]) as usize;
    let mut best: Option<(i32, i32, usize, usize)> = None;
    for i in 0..count {
        let entry = 6 + i * 16;
        if entry + 16 > file.len() {
            break;
        }
        let mut side = file[entry] as i32;
        if side == 0 {
            side = 256;
        }
        let len = u32::from_le_bytes(file[entry + 8..entry + 12].try_into().ok()?) as usize;
        let off = u32::from_le_bytes(file[entry + 12..entry + 16].try_into().ok()?) as usize;
        if off.saturating_add(len) > file.len() || len == 0 {
            continue;
        }
        let dist = (side - px).abs();
        let replace = match best {
            None => true,
            Some((d, s, _, _)) => dist < d || (dist == d && side > s),
        };
        if replace {
            best = Some((dist, side, off, len));
        }
    }
    best.map(|(_, _, off, len)| &file[off..off + len])
}

/// 32-bit ICO XOR bitmap (`biHeight` is twice the side) to a PNG. Alpha is already in the pixel.
fn dib_png(dib: &[u8]) -> Option<Vec<u8>> {
    if dib.len() < 40 || u32::from_le_bytes(dib[0..4].try_into().ok()?) != 40 {
        return None;
    }
    let bit_count = u16::from_le_bytes(dib[14..16].try_into().ok()?);
    let width = i32::from_le_bytes(dib[4..8].try_into().ok()?);
    let height = i32::from_le_bytes(dib[8..12].try_into().ok()?);
    if bit_count != 32 || width <= 0 || height <= 0 {
        return None;
    }
    let side = height / 2;
    if side != width {
        return None;
    }
    let stride = (side as usize).saturating_mul(4);
    let xor = stride.saturating_mul(side as usize);
    if 40usize.saturating_add(xor) > dib.len() {
        return None;
    }
    let mut rgba = vec![0u8; xor];
    for y in 0..side as usize {
        let src = 40 + (side as usize - 1 - y) * stride;
        let dst = y * stride;
        for x in 0..side as usize {
            let s = src + x * 4;
            let d = dst + x * 4;
            rgba[d] = dib[s + 2];
            rgba[d + 1] = dib[s + 1];
            rgba[d + 2] = dib[s];
            rgba[d + 3] = dib[s + 3];
        }
    }
    Some(encode_png(side as u32, &rgba))
}

fn encode_png(side: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity((side as usize + 1) * rgba.len() / 4);
    let stride = (side as usize) * 4;
    for y in 0..side as usize {
        raw.push(0);
        raw.extend_from_slice(&rgba[y * stride..(y + 1) * stride]);
    }
    let idat = zlib_store(&raw);
    let mut png = Vec::new();
    png.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&side.to_be_bytes());
    ihdr.extend_from_slice(&side.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &idat);
    chunk(&mut png, b"IEND", &[]);
    png
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_data = kind.to_vec();
    crc_data.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_data).to_be_bytes());
}

fn zlib_store(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut i = 0;
    while i < data.len() || data.is_empty() && i == 0 {
        let n = (data.len() - i).min(65535);
        let last = i + n >= data.len();
        out.push(if last { 1 } else { 0 });
        out.extend_from_slice(&(n as u16).to_le_bytes());
        out.extend_from_slice(&(!n as u16).to_le_bytes());
        out.extend_from_slice(&data[i..i + n]);
        i += n;
        if data.is_empty() {
            break;
        }
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            let bit = c & 1;
            c >>= 1;
            if bit != 0 {
                c ^= 0xEDB8_8320;
            }
        }
    }
    !c
}

fn adler32(data: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}
