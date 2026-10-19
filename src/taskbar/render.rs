//! Direct2D renderer for the taskbar window, mirroring the two D2D paths of the original:
//! `UpdateLayeredWindowIndirect` with a GDI compatible DXGI surface, and DirectComposition
//! as the fallback when updating the layered window fails. There is no GDI drawing path.

use windows::Win32::Foundation::{HWND, POINT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
    D2D1_BITMAP_OPTIONS_GDI_COMPATIBLE, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1,
    D2D1_CAP_STYLE_FLAT, D2D1_DASH_STYLE_DASH, D2D1_DEVICE_CONTEXT_OPTIONS_NONE,
    D2D1_DRAW_TEXT_OPTIONS, D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
    D2D1_DRAW_TEXT_OPTIONS_NO_SNAP, D2D1_FACTORY_OPTIONS, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_LINE_JOIN_MITER, D2D1_STROKE_STYLE_PROPERTIES, D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1Bitmap1, ID2D1Device,
    ID2D1DeviceContext, ID2D1Factory, ID2D1Factory1, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_9_3,
    D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_RESOURCE_MISC_GDI_COMPATIBLE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11CreateDevice, ID3D11Device, ID3D11Texture2D,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE,
    DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection,
    IDWriteLocalizedStrings, IDWriteTextFormat, IDWriteTextLayout,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
    DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIDevice, IDXGIFactory2, IDXGISurface, IDXGISurface1,
    IDXGISwapChain1,
};
use windows::Win32::Graphics::Gdi::{AC_SRC_ALPHA, AC_SRC_OVER, BLENDFUNCTION};
use windows::Win32::UI::WindowsAndMessaging::{
    ULW_ALPHA, UPDATELAYEREDWINDOWINFO, UpdateLayeredWindowIndirect,
};
use windows::core::{Interface, PCWSTR, Result};

use crate::config::{FontInfo, color_b, color_g, color_r};
use crate::win::wide;

/// Installed font families, named in the UI locale, for the options font list.
/// `CreateTextFormat` accepts these names. Cached for the process.
pub fn system_font_families() -> Vec<String> {
    static CACHE: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    CACHE.get_or_init(load_font_families).clone()
}

fn load_font_families() -> Vec<String> {
    let mut names = collect_font_families().unwrap_or_else(|e| {
        log::warn!("font enumeration failed: {e}");
        Vec::new()
    });
    if names.is_empty() {
        names.push(crate::i18n::default_font());
    }
    names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names
}

fn collect_font_families() -> Result<Vec<String>> {
    unsafe {
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let mut collection: Option<IDWriteFontCollection> = None;
        factory.GetSystemFontCollection(&mut collection, false)?;
        let Some(collection) = collection else {
            return Ok(Vec::new());
        };
        let locale = wide(&crate::win::user_ui_language());
        let english = wide("en-us");
        let count = collection.GetFontFamilyCount();
        let mut names = Vec::with_capacity(count as usize);
        for i in 0..count {
            let family = collection.GetFontFamily(i)?;
            let strings = family.GetFamilyNames()?;
            let index = locale_index(&strings, &locale)
                .or_else(|| locale_index(&strings, &english))
                .unwrap_or(0);
            if let Some(name) = read_localized(&strings, index)
                && !name.is_empty()
            {
                names.push(name);
            }
        }
        Ok(names)
    }
}

fn locale_index(names: &IDWriteLocalizedStrings, locale: &[u16]) -> Option<u32> {
    let mut index = 0u32;
    let mut exists = windows::core::BOOL(0);
    unsafe {
        names
            .FindLocaleName(PCWSTR(locale.as_ptr()), &mut index, &mut exists)
            .ok()?
    };
    (exists.0 != 0).then_some(index)
}

