#![allow(unsafe_op_in_unsafe_fn)]

use crate::log;
use std::ptr::null_mut;
use windows_sys::Win32::Graphics::Gdi::{HDC, LOGFONTW};
use windows_sys::Win32::Graphics::GdiPlus::*;

pub const fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

fn mix_channel(a: u32, b: u32, shift: u32, t: f32) -> u32 {
    let ca = (a >> shift) & 0xFF;
    let cb = (b >> shift) & 0xFF;
    let v = ca as f32 + (cb as f32 - ca as f32) * t;
    (v.round().clamp(0.0, 255.0) as u32) << shift
}

pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    mix_channel(a, b, 24, t)
        | mix_channel(a, b, 16, t)
        | mix_channel(a, b, 8, t)
        | mix_channel(a, b, 0, t)
}

pub mod theme {
    use super::argb;

    pub const BG: u32 = argb(255, 10, 13, 20);           // Alienware Deep Charcoal
    pub const CARD: u32 = argb(255, 17, 22, 33);         // Alienware Gunmetal Card
    pub const CARD_HOVER: u32 = argb(255, 23, 31, 46);   // Card hover
    pub const CARD_ACTIVE: u32 = argb(255, 15, 34, 52);  // Card active
    pub const BORDER: u32 = argb(255, 28, 38, 56);       // Crisp border
    pub const BORDER_HOVER: u32 = argb(255, 45, 68, 98); // Border hover
    pub const TEXT: u32 = argb(255, 245, 248, 252);      // Crisp pure white
    pub const TEXT_DIM: u32 = argb(255, 150, 168, 192);  // Muted secondary text
    pub const TEXT_FAINT: u32 = argb(255, 96, 114, 138); // Faint hints
    pub const ACCENT: u32 = argb(255, 0, 229, 255);      // Alienware Electric Cyan (#00E5FF)
    pub const ACCENT_DIM: u32 = argb(255, 0, 140, 175);  // Deep cyan
    pub const ACCENT_GLOW: u32 = argb(75, 0, 229, 255);  // Neon Cyan Glow
    pub const GOOD: u32 = argb(255, 0, 230, 118);        // Eco Green (#00E676)
    pub const WARN: u32 = argb(255, 255, 171, 0);        // Performance Amber (#FFAB00)
    pub const BAD: u32 = argb(255, 255, 82, 82);         // Overdrive Hot Red (#FF5252)
    pub const TRACK: u32 = argb(255, 27, 35, 50);        // Slider track
}

#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontKind {
    Display,
    Title,
    Section,
    Body,
    Label,
    Value,
    Tiny,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

struct Fonts {
    display: *mut GpFont,
    title: *mut GpFont,
    section: *mut GpFont,
    body: *mut GpFont,
    label: *mut GpFont,
    value: *mut GpFont,
    tiny: *mut GpFont,
    left: *mut GpStringFormat,
    center: *mut GpStringFormat,
    right: *mut GpStringFormat,
}

impl Fonts {
    unsafe fn build(hdc: HDC, scale: f32) -> Option<Self> {
        let display = create_font(hdc, "Segoe UI Variable Display", 46.0 * scale, true)?;
        let title = create_font(hdc, "Segoe UI Variable Display", 26.0 * scale, true)?;
        let section = create_font(hdc, "Segoe UI", 11.0 * scale, true)?;
        let body = create_font(hdc, "Segoe UI", 13.0 * scale, false)?;
        let label = create_font(hdc, "Segoe UI", 12.0 * scale, true)?;
        let value = create_font(hdc, "Segoe UI Variable Display", 22.0 * scale, true)?;
        let tiny = create_font(hdc, "Segoe UI", 10.5 * scale, false)?;

        Some(Self {
            display,
            title,
            section,
            body,
            label,
            value,
            tiny,
            left: create_string_format(StringAlignmentNear)?,
            center: create_string_format(StringAlignmentCenter)?,
            right: create_string_format(StringAlignmentFar)?,
        })
    }

