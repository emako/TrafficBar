//! Menu icons follow `admin_dark.ico`: an ICO whose frames are PNG, 16, 20, 24, 28 and 32 at
//! 32-bit. Light and dark are separate files. The shown frame is `dpi * 16 / 96`.

use std::sync::Mutex;

use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDIBSection, DIB_RGB_COLORS, DeleteObject, HBITMAP,
    HGDIOBJ,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmapSource, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapInterpolationMode,
    WICBitmapInterpolationModeHighQualityCubic, WICBitmapInterpolationModeNearestNeighbor,
    WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnLoad,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::UI::Shell::SHCreateMemStream;

const COUNT: usize = 15;

const CONNECTION_LIGHT: &[u8] = include_bytes!("../assets/icons/menu/connection_light.ico");
const CONNECTION_DARK: &[u8] = include_bytes!("../assets/icons/menu/connection_dark.ico");
const INFO_LIGHT: &[u8] = include_bytes!("../assets/icons/menu/info_light.ico");
const INFO_DARK: &[u8] = include_bytes!("../assets/icons/menu/info_dark.ico");
const STATISTICS_LIGHT: &[u8] = include_bytes!("../assets/icons/menu/statistics_light.ico");
const STATISTICS_DARK: &[u8] = include_bytes!("../assets/icons/menu/statistics_dark.ico");
const DISPLAY_LIGHT: &[u8] = include_bytes!("../assets/icons/menu/display_light.ico");
const DISPLAY_DARK: &[u8] = include_bytes!("../assets/icons/menu/display_dark.ico");
const TASK_LIGHT: &[u8] = include_bytes!("../assets/icons/menu/task_light.ico");
const TASK_DARK: &[u8] = include_bytes!("../assets/icons/menu/task_dark.ico");
const PLUGINS_LIGHT: &[u8] = include_bytes!("../assets/icons/menu/plugins_light.ico");
const PLUGINS_DARK: &[u8] = include_bytes!("../assets/icons/menu/plugins_dark.ico");
const SETTINGS_LIGHT: &[u8] = include_bytes!("../assets/icons/menu/settings_light.ico");
const SETTINGS_DARK: &[u8] = include_bytes!("../assets/icons/menu/settings_dark.ico");
const HELP_LIGHT: &[u8] = include_bytes!("../assets/icons/menu/help_light.ico");
const HELP_DARK: &[u8] = include_bytes!("../assets/icons/menu/help_dark.ico");
const ABOUT_LIGHT: &[u8] = include_bytes!("../assets/icons/menu/about_light.ico");
const ABOUT_DARK: &[u8] = include_bytes!("../assets/icons/menu/about_dark.ico");
const EXIT_LIGHT: &[u8] = include_bytes!("../assets/icons/menu/exit_light.ico");
const EXIT_DARK: &[u8] = include_bytes!("../assets/icons/menu/exit_dark.ico");
const ADMIN_LIGHT: &[u8] = include_bytes!("../assets/icons/menu/admin_light.ico");
const ADMIN_DARK: &[u8] = include_bytes!("../assets/icons/menu/admin_dark.ico");
const RESTART_LIGHT: &[u8] = include_bytes!("../assets/icons/menu/restart_light.ico");
const RESTART_DARK: &[u8] = include_bytes!("../assets/icons/menu/restart_dark.ico");
const NOTIFY_ICON_LIGHT: &[u8] = include_bytes!("../assets/icons/menu/notify_icon_light.ico");
const NOTIFY_ICON_DARK: &[u8] = include_bytes!("../assets/icons/menu/notify_icon_dark.ico");
const MEMORY_LIGHT: &[u8] = include_bytes!("../assets/icons/menu/memory_light.ico");
const MEMORY_DARK: &[u8] = include_bytes!("../assets/icons/menu/memory_dark.ico");
const EXPLORER_LIGHT: &[u8] = include_bytes!("../assets/icons/menu/explorer_light.ico");
const EXPLORER_DARK: &[u8] = include_bytes!("../assets/icons/menu/explorer_dark.ico");