fn read_localized(names: &IDWriteLocalizedStrings, index: u32) -> Option<String> {
    unsafe {
        let len = names.GetStringLength(index).ok()? as usize;
        let mut buf = vec![0u16; len + 1];
        names.GetString(index, &mut buf).ok()?;
        Some(crate::win::from_wide(&buf))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenderMode {
    /// `RenderType::D2D1`: layered window + `UpdateLayeredWindowIndirect`.
    Layered,
    /// `RenderType::D2D1_WITH_DCOMPOSITION`.
    Composition,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }
    pub fn width(&self) -> i32 {
        self.right - self.left
    }
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
    pub fn is_empty(&self) -> bool {
        self.width() <= 0 || self.height() <= 0
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
    fn f(&self) -> D2D_RECT_F {
        D2D_RECT_F {
            left: self.left as f32,
            top: self.top as f32,
            right: self.right as f32,
            bottom: self.bottom as f32,
        }
    }
}

fn color(c: u32, alpha: u8) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: color_r(c) as f32 / 255.0,
        g: color_g(c) as f32 / 255.0,
        b: color_b(c) as f32 / 255.0,
        a: alpha as f32 / 255.0,
    }
}

/// Process-wide factories and the D3D/D2D device shared by every frame.
pub struct Devices {
    pub dwrite: IDWriteFactory,
    d3d: ID3D11Device,
    _d2d_device: ID2D1Device,
    dc: ID2D1DeviceContext,
    dot_style: ID2D1StrokeStyle,
    brush: ID2D1SolidColorBrush,
}

fn create_d3d(driver: D3D_DRIVER_TYPE) -> Result<ID3D11Device> {
    let levels = [
        D3D_FEATURE_LEVEL_11_1,
        D3D_FEATURE_LEVEL_11_0,
        D3D_FEATURE_LEVEL_10_1,
        D3D_FEATURE_LEVEL_10_0,
        D3D_FEATURE_LEVEL_9_3,
    ];
    let mut device = None;
    unsafe {
        D3D11CreateDevice(
            None,
            driver,
            Default::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )?;
    }
    device.ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))
}

impl Devices {
    pub fn new() -> Result<Self> {
        unsafe {
            let d2d_factory: ID2D1Factory1 = D2D1CreateFactory(
                D2D1_FACTORY_TYPE_SINGLE_THREADED,
                Some(&D2D1_FACTORY_OPTIONS::default()),
            )?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let d3d = create_d3d(D3D_DRIVER_TYPE_HARDWARE)
                .or_else(|_| create_d3d(D3D_DRIVER_TYPE_WARP))?;
            let dxgi: IDXGIDevice = d3d.cast()?;
            let d2d_device = d2d_factory.CreateDevice(&dxgi)?;
            let dc = d2d_device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            let props = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_FLAT,
                endCap: D2D1_CAP_STYLE_FLAT,
                dashCap: D2D1_CAP_STYLE_FLAT,
                lineJoin: D2D1_LINE_JOIN_MITER,
                miterLimit: 10.0,
                dashStyle: D2D1_DASH_STYLE_DASH,
                dashOffset: 1.0,
            };
            let base: &ID2D1Factory = &d2d_factory;
            let dot_style = base.CreateStrokeStyle(&props, None)?;
            let brush = dc.CreateSolidColorBrush(&color(0xFFFFFF, 255), None)?;
            Ok(Devices {
                dwrite,
                d3d,
                _d2d_device: d2d_device,
                dc,
                dot_style,
                brush,
            })
        }
    }
}

/// Text format built from the taskbar font (`CDWriteHelper::SetFont`).
pub struct Font {
    format: IDWriteTextFormat,
    underline: bool,
    strike: bool,
    pub size_px: f32,
}

impl Font {
    pub fn new(dev: &Devices, info: &FontInfo, dpi: u32) -> Result<Self> {
        let size_px = (info.size.max(1) as f32 * dpi as f32 / 72.0).round();
        let name = wide(if info.name.is_empty() {
            "Segoe UI"
        } else {
            &info.name
        });
        let locale = wide("");
        let format = unsafe {
            dev.dwrite.CreateTextFormat(
                PCWSTR(name.as_ptr()),
                None,
                DWRITE_FONT_WEIGHT(if info.bold { 700 } else { 400 }),
                if info.italic {
                    DWRITE_FONT_STYLE_ITALIC
                } else {
                    DWRITE_FONT_STYLE_NORMAL
                },
                DWRITE_FONT_STRETCH_NORMAL,
                size_px,
                PCWSTR(locale.as_ptr()),
            )?
        };
        unsafe {
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        }
        Ok(Font {
            format,
            underline: info.underline,
            strike: info.strike_out,
            size_px,
        })
    }