    fn get(&self, kind: FontKind) -> *mut GpFont {
        match kind {
            FontKind::Display => self.display,
            FontKind::Title => self.title,
            FontKind::Section => self.section,
            FontKind::Body => self.body,
            FontKind::Label => self.label,
            FontKind::Value => self.value,
            FontKind::Tiny => self.tiny,
        }
    }

    fn format(&self, align: Align) -> *mut GpStringFormat {
        match align {
            Align::Left => self.left,
            Align::Center => self.center,
            Align::Right => self.right,
        }
    }
}

unsafe fn create_font(
    hdc: HDC,
    face: &str,
    size: f32,
    bold: bool,
) -> Option<*mut GpFont> {
    let mut logfont: LOGFONTW = std::mem::zeroed();
    logfont.lfHeight = -(size.round() as i32);
    logfont.lfWeight = if bold { 700 } else { 400 };
    logfont.lfCharSet = 1;
    logfont.lfQuality = 5;
    logfont.lfOutPrecision = 3;
    logfont.lfPitchAndFamily = 0x22;
    let name = wide(face);
    for (index, ch) in name.iter().take(31).enumerate() {
        logfont.lfFaceName[index] = *ch;
    }
    let mut font: *mut GpFont = null_mut();
    let status = GdipCreateFontFromLogfontW(hdc, &logfont, &mut font);
    if status != 0 || font.is_null() {
        log::error(format_args!(
            "GdipCreateFontFromLogfontW failed: status={status} face={face} size={size}"
        ));
        None
    } else {
        Some(font)
    }
}

unsafe fn create_string_format(align: StringAlignment) -> Option<*mut GpStringFormat> {
    let mut format: *mut GpStringFormat = null_mut();
    let status = GdipCreateStringFormat(0, 0, &mut format);
    if status != 0 || format.is_null() {
        return None;
    }
    GdipSetStringFormatFlags(
        format,
        StringFormatFlagsNoWrap | StringFormatFlagsNoClip | StringFormatFlagsNoFitBlackBox | StringFormatFlagsLineLimit,
    );
    GdipSetStringFormatAlign(format, align);
    Some(format)
}

pub struct Painter {
    graphics: *mut GpGraphics,
    fonts: Fonts,
    scale: f32,
}

impl Painter {
    pub fn new(hdc: HDC, scale: f32) -> Option<Self> {
        unsafe {
            let mut graphics: *mut GpGraphics = null_mut();
            if GdipCreateFromHDC(hdc, &mut graphics) != 0 || graphics.is_null() {
                return None;
            }
            GdipSetSmoothingMode(graphics, SmoothingModeAntiAlias);
            GdipSetTextRenderingHint(graphics, TextRenderingHintAntiAliasGridFit);
            let fonts = Fonts::build(hdc, scale)?;
            Some(Self {
                graphics,
                fonts,
                scale,
            })
        }
    }

    fn s(&self, value: f32) -> f32 {
        value * self.scale
    }

    fn brush(&self, color: u32) -> *mut GpBrush {
        unsafe {
            let mut brush: *mut GpSolidFill = null_mut();
            if GdipCreateSolidFill(color, &mut brush) != 0 || brush.is_null() {
                return null_mut();
            }
            brush as *mut GpBrush
        }
    }

    fn pen(&self, color: u32, width: f32) -> *mut GpPen {
        unsafe {
            let mut pen: *mut GpPen = null_mut();
            if GdipCreatePen1(color, self.s(width), UnitPixel, &mut pen) != 0 || pen.is_null() {
                return null_mut();
            }
            pen
        }
    }

    pub fn fill_rect(&self, rect: Rect, color: u32) {
        let brush = self.brush(color);
        if brush.is_null() {
            return;
        }
        unsafe {
            GdipFillRectangle(
                self.graphics,
                brush,
                self.s(rect.x),
                self.s(rect.y),
                self.s(rect.w),
                self.s(rect.h),
            );
            GdipDeleteBrush(brush);
        }
    }