/// Bitmaps for one theme. Handles stay alive for the process; the menu does not own them.
#[derive(Clone, Copy)]
pub struct Set {
    pub connection: HBITMAP,
    pub info: HBITMAP,
    pub statistics: HBITMAP,
    pub display: HBITMAP,
    pub task: HBITMAP,
    pub plugins: HBITMAP,
    pub settings: HBITMAP,
    pub help: HBITMAP,
    pub about: HBITMAP,
    pub exit: HBITMAP,
    pub admin: HBITMAP,
    pub restart: HBITMAP,
    pub notify_icon: HBITMAP,
    pub memory: HBITMAP,
    pub explorer: HBITMAP,
}

impl Set {
    fn from_bits(bits: [usize; COUNT]) -> Self {
        Self {
            connection: hbmp(bits[0]),
            info: hbmp(bits[1]),
            statistics: hbmp(bits[2]),
            display: hbmp(bits[3]),
            task: hbmp(bits[4]),
            plugins: hbmp(bits[5]),
            settings: hbmp(bits[6]),
            help: hbmp(bits[7]),
            about: hbmp(bits[8]),
            exit: hbmp(bits[9]),
            admin: hbmp(bits[10]),
            restart: hbmp(bits[11]),
            notify_icon: hbmp(bits[12]),
            memory: hbmp(bits[13]),
            explorer: hbmp(bits[14]),
        }
    }
}

fn hbmp(bits: usize) -> HBITMAP {
    HBITMAP(bits as *mut _)
}

struct Cache {
    dark: bool,
    px: i32,
    bits: [usize; COUNT],
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

/// Same rule as `CTrafficMonitorApp::DPI(16)`.
pub fn pixel_size(dpi: u32) -> i32 {
    let dpi = if dpi == 0 { 96 } else { dpi };
    ((16 * dpi) / 96).max(1) as i32
}

pub fn load(dark: bool, px: i32) -> Set {
    let px = px.max(1);
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(cache) = guard.as_ref() {
        if cache.dark == dark && cache.px == px {
            return Set::from_bits(cache.bits);
        }
    }
    let bits = decode_all(dark, px);
    if let Some(old) = guard.replace(Cache { dark, px, bits }) {
        discard(&old.bits);
    }
    Set::from_bits(bits)
}

fn discard(bits: &[usize]) {
    for &b in bits {
        if b != 0 {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(b as *mut _));
            }
        }
    }
}

fn ico(dark: bool, index: usize) -> &'static [u8] {
    match (dark, index) {
        (false, 0) => CONNECTION_LIGHT,
        (true, 0) => CONNECTION_DARK,
        (false, 1) => INFO_LIGHT,
        (true, 1) => INFO_DARK,
        (false, 2) => STATISTICS_LIGHT,
        (true, 2) => STATISTICS_DARK,
        (false, 3) => DISPLAY_LIGHT,
        (true, 3) => DISPLAY_DARK,
        (false, 4) => TASK_LIGHT,
        (true, 4) => TASK_DARK,
        (false, 5) => PLUGINS_LIGHT,
        (true, 5) => PLUGINS_DARK,
        (false, 6) => SETTINGS_LIGHT,
        (true, 6) => SETTINGS_DARK,
        (false, 7) => HELP_LIGHT,
        (true, 7) => HELP_DARK,
        (false, 8) => ABOUT_LIGHT,
        (true, 8) => ABOUT_DARK,
        (false, 9) => EXIT_LIGHT,
        (true, 9) => EXIT_DARK,
        (false, 10) => ADMIN_LIGHT,
        (true, 10) => ADMIN_DARK,
        (false, 11) => RESTART_LIGHT,
        (true, 11) => RESTART_DARK,
        (false, 12) => NOTIFY_ICON_LIGHT,
        (true, 12) => NOTIFY_ICON_DARK,
        (false, 13) => MEMORY_LIGHT,
        (true, 13) => MEMORY_DARK,
        (false, 14) => EXPLORER_LIGHT,
        (true, 14) => EXPLORER_DARK,
        _ => &[],
    }
}