    /// Plain font with a size in pixels, used to size the GPUI tooltip window.
    pub fn from_px(dev: &Devices, name: &str, size_px: f32) -> Result<Self> {
        let info = FontInfo {
            name: name.to_string(),
            size: 1,
            bold: false,
            italic: false,
            underline: false,
            strike_out: false,
        };
        let mut f = Font::new(dev, &info, 72)?;
        let n = wide(if name.is_empty() { "Segoe UI" } else { name });
        let locale = wide("");
        f.format = unsafe {
            dev.dwrite.CreateTextFormat(
                PCWSTR(n.as_ptr()),
                None,
                DWRITE_FONT_WEIGHT(400),
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size_px,
                PCWSTR(locale.as_ptr()),
            )?
        };
        f.size_px = size_px;
        Ok(f)
    }

    fn layout(
        &self,
        dev: &Devices,
        text: &str,
        w: f32,
        h: f32,
        align: DWRITE_TEXT_ALIGNMENT,
    ) -> Result<IDWriteTextLayout> {
        let t: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let layout = dev
                .dwrite
                .CreateTextLayout(&t, &self.format, w.max(1.0), h.max(1.0))?;
            layout.SetTextAlignment(align)?;
            let range = DWRITE_TEXT_RANGE {
                startPosition: 0,
                length: t.len() as u32,
            };
            if self.underline {
                layout.SetUnderline(true, range)?;
            }
            if self.strike {
                layout.SetStrikethrough(true, range)?;
            }
            Ok(layout)
        }
    }

    /// Width in pixels including trailing spaces, like GDI `GetTextExtent`.
    pub fn text_width(&self, dev: &Devices, text: &str) -> i32 {
        if text.is_empty() {
            return 0;
        }
        match self.layout(dev, text, 10000.0, 1000.0, DWRITE_TEXT_ALIGNMENT_LEADING) {
            Ok(l) => unsafe {
                let mut m = DWRITE_TEXT_METRICS::default();
                if l.GetMetrics(&mut m).is_ok() {
                    m.widthIncludingTrailingWhitespace.ceil() as i32
                } else {
                    0
                }
            },
            Err(_) => 0,
        }
    }
}

enum Target {
    Layered {
        _texture: ID3D11Texture2D,
        surface: IDXGISurface1,
        bitmap: ID2D1Bitmap1,
        size: (u32, u32),
    },
    Composition {
        swap_chain: IDXGISwapChain1,
        _device: IDCompositionDevice,
        _target: IDCompositionTarget,
        _visual: IDCompositionVisual,
        size: (u32, u32),
    },
}

pub struct Renderer {
    pub mode: RenderMode,
    hwnd: HWND,
    target: Option<Target>,
}

/// Drawing API used by the layout code (`IDrawCommon` subset).
pub struct Frame<'a> {
    dev: &'a Devices,
    font: &'a Font,
    color_emoji: bool,
}

impl Frame<'_> {
    pub fn fill_rect(&self, r: Rect, c: u32, alpha: u8) {
        unsafe {
            self.dev.brush.SetColor(&color(c, alpha));
            self.dev.dc.FillRectangle(&r.f(), &self.dev.brush);
        }
    }

    /// `DrawRectOutLine(rect, color, 1, dot_line)`.
    pub fn outline(&self, r: Rect, c: u32, dashed: bool) {
        let f = D2D_RECT_F {
            left: r.left as f32 + 0.5,
            top: r.top as f32 + 0.5,
            right: r.right as f32 - 0.5,
            bottom: r.bottom as f32 - 0.5,
        };
        unsafe {
            self.dev.brush.SetColor(&color(c, 255));
            if dashed {
                self.dev
                    .dc
                    .DrawRectangle(&f, &self.dev.brush, 1.0, &self.dev.dot_style);
            } else {
                self.dev.dc.DrawRectangle(&f, &self.dev.brush, 1.0, None);
            }
        }
    }

    /// Vertical line from (x, bottom) upward by `height` pixels.
    pub fn vline(&self, x: i32, bottom: i32, height: i32, c: u32) {
        if height <= 0 {
            return;
        }
        unsafe {
            self.dev.brush.SetColor(&color(c, 255));
            self.dev.dc.FillRectangle(
                &D2D_RECT_F {
                    left: x as f32,
                    top: (bottom - height) as f32,
                    right: x as f32 + 1.0,
                    bottom: bottom as f32,
                },
                &self.dev.brush,
            );
        }
    }

    pub fn text(&self, r: Rect, text: &str, c: u32, align: Align) {
        if text.is_empty() || r.is_empty() {
            return;
        }
        let a = match align {
            Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
            Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
            Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
        };
        let Ok(layout) = self
            .font
            .layout(self.dev, text, r.width() as f32, r.height() as f32, a)
        else {
            return;
        };
        let mut options: D2D1_DRAW_TEXT_OPTIONS =
            D2D1_DRAW_TEXT_OPTIONS_NO_SNAP | D2D1_DRAW_TEXT_OPTIONS_CLIP;
        if self.color_emoji {
            options |= D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT;
        }
        unsafe {
            self.dev.brush.SetColor(&color(c, 255));
            self.dev.dc.DrawTextLayout(
                windows_numerics::Vector2 {
                    X: r.left as f32,
                    Y: r.top as f32,
                },
                &layout,
                &self.dev.brush,
                options,
            );
        }
    }
}