    pub fn fill_round_rect(&self, rect: Rect, radius: f32, color: u32) {
        let path = round_path(self.scaled(rect), self.s(radius));
        if path.is_null() {
            return;
        }
        let brush = self.brush(color);
        unsafe {
            GdipFillPath(self.graphics, brush, path);
            GdipDeleteBrush(brush);
            GdipDeletePath(path);
        }
    }

    pub fn stroke_round_rect(&self, rect: Rect, radius: f32, color: u32, width: f32) {
        let path = round_path(self.scaled(rect), self.s(radius));
        if path.is_null() {
            return;
        }
        let pen = self.pen(color, width);
        unsafe {
            GdipDrawPath(self.graphics, pen, path);
            GdipDeletePen(pen);
            GdipDeletePath(path);
        }
    }

    pub fn stroke_glow_round_rect(&self, rect: Rect, radius: f32, color: u32, glow: u32) {
        self.stroke_round_rect(
            Rect::new(rect.x - 1.0, rect.y - 1.0, rect.w + 2.0, rect.h + 2.0),
            radius + 1.0,
            glow,
            2.0,
        );
        self.stroke_round_rect(rect, radius, color, 1.5);
    }

    pub fn fill_circle(&self, center: (f32, f32), radius: f32, color: u32) {
        let brush = self.brush(color);
        unsafe {
            GdipFillEllipse(
                self.graphics,
                brush,
                self.s(center.0 - radius),
                self.s(center.1 - radius),
                self.s(radius * 2.0),
                self.s(radius * 2.0),
            );
            GdipDeleteBrush(brush);
        }
    }

    pub fn arc(&self, rect: Rect, start: f32, sweep: f32, color: u32, width: f32) {
        let pen = self.pen(color, width);
        unsafe {
            GdipDrawArc(
                self.graphics,
                pen,
                self.s(rect.x),
                self.s(rect.y),
                self.s(rect.w),
                self.s(rect.h),
                start,
                sweep,
            );
            GdipDeletePen(pen);
        }
    }

    pub fn text(&self, text: &str, rect: Rect, kind: FontKind, color: u32, align: Align) {
        if text.is_empty() {
            return;
        }
        let wide = wide(text);
        let brush = self.brush(color);
        if brush.is_null() {
            return;
        }
        let layout = RectF {
            X: self.s(rect.x),
            Y: self.s(rect.y),
            Width: self.s(rect.w),
            Height: self.s(rect.h),
        };
        unsafe {
            GdipDrawString(
                self.graphics,
                wide.as_ptr(),
                -1,
                self.fonts.get(kind),
                &layout,
                self.fonts.format(align),
                brush,
            );
            GdipDeleteBrush(brush);
        }
    }

    pub fn measure(&self, text: &str, kind: FontKind) -> (f32, f32) {
        let wide = wide(text);
        let mut bounds: RectF = RectF {
            X: 0.0,
            Y: 0.0,
            Width: 0.0,
            Height: 0.0,
        };
        let layout = RectF {
            X: 0.0,
            Y: 0.0,
            Width: 10000.0,
            Height: 1000.0,
        };
        let mut fitted: i32 = 0;
        let mut lines: i32 = 0;
        unsafe {
            GdipMeasureString(
                self.graphics,
                wide.as_ptr(),
                -1,
                self.fonts.get(kind),
                &layout,
                self.fonts.format(Align::Left),
                &mut bounds,
                &mut fitted,
                &mut lines,
            );
        }
        (bounds.Width / self.scale, bounds.Height / self.scale)
    }

    fn scaled(&self, rect: Rect) -> RectF {
        RectF {
            X: self.s(rect.x),
            Y: self.s(rect.y),
            Width: self.s(rect.w),
            Height: self.s(rect.h),
        }
    }
}