/// PNG payload of the directory entry closest to `px`. An exact size wins; a tie prefers the larger frame.
fn ico_png(file: &'static [u8], px: i32) -> &'static [u8] {
    if file.len() < 6 || u16::from_le_bytes([file[2], file[3]]) != 1 {
        return &[];
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
        let len = u32::from_le_bytes(file[entry + 8..entry + 12].try_into().unwrap()) as usize;
        let off = u32::from_le_bytes(file[entry + 12..entry + 16].try_into().unwrap()) as usize;
        if off.saturating_add(len) > file.len() || len < 8 {
            continue;
        }
        let png = &file[off..off + 8];
        if png != [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
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
    match best {
        Some((_, _, off, len)) => &file[off..off + len],
        None => &[],
    }
}

fn decode_all(dark: bool, px: i32) -> [usize; COUNT] {
    let mut bits = [0usize; COUNT];
    let Some(factory) = factory() else {
        return bits;
    };
    for i in 0..COUNT {
        if let Some(bmp) = decode(&factory, ico_png(ico(dark, i), px), px) {
            bits[i] = bmp.0 as usize;
        }
    }
    bits
}

fn factory() -> Option<IWICImagingFactory> {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    });
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok() }
}

/// PNG frame to a 32-bit premultiplied bitmap. Menu items only honor alpha in that form.
fn decode(factory: &IWICImagingFactory, png: &[u8], px: i32) -> Option<HBITMAP> {
    if png.is_empty() || px <= 0 {
        return None;
    }
    unsafe {
        let stream = SHCreateMemStream(Some(png))?;
        let decoder = factory
            .CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnLoad)
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let mut src_w = 0u32;
        let mut src_h = 0u32;
        frame.GetSize(&mut src_w, &mut src_h).ok()?;
        if src_w == 0 || src_h == 0 {
            return None;
        }
        let converter = factory.CreateFormatConverter().ok()?;
        // An exact frame is copied as authored. Only a non-matching DPI is resampled.
        if src_w == px as u32 && src_h == px as u32 {
            converter
                .Initialize(
                    &*frame,
                    &GUID_WICPixelFormat32bppPBGRA,
                    WICBitmapDitherTypeNone,
                    None,
                    0.0,
                    WICBitmapPaletteTypeCustom,
                )
                .ok()?;
            return copy_source(&converter, px as u32);
        }
        let scaler = factory.CreateBitmapScaler().ok()?;
        let mode: WICBitmapInterpolationMode = if src_w * 2 > (px as u32).saturating_mul(3) {
            WICBitmapInterpolationModeHighQualityCubic
        } else {
            WICBitmapInterpolationModeNearestNeighbor
        };
        scaler
            .Initialize(&*frame, px as u32, px as u32, mode)
            .ok()?;
        converter
            .Initialize(
                &*scaler,
                &GUID_WICPixelFormat32bppPBGRA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;
        copy_source(&converter, px as u32)
    }
}

fn copy_source(source: &IWICBitmapSource, px: u32) -> Option<HBITMAP> {
    let (bmp, bits) = empty_dib(px)?;
    let stride = px.saturating_mul(4);
    let len = (stride as usize).saturating_mul(px as usize);
    unsafe {
        let slice = std::slice::from_raw_parts_mut(bits, len);
        if source.CopyPixels(std::ptr::null(), stride, slice).is_err() {
            let _ = DeleteObject(HGDIOBJ(bmp.0));
            return None;
        }
    }
    Some(bmp)
}

fn empty_dib(px: u32) -> Option<(HBITMAP, *mut u8)> {
    unsafe {
        let stride = px.saturating_mul(4);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: px as i32,
                biHeight: -(px as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                biSizeImage: stride.saturating_mul(px),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let bmp = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        if bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(bmp.0));
            return None;
        }
        let len = (stride as usize).saturating_mul(px as usize);
        std::slice::from_raw_parts_mut(bits as *mut u8, len).fill(0);
        Some((bmp, bits as *mut u8))
    }
}