impl Renderer {
    pub fn new(hwnd: HWND, mode: RenderMode) -> Self {
        Renderer {
            mode,
            hwnd,
            target: None,
        }
    }

    fn ensure_target(&mut self, dev: &Devices, w: u32, h: u32) -> Result<()> {
        let size_ok = match &self.target {
            Some(Target::Layered { size, .. }) | Some(Target::Composition { size, .. }) => {
                *size == (w, h)
            }
            None => false,
        };
        if size_ok {
            return Ok(());
        }
        unsafe {
            dev.dc.SetTarget(None);
        }
        match self.mode {
            RenderMode::Layered => {
                self.target = None;
                let desc = D3D11_TEXTURE2D_DESC {
                    Width: w,
                    Height: h,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                    CPUAccessFlags: 0,
                    MiscFlags: D3D11_RESOURCE_MISC_GDI_COMPATIBLE.0 as u32,
                };
                let mut texture = None;
                unsafe { dev.d3d.CreateTexture2D(&desc, None, Some(&mut texture))? };
                let texture = texture.ok_or_else(|| {
                    windows::core::Error::from(windows::Win32::Foundation::E_FAIL)
                })?;
                let surface: IDXGISurface1 = texture.cast()?;
                let props = D2D1_BITMAP_PROPERTIES1 {
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                    },
                    dpiX: 96.0,
                    dpiY: 96.0,
                    bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_GDI_COMPATIBLE,
                    colorContext: std::mem::ManuallyDrop::new(None),
                };
                let s: IDXGISurface = surface.cast()?;
                let bitmap = unsafe { dev.dc.CreateBitmapFromDxgiSurface(&s, Some(&props))? };
                self.target = Some(Target::Layered {
                    _texture: texture,
                    surface,
                    bitmap,
                    size: (w, h),
                });
            }
            RenderMode::Composition => {
                if let Some(Target::Composition { swap_chain, .. }) = &self.target {
                    unsafe {
                        swap_chain.ResizeBuffers(
                            2,
                            w,
                            h,
                            DXGI_FORMAT_B8G8R8A8_UNORM,
                            Default::default(),
                        )?;
                    }
                    if let Some(Target::Composition { size, .. }) = &mut self.target {
                        *size = (w, h);
                    }
                    return Ok(());
                }
                let dxgi: IDXGIDevice = dev.d3d.cast()?;
                let adapter = unsafe { dxgi.GetAdapter()? };
                let factory: IDXGIFactory2 = unsafe { adapter.GetParent()? };
                let desc = DXGI_SWAP_CHAIN_DESC1 {
                    Width: w,
                    Height: h,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    Stereo: false.into(),
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                    BufferCount: 2,
                    Scaling: DXGI_SCALING_STRETCH,
                    SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                    AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
                    Flags: 0,
                };
                let swap_chain =
                    unsafe { factory.CreateSwapChainForComposition(&dev.d3d, &desc, None)? };
                let device: IDCompositionDevice = unsafe { DCompositionCreateDevice(&dxgi)? };
                let target = unsafe { device.CreateTargetForHwnd(self.hwnd, true)? };
                let visual = unsafe { device.CreateVisual()? };
                unsafe {
                    visual.SetContent(&swap_chain)?;
                    target.SetRoot(&visual)?;
                    device.Commit()?;
                }
                self.target = Some(Target::Composition {
                    swap_chain,
                    _device: device,
                    _target: target,
                    _visual: visual,
                    size: (w, h),
                });
            }
        }
        Ok(())
    }

    /// Renders one frame. `background` is `None` for a transparent background.
    /// Returns `Err` with the Win32/HRESULT code when presenting failed.
    pub fn render(
        &mut self,
        dev: &Devices,
        font: &Font,
        size: (i32, i32),
        background: Option<u32>,
        color_emoji: bool,
        draw: impl FnOnce(&Frame),
    ) -> std::result::Result<(), i32> {
        let (w, h) = (size.0.max(1) as u32, size.1.max(1) as u32);
        self.ensure_target(dev, w, h).map_err(|e| e.code().0)?;
        unsafe {
            match self.target.as_ref().unwrap() {
                Target::Layered { bitmap, .. } => dev.dc.SetTarget(bitmap),
                Target::Composition { swap_chain, .. } => {
                    let surface: IDXGISurface = swap_chain.GetBuffer(0).map_err(|e| e.code().0)?;
                    let props = D2D1_BITMAP_PROPERTIES1 {
                        pixelFormat: D2D1_PIXEL_FORMAT {
                            format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                        },
                        dpiX: 96.0,
                        dpiY: 96.0,
                        bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                        colorContext: std::mem::ManuallyDrop::new(None),
                    };
                    let bitmap = dev
                        .dc
                        .CreateBitmapFromDxgiSurface(&surface, Some(&props))
                        .map_err(|e| e.code().0)?;
                    dev.dc.SetTarget(&bitmap);
                }
            }
            dev.dc.SetDpi(96.0, 96.0);
            dev.dc.SetAntialiasMode(D2D1_ANTIALIAS_MODE_ALIASED);
            dev.dc.SetTextAntialiasMode(if background.is_some() {
                D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE
            } else {
                D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE
            });
            dev.dc.BeginDraw();
            match background {
                Some(c) => dev.dc.Clear(Some(&color(c, 255))),
                // Alpha 1 keeps the transparent area hit-testable, like the original renderer.
                None => dev.dc.Clear(Some(&color(0, 1))),
            }
            let frame = Frame {
                dev,
                font,
                color_emoji,
            };
            draw(&frame);
            let end = dev.dc.EndDraw(None, None);
            if let Err(e) = end {
                dev.dc.SetTarget(None);
                return Err(e.code().0);
            }
            match self.target.as_ref().unwrap() {
                Target::Layered { surface, .. } => {
                    let hdc = surface.GetDC(false).map_err(|e| e.code().0)?;
                    let sz = SIZE {
                        cx: w as i32,
                        cy: h as i32,
                    };
                    let src = POINT { x: 0, y: 0 };
                    let blend = BLENDFUNCTION {
                        BlendOp: AC_SRC_OVER as u8,
                        BlendFlags: 0,
                        SourceConstantAlpha: 255,
                        AlphaFormat: AC_SRC_ALPHA as u8,
                    };
                    let info = UPDATELAYEREDWINDOWINFO {
                        cbSize: std::mem::size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
                        hdcDst: Default::default(),
                        pptDst: std::ptr::null(),
                        psize: &sz,
                        hdcSrc: hdc,
                        pptSrc: &src,
                        crKey: Default::default(),
                        pblend: &blend,
                        dwFlags: ULW_ALPHA,
                        prcDirty: std::ptr::null(),
                    };
                    let r = UpdateLayeredWindowIndirect(self.hwnd, &info);
                    let err = windows::Win32::Foundation::GetLastError();
                    let _ = surface.ReleaseDC(None);
                    if !r.as_bool() {
                        return Err(err.0 as i32);
                    }
                }
                Target::Composition { swap_chain, .. } => {
                    dev.dc.SetTarget(None);
                    swap_chain
                        .Present(1, DXGI_PRESENT(0))
                        .ok()
                        .map_err(|e| e.code().0)?;
                }
            }
        }
        Ok(())
    }

    pub fn reset(&mut self, dev: &Devices) {
        unsafe { dev.dc.SetTarget(None) };
        self.target = None;
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        self.target = None;
    }
}
