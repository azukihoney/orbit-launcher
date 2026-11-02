use crate::model::Settings;
use std::ptr::null_mut;
use std::result::Result::Ok;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::{Gdi::*, GdiPlus::*};
use windows_sys::Win32::UI::WindowsAndMessaging::*;
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}
/// Windows font-family buffers contain at most 31 UTF-16 units plus NUL.
pub fn font_face(name: &str) -> [u16; 32] {
    let mut face = [0; 32];
    let mut used = 0;
    for ch in name.chars() {
        if ch == '\0' || used + ch.len_utf16() > 31 {
            break;
        }
        let mut units = [0; 2];
        let encoded = ch.encode_utf16(&mut units);
        face[used..used + encoded.len()].copy_from_slice(encoded);
        used += encoded.len();
    }
    face
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct R {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}
impl R {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
    pub fn has(self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    pub fn inset(self, n: i32) -> Self {
        Self::new(self.x + n, self.y + n, self.w - 2 * n, self.h - 2 * n)
    }
    pub fn rect(self) -> RECT {
        RECT {
            left: self.x,
            top: self.y,
            right: self.x + self.w,
            bottom: self.y + self.h,
        }
    }
    pub fn scale(self, s: f64) -> Self {
        Self::new(
            (self.x as f64 * s).round() as i32,
            (self.y as f64 * s).round() as i32,
            (self.w as f64 * s).round() as i32,
            (self.h as f64 * s).round() as i32,
        )
    }
}
#[derive(Clone, Copy)]
pub struct Palette {
    pub top: u32,
    pub bottom: u32,
    pub text: u32,
    pub quiet: u32,
    pub border: u32,
    pub card: u32,
    pub card_bottom: u32,
    pub card_border: u32,
    pub hover: u32,
    pub hover_bottom: u32,
    pub hover_border: u32,
    pub accent: u32,
}
pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            top: 0xff202632,
            bottom: 0xff181e28,
            text: 0xffe5eaf2,
            quiet: 0xff9ba8bb,
            border: 0xff3b4659,
            card: 0xff2d3544,
            card_bottom: 0xff252d3a,
            card_border: 0xff445066,
            hover: 0xff644835,
            hover_bottom: 0xff59402b,
            hover_border: 0xffdcaa7c,
            accent: 0xfff4c6a0,
        }
    } else {
        Palette {
            top: 0xffffffff,
            bottom: 0xfff8fafc,
            text: 0xff283447,
            quiet: 0xff8a95a5,
            border: 0xffdce3ec,
            card: 0xffffffff,
            card_bottom: 0xfff7f9fc,
            card_border: 0xffe1e6ed,
            hover: 0xfffff9f5,
            hover_bottom: 0xfffff2e9,
            hover_border: 0xffe9cab4,
            accent: 0xffb16d38,
        }
    }
}
pub fn colorref(c: u32) -> u32 {
    ((c >> 16) & 255) | (c & 0x00ff00) | ((c & 255) << 16)
}
pub struct GdiPlus {
    token: usize,
}
impl GdiPlus {
    pub fn new() -> Result<Self, String> {
        let mut token = 0;
        let input = GdiplusStartupInput {
            GdiplusVersion: 1,
            DebugEventCallback: 0,
            SuppressBackgroundThread: 0,
            SuppressExternalCodecs: 0,
        };
        if unsafe { GdiplusStartup(&mut token, &input, null_mut()) } != 0 {
            return Err("GDI+ startup failed".into());
        }
        Ok(Self { token })
    }
}
impl Drop for GdiPlus {
    fn drop(&mut self) {
        unsafe { GdiplusShutdown(self.token) }
    }
}
pub struct Font(pub HFONT);
impl Font {
    pub fn new(name: &str, px: i32, bold: bool) -> Self {
        Self::weighted(name, px, if bold { 600 } else { 400 })
    }
    pub fn weighted(name: &str, px: i32, weight: i32) -> Self {
        let face = font_face(name);
        #[cfg(feature = "review-test")]
        crate::stage2_probe::font("create_font", &face);
        Self(unsafe {
            CreateFontW(
                -px,
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                CLEARTYPE_QUALITY as u32,
                DEFAULT_PITCH as u32,
                face.as_ptr(),
            )
        })
    }
}
impl Drop for Font {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.0);
        }
    }
}
pub struct Fonts {
    pub app: Font,
    pub folder: Font,
    pub ui: Font,
    pub tool_glyph: Font,
    pub quiet: Font,
    pub brand: Font,
}
impl Fonts {
    pub fn new(s: &Settings, scale: f64) -> Self {
        let px = |p: i32| (p as f64 * scale).round() as i32;
        Self {
            app: Font::new(&s.font_family, px(s.app_font_px), false),
            folder: Font::new(&s.font_family, px(s.folder_font_px), false),
            ui: Font::new(&s.font_family, px(12), false),
            tool_glyph: Font::new(&s.font_family, px(18), false),
            quiet: Font::new(&s.font_family, px(11), false),
            brand: if scale < 1. {
                // Weight 600 is emboldened by GDI, one pixel per character: too
                // wide and smeared at small sizes. A real bold keeps its shape.
                Font::weighted(&s.font_family, px(12), 700)
            } else {
                Font::new(&s.font_family, px(12), true)
            },
        }
    }
}
pub struct Icon(pub HICON);
impl Drop for Icon {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                DestroyIcon(self.0);
            }
        }
    }
}
pub struct Canvas {
    pub dc: HDC,
    graphics: *mut GpGraphics,
}
impl Canvas {
    pub fn clear(&self, color: u32) {
        unsafe {
            GdipGraphicsClear(self.graphics, color);
        }
    }
    pub fn new(dc: HDC) -> Self {
        let mut graphics = null_mut();
        unsafe {
            GdipCreateFromHDC(dc, &mut graphics);
            GdipSetSmoothingMode(graphics, SmoothingModeAntiAlias);
        }
        Self { dc, graphics }
    }
    pub fn clip(&self, r: R, draw: impl FnOnce()) {
        unsafe {
            let saved = SaveDC(self.dc);
            IntersectClipRect(self.dc, r.x, r.y, r.x + r.w, r.y + r.h);
            GdipSetClipRect(
                self.graphics,
                r.x as f32,
                r.y as f32,
                r.w as f32,
                r.h as f32,
                CombineModeReplace,
            );
            draw();
            GdipResetClip(self.graphics);
            RestoreDC(self.dc, saved);
        }
    }
    pub fn round(&self, r: R, radius: f32, top: u32, bottom: u32, border: u32, stroke: f32) {
        unsafe {
            let mut path = null_mut();
            GdipCreatePath(FillModeAlternate, &mut path);
            let (x, y, w, h) = (r.x as f32, r.y as f32, r.w as f32 - 1., r.h as f32 - 1.);
            let d = (radius * 2.).min(w).min(h);
            for (cx, cy, a) in [
                (x, y, 180.),
                (x + w - d, y, 270.),
                (x + w - d, y + h - d, 0.),
                (x, y + h - d, 90.),
            ] {
                GdipAddPathArc(path, cx, cy, d, d, a, 90.);
            }
            GdipClosePathFigure(path);
            let mut brush = null_mut();
            let rect = RectF {
                X: x,
                Y: y,
                Width: w.max(1.),
                Height: h.max(1.),
            };
            GdipCreateLineBrushFromRect(
                &rect,
                top,
                bottom,
                LinearGradientModeVertical,
                WrapModeTileFlipXY,
                &mut brush,
            );
            if !brush.is_null() {
                GdipFillPath(self.graphics, brush.cast(), path);
                GdipDeleteBrush(brush.cast());
            }
            if stroke > 0. {
                let mut pen = null_mut();
                GdipCreatePen1(border, stroke, UnitPixel, &mut pen);
                GdipDrawPath(self.graphics, pen, path);
                GdipDeletePen(pen);
            }
            GdipDeletePath(path);
        }
    }
    pub fn line(&self, x1: f32, y1: f32, x2: f32, y2: f32, color: u32, width: f32) {
        unsafe {
            let mut pen = null_mut();
            GdipCreatePen1(color, width, UnitPixel, &mut pen);
            GdipDrawLine(self.graphics, pen, x1, y1, x2, y2);
            GdipDeletePen(pen);
        }
    }
    pub fn ascent(&self, font: &Font) -> i32 {
        unsafe {
            let old = SelectObject(self.dc, font.0);
            let mut metrics: TEXTMETRICW = std::mem::zeroed();
            let found = GetTextMetricsW(self.dc, &mut metrics) != 0;
            SelectObject(self.dc, old);
            if found { metrics.tmAscent } else { 0 }
        }
    }
    pub fn text(&self, text: &str, r: R, font: &Font, color: u32, flags: u32) {
        let mut text = wide(text);
        unsafe {
            let old = SelectObject(self.dc, font.0);
            SetBkMode(self.dc, TRANSPARENT as i32);
            SetTextColor(self.dc, colorref(color));
            DrawTextW(
                self.dc,
                text.as_mut_ptr(),
                (text.len() - 1) as i32,
                &mut r.rect(),
                flags | DT_NOPREFIX,
            );
            SelectObject(self.dc, old);
        }
    }
    pub fn icon(&self, icon: &Icon, r: R) {
        unsafe {
            DrawIconEx(
                self.dc,
                r.x,
                r.y,
                icon.0,
                r.w,
                r.h,
                0,
                null_mut(),
                DI_NORMAL,
            );
        }
    }
    pub fn icon_only_hover(
        &self,
        name: &str,
        inner: R,
        text: R,
        font: &Font,
        palette: Palette,
        scale: f64,
    ) {
        #[cfg(feature = "review-test")]
        let measured_start = std::time::Instant::now();
        unsafe {
            let mut state = 0;
            GdipSaveGraphics(self.graphics, &mut state);
            let dc_state = SaveDC(self.dc);
            let mut path = null_mut();
            GdipCreatePath(FillModeAlternate, &mut path);
            let radius = (7. * scale) as f32;
            let diameter = (radius * 2.).min(inner.w as f32).min(inner.h as f32);
            let (x, y, w, h) = (
                inner.x as f32,
                inner.y as f32,
                inner.w as f32 - 1.,
                inner.h as f32 - 1.,
            );
            for (cx, cy, angle) in [
                (x, y, 180.),
                (x + w - diameter, y, 270.),
                (x + w - diameter, y + h - diameter, 0.),
                (x, y + h - diameter, 90.),
            ] {
                GdipAddPathArc(path, cx, cy, diameter, diameter, angle, 90.);
            }
            GdipClosePathFigure(path);
            GdipSetClipPath(self.graphics, path, CombineModeIntersect);
            let region = CreateRoundRectRgn(
                inner.x,
                inner.y,
                inner.x + inner.w,
                inner.y + inner.h,
                diameter.round() as i32,
                diameter.round() as i32,
            );
            ExtSelectClipRgn(self.dc, region, RGN_AND);
            DeleteObject(region);
            // Flush GDI+ before reading its backing DC; every repaint starts from the clean card.
            GdipFlush(self.graphics, FlushIntentionSync);
            let small_w = (inner.w / 6).max(1);
            let small_h = (inner.h / 6).max(1);
            let temporary = CreateCompatibleDC(self.dc);
            let bitmap = CreateCompatibleBitmap(self.dc, small_w, small_h);
            #[cfg(not(feature = "review-test"))]
            let blur = true;
            #[cfg(feature = "review-test")]
            let blur = crate::icon_only_probe::fixture()["skip_blur"].as_bool() != Some(true);
            if blur && !temporary.is_null() && !bitmap.is_null() {
                let old = SelectObject(temporary, bitmap);
                SetStretchBltMode(temporary, HALFTONE);
                SetBrushOrgEx(temporary, 0, 0, null_mut());
                let copied = StretchBlt(
                    temporary, 0, 0, small_w, small_h, self.dc, inner.x, inner.y, inner.w, inner.h,
                    SRCCOPY,
                );
                GdiFlush();
                SelectObject(temporary, old);
                let mut image = null_mut();
                if copied != 0 && GdipCreateBitmapFromHBITMAP(bitmap, null_mut(), &mut image) == 0 {
                    GdipSetInterpolationMode(self.graphics, InterpolationModeHighQualityBilinear);
                    GdipSetPixelOffsetMode(self.graphics, PixelOffsetModeHalf);
                    GdipDrawImageRectI(
                        self.graphics,
                        image.cast(),
                        inner.x,
                        inner.y,
                        inner.w,
                        inner.h,
                    );
                    GdipDisposeImage(image.cast());
                }
            }
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            if !temporary.is_null() {
                DeleteDC(temporary);
            }
            self.round(
                inner,
                radius,
                (palette.hover & 0x00ffffff) | 0x8c000000,
                (palette.hover_bottom & 0x00ffffff) | 0x8c000000,
                0,
                0.,
            );
            GdipFlush(self.graphics, FlushIntentionSync);
            let old_font = SelectObject(self.dc, font.0);
            let mut metric: TEXTMETRICW = std::mem::zeroed();
            GetTextMetricsW(self.dc, &mut metric);
            let height = metric.tmHeight.max(1);
            let maximum = (text.h / height).clamp(1, 4) as usize;
            let measure = |value: &str| {
                let value = wide(value);
                let mut size = SIZE::default();
                GetTextExtentPoint32W(self.dc, value.as_ptr(), (value.len() - 1) as i32, &mut size);
                size.cx
            };
            let lines = crate::icon_only::lines(name, text.w, maximum, measure);
            let top = text.y + (text.h - lines.len() as i32 * height) / 2;
            for (index, line) in lines.iter().enumerate() {
                self.text(
                    line,
                    R::new(text.x, top + index as i32 * height, text.w, height),
                    font,
                    palette.accent,
                    DT_CENTER | DT_SINGLELINE,
                );
            }
            #[cfg(feature = "review-test")]
            crate::icon_only_probe::event("name_lines", serde_json::json!(lines));
            SelectObject(self.dc, old_font);
            RestoreDC(self.dc, dc_state);
            GdipDeletePath(path);
            GdipRestoreGraphics(self.graphics, state);
        }
        #[cfg(feature = "review-test")]
        crate::icon_only_probe::event(
            "hover_time",
            serde_json::json!({"ms":measured_start.elapsed().as_secs_f64()*1000.}),
        );
    }
    pub fn bin(&self, r: R, empty: bool, p: Palette, scale: f64) {
        let x = r.x as f32;
        let y = r.y as f32;
        let s = scale as f32;
        let ln = |a: f32, b: f32, c: f32, d: f32, color: u32| {
            self.line(x + a * s, y + b * s, x + c * s, y + d * s, color, 1.4 * s)
        };
        ln(10., 11., 33., 11., p.quiet);
        ln(17., 8., 26., 8., p.quiet);
        ln(13., 14., 14., 33., p.quiet);
        ln(14., 33., 29., 33., p.quiet);
        ln(29., 33., 30., 14., p.quiet);
        if empty {
            ln(18., 20., 25., 27., p.accent);
            ln(25., 20., 18., 27., p.accent);
        } else {
            ln(21., 23., 34., 10., p.accent);
            ln(28., 10., 34., 10., p.accent);
            ln(34., 10., 34., 16., p.accent);
        }
    }
}
impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            GdipDeleteGraphics(self.graphics);
        }
    }
}
pub fn buffered_paint(hwnd: HWND, draw: impl FnOnce(&Canvas)) {
    #[cfg(feature = "review-test")]
    let measured_start = std::time::Instant::now();
    unsafe {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let dc = BeginPaint(hwnd, &mut ps);
        let mut r: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut r);
        let mem = CreateCompatibleDC(dc);
        let bitmap = CreateCompatibleBitmap(dc, r.right.max(1), r.bottom.max(1));
        let old = SelectObject(mem, bitmap);
        {
            let canvas = Canvas::new(mem);
            draw(&canvas);
        }
        BitBlt(dc, 0, 0, r.right, r.bottom, mem, 0, 0, SRCCOPY);
        SelectObject(mem, old);
        DeleteObject(bitmap);
        DeleteDC(mem);
        EndPaint(hwnd, &ps);
    }
    #[cfg(feature = "review-test")]
    crate::icon_only_probe::paint_time(measured_start.elapsed().as_secs_f64() * 1000.);
}