fn round_path(rect: RectF, radius: f32) -> *mut GpPath {
    let radius = radius.min(rect.Width / 2.0).min(rect.Height / 2.0).max(0.5);
    unsafe {
        let mut path: *mut GpPath = null_mut();
        if GdipCreatePath(FillModeWinding, &mut path) != 0 || path.is_null() {
            return null_mut();
        }
        let d = radius * 2.0;
        let left = rect.X;
        let top = rect.Y;
        let right = rect.X + rect.Width;
        let bottom = rect.Y + rect.Height;

        GdipAddPathArc(path, left, top, d, d, 180.0, 90.0);
        GdipAddPathLine(path, left + radius, top, right - radius, top);
        GdipAddPathArc(path, right - d, top, d, d, 270.0, 90.0);
        GdipAddPathLine(path, right, top + radius, right, bottom - radius);
        GdipAddPathArc(path, right - d, bottom - d, d, d, 0.0, 90.0);
        GdipAddPathLine(path, right - radius, bottom, left + radius, bottom);
        GdipAddPathArc(path, left, bottom - d, d, d, 90.0, 90.0);
        GdipAddPathLine(path, left, bottom - radius, left, top + radius);
        path
    }
}

pub struct GdiPlusGuard {
    token: usize,
}

impl GdiPlusGuard {
    pub fn start() -> Option<Self> {
        unsafe {
            let mut token: usize = 0;
            let input = GdiplusStartupInput {
                GdiplusVersion: 1,
                DebugEventCallback: 0,
                SuppressBackgroundThread: 0,
                SuppressExternalCodecs: 0,
            };
            let mut output: GdiplusStartupOutput = std::mem::zeroed();
            if GdiplusStartup(&mut token, &input, &mut output) != 0 {
                return None;
            }
            Some(Self { token })
        }
    }
}

impl Drop for GdiPlusGuard {
    fn drop(&mut self) {
        unsafe {
            GdiplusShutdown(self.token);
        }
    }
}

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod probe {
    use super::*;
    use windows_sys::Win32::Graphics::Gdi::*;

    #[test]
    fn probe_painting() {
        let _guard = GdiPlusGuard::start();
        unsafe {
            let screen = GetDC(null_mut());
            let dc = CreateCompatibleDC(screen);
            let bitmap = CreateCompatibleBitmap(screen, 400, 300);
            let old = SelectObject(dc, bitmap);

            let mut path: *mut GpPath = null_mut();
            let _ = GdipCreatePath2(std::ptr::null(), std::ptr::null(), 0, FillModeWinding, &mut path);
            if !path.is_null() {
                let _ = GdipAddPathArc(path, 10.0, 10.0, 24.0, 24.0, 180.0, 90.0);
                let _ = GdipAddPathLine(path, 22.0, 10.0, 178.0, 10.0);
                GdipDeletePath(path);
            }

            let Some(painter) = Painter::new(dc, 1.0) else {
                return;
            };

            painter.fill_rect(Rect::new(0.0, 0.0, 400.0, 300.0), theme::BG);
            painter.fill_round_rect(Rect::new(10.0, 10.0, 200.0, 80.0), 12.0, theme::CARD);
            painter.stroke_glow_round_rect(
                Rect::new(10.0, 10.0, 200.0, 80.0),
                12.0,
                theme::ACCENT,
                theme::ACCENT_GLOW,
            );
            painter.arc(Rect::new(20.0, 20.0, 100.0, 100.0), 135.0, 270.0, theme::ACCENT, 10.0);
            painter.fill_circle((100.0, 100.0), 20.0, theme::ACCENT);
            painter.text(
                "Hello 61 C",
                Rect::new(10.0, 110.0, 300.0, 40.0),
                FontKind::Display,
                theme::TEXT,
                Align::Left,
            );

            SelectObject(dc, old);
            DeleteObject(bitmap as HGDIOBJ);
            DeleteDC(dc);
            ReleaseDC(null_mut(), screen);
        }
    }
}
