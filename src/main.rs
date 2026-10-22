#![cfg_attr(not(test), windows_subsystem = "windows")]

mod awcc;
mod load;
mod log;
mod ui;

use anyhow::{Result, bail};
use awcc::{
    Awcc, FAN_CPU, FAN_GPU, PROFILE_CUSTOM, PROFILE_PERFORMANCE, PROFILE_QUIET, PROFILES, Status,
    TccStatus,
};
use load::{LoadSampler, Loads};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use ui::{Align, FontKind, GdiPlusGuard, Painter, Rect, mix, theme};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
use windows_sys::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem, SetProcessDpiAwarenessContext,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows_sys::Win32::UI::Shell::{IsUserAnAdmin, ShellExecuteW};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const CLASS_NAME: &str = "AwccLiteWindow";
const APP_TITLE: &str = "AWCC Lite";
const TIMER_ID: usize = 1;
const POLL_MS: u32 = 2000;

const UI_WIDTH: f32 = 700.0;
const UI_HEIGHT: f32 = 830.0;

const PAD: f32 = 24.0;

const MODE_HINTS: [&str; 6] = [
    "Express charge, quiet",
    "Minimum noise",
    "Everyday default",
    "Boosted clocks",
    "Maximum speed",
    "Your own fan curve",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    None,
    Profile(usize),
    Stealth,
    TccTrack,
    TccApply,
    Refresh,
    Logs,
    Elevate,
}

struct Layout {
    cpu: Rect,
    gpu: Rect,
    fans: Rect,
    profile: Rect,
    tcc: Rect,
    tiles: [Rect; 6],
    stealth: Rect,
    tcc_track: Rect,
    tcc_apply: Rect,
    refresh: Rect,
    logs: Rect,
    elevate: Rect,
    footer: Rect,
}

fn build_layout() -> Layout {
    let card_w = UI_WIDTH - PAD * 2.0;
    let half = (card_w - 16.0) / 2.0;

    let y_cpu = 96.0;
    let h_cpu = 186.0;
    let y_fans = y_cpu + h_cpu + 14.0;
    let h_fans = 116.0;
    let y_profile = y_fans + h_fans + 14.0;
    let h_profile = 214.0;
    let y_tcc = y_profile + h_profile + 14.0;
    let h_tcc = 124.0;
    let y_footer = y_tcc + h_tcc + 14.0;

    let inner = card_w - 40.0;
    let tile_w = (inner - 24.0) / 3.0;
    let tile_h = 64.0;
    let mut tiles = [Rect::new(0.0, 0.0, 0.0, 0.0); 6];
    for (index, tile) in tiles.iter_mut().enumerate() {
        let column = index % 3;
        let row = index / 3;
        *tile = Rect::new(
            PAD + 20.0 + column as f32 * (tile_w + 12.0),
            y_profile + 58.0 + row as f32 * (tile_h + 10.0),
            tile_w,
            tile_h,
        );
    }

    Layout {
        cpu: Rect::new(PAD, y_cpu, half, h_cpu),
        gpu: Rect::new(PAD + half + 16.0, y_cpu, half, h_cpu),
        fans: Rect::new(PAD, y_fans, card_w, h_fans),
        profile: Rect::new(PAD, y_profile, card_w, h_profile),
        tcc: Rect::new(PAD, y_tcc, card_w, h_tcc),
        tiles,
        stealth: Rect::new(PAD + card_w - 20.0 - 54.0, y_profile + 20.0, 54.0, 28.0),
        tcc_track: Rect::new(PAD + 20.0, y_tcc + 64.0, 392.0, 8.0),
        tcc_apply: Rect::new(PAD + card_w - 20.0 - 96.0, y_tcc + 50.0, 96.0, 34.0),
        refresh: Rect::new(UI_WIDTH - PAD - 84.0, y_footer, 84.0, 32.0),
        logs: Rect::new(UI_WIDTH - PAD - 84.0 - 10.0 - 108.0, y_footer, 108.0, 32.0),
        elevate: Rect::new(UI_WIDTH - PAD - 92.0, 22.0, 92.0, 32.0),
        footer: Rect::new(PAD, y_footer, card_w, 32.0),
    }
}

struct App {
    hwnd: HWND,
    scale: f32,
    admin: bool,
    awcc: Option<Awcc>,
    status: Option<Status>,
    tcc: Option<TccStatus>,
    tcc_failed: bool,
    tcc_pending: i32,
    loads: Loads,
    sampler: LoadSampler,
    error: Option<String>,
    notice: Option<String>,
    stealth_on: bool,
    stealth_previous: u8,
    hover: Hit,
    pressed: Hit,
    dragging: bool,
    layout: Layout,
    buffer_dc: HDC,
    buffer_bitmap: HBITMAP,
    buffer_old: HBITMAP,
    buffer_w: i32,
    buffer_h: i32,
    painter: Option<Painter>,
}

impl App {
    fn new() -> Self {
        Self {
            hwnd: null_mut(),
            scale: 1.0,
            admin: unsafe { IsUserAnAdmin() != 0 },
            awcc: None,
            status: None,
            tcc: None,
            tcc_failed: false,
            tcc_pending: 0,
            loads: Loads::default(),
            sampler: LoadSampler::new(),
            error: None,
            notice: None,
            stealth_on: false,
            stealth_previous: PROFILE_PERFORMANCE,
            hover: Hit::None,
            pressed: Hit::None,
            dragging: false,
            layout: build_layout(),
            buffer_dc: null_mut(),
            buffer_bitmap: null_mut(),
            buffer_old: null_mut(),
            buffer_w: 0,
            buffer_h: 0,
            painter: None,
        }
    }

    fn init(&mut self, hwnd: HWND, scale: f32) {
        self.hwnd = hwnd;
        self.scale = scale;
        self.layout = build_layout();
        log::info(format_args!(
            "window init: admin={} scale={scale:.2}",
            self.admin
        ));

        match Awcc::open() {
            Ok(awcc) => {
                log::info(format_args!("AWCC interface: {}", awcc.instance_name));
                self.awcc = Some(awcc);
            }
            Err(error) => {
                log::error(format_args!("AWCC open failed: {error:#}"));
                self.error = Some(format!("{error:#}"));
            }
        }

        unsafe {
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: self.px(UI_WIDTH as i32),
                bottom: self.px(UI_HEIGHT as i32),
            };
            AdjustWindowRect(
                &mut rect,
                WS_OVERLAPPEDWINDOW & !WS_THICKFRAME & !WS_MAXIMIZEBOX,
                0,
            );
            SetWindowPos(
                hwnd,
                null_mut(),
                0,
                0,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            SetTimer(hwnd, TIMER_ID, POLL_MS, None);
        }

        self.refresh();
    }

    fn px(&self, value: i32) -> i32 {
        (value as f32 * self.scale).round() as i32
    }

    fn can_write(&self) -> bool {
        self.admin && self.awcc.is_some()
    }

    fn refresh(&mut self) {
        let result = self.awcc.as_ref().map(|awcc| awcc.status());
        match result {
            Some(Ok(status)) => {
                log::info(format_args!("status: {}", status_summary(&status)));
                self.status = Some(status);
                self.error = None;
            }
            Some(Err(error)) => {
                log::error(format_args!("status failed: {error:#}"));
                self.error = Some(format!("{error:#}"));
            }
            None => {}
        }
        self.loads = self.sampler.sample();
        self.refresh_tcc();
        self.invalidate();
    }

    fn refresh_tcc(&mut self) {
        if self.tcc_failed {
            return;
        }
        match self.awcc.as_ref().map(|awcc| awcc.tcc()) {
            Some(Ok(status)) => {
                if !status.supported {
                    log::warn(format_args!("TCC control is not exposed by this firmware"));
                    self.tcc_failed = true;
                    self.tcc = None;
                } else {
                    self.tcc = Some(status);
                }
            }
            Some(Err(error)) => {
                log::error(format_args!("tcc read failed: {error:#}"));
                self.tcc_failed = true;
                self.tcc = None;
            }
            None => {}
        }
    }

    fn tcc_max(&self) -> i32 {
        self.tcc
            .as_ref()
            .and_then(|tcc| tcc.max_offset)
            .unwrap_or(0)
            .clamp(0, 255)
    }

    fn tcc_current(&self) -> i32 {
        self.tcc.as_ref().and_then(|tcc| tcc.offset).unwrap_or(0)
    }

    fn tcc_value(&self) -> i32 {
        self.tcc_pending.clamp(0, self.tcc_max())
    }

    fn invalidate(&self) {
        unsafe {
            InvalidateRect(self.hwnd, null(), 0);
        }
    }

    fn hit_at(&self, x: f32, y: f32) -> Hit {
        let layout = &self.layout;
        for (index, tile) in layout.tiles.iter().enumerate() {
            if tile.contains(x, y) {
                return Hit::Profile(index);
            }
        }
        if layout.stealth.contains(x, y) {
            return Hit::Stealth;
        }
        if layout.tcc_track.contains(x, y)
            || layout
                .tcc_apply
                .contains(x, y)
        {
            if layout.tcc_apply.contains(x, y) {
                return Hit::TccApply;
            }
            return Hit::TccTrack;
        }
        if layout.refresh.contains(x, y) {
            return Hit::Refresh;
        }
        if layout.logs.contains(x, y) {
            return Hit::Logs;
        }
        if layout.elevate.contains(x, y) {
            return Hit::Elevate;
        }
        Hit::None
    }

    fn slider_value_at(&self, x: f32) -> i32 {
        let track = self.layout.tcc_track;
        let max = self.tcc_max();
        if max <= 0 {
            return 0;
        }
        let radius = 9.0;
        let usable = (track.w - radius * 2.0).max(1.0);
        let ratio = ((x - track.x - radius) / usable).clamp(0.0, 1.0);
        (ratio * max as f32).round() as i32
    }

    fn on_mouse_move(&mut self, x: f32, y: f32) {
        if self.dragging {
            let value = self.slider_value_at(x);
            if value != self.tcc_pending {
                self.tcc_pending = value;
                self.invalidate();
            }
            return;
        }
        let hit = self.hit_at(x, y);
        if hit != self.hover {
            self.hover = hit;
            self.invalidate();
        }
    }

    fn on_mouse_down(&mut self, x: f32, y: f32) {
        let hit = self.hit_at(x, y);
        self.pressed = hit;
        if hit == Hit::TccTrack {
            self.dragging = true;
            unsafe {
                SetCapture(self.hwnd);
            }
            let value = self.slider_value_at(x);
            self.tcc_pending = value;
            self.invalidate();
        } else if hit != Hit::None {
            self.invalidate();
        }
    }

    fn on_mouse_up(&mut self, x: f32, y: f32) {
        let hit = self.hit_at(x, y);
        let pressed = self.pressed;
        self.pressed = Hit::None;
        self.dragging = false;
        unsafe {
            ReleaseCapture();
        }
        if pressed != Hit::None && pressed == hit {
            self.activate(hit);
        }
        self.invalidate();
    }

    fn activate(&mut self, hit: Hit) {
        match hit {
            Hit::Profile(index) => self.apply_profile(index),
            Hit::Stealth => self.toggle_stealth(),
            Hit::TccApply => self.apply_tcc(),
            Hit::Refresh => self.refresh(),
            Hit::Logs => self.open_logs(),
            Hit::Elevate => self.elevate(),
            Hit::TccTrack | Hit::None => {}
        }
    }

    fn apply_profile(&mut self, index: usize) {
        let Some(profile) = PROFILES.get(index) else {
            return;
        };
        log::info(format_args!(
            "profile selected: {} (0x{:02X})",
            profile.label, profile.id
        ));
        if self.stealth_on {
            self.notice = None;
            self.error = Some("Stealth is on. Turn it off before changing the profile.".into());
            self.invalidate();
            return;
        }
        if !self.can_write() {
            self.error = Some("Administrator access is required to change the profile.".into());
            self.invalidate();
            return;
        }
        let result = self.awcc.as_ref().map(|awcc| awcc.set_profile(profile));
        match result {
            Some(Ok(id)) => {
                log::info(format_args!("profile changed: 0x{id:02X} ({})", profile.label));
                if profile.id != PROFILE_QUIET {
                    self.stealth_previous = profile.id;
                }
                self.error = None;
                self.notice = Some(format!("{} profile applied", profile.label));
                self.refresh();
            }
            Some(Err(error)) => {
                log::error(format_args!(
                    "profile {} (0x{:02X}) failed: {error:#}",
                    profile.label, profile.id
                ));
                self.notice = None;
                self.error = Some(format!("{} failed: {error:#}", profile.label));
                self.invalidate();
            }
            None => {
                self.error = Some("The AWCC WMI interface is not available.".into());
                self.invalidate();
            }
        }
    }

    fn toggle_stealth(&mut self) {
        if !self.can_write() {
            self.error = Some("Administrator access is required to toggle Stealth.".into());
            self.invalidate();
            return;
        }
        let was_on = self.stealth_on;
        let current = self
            .status
            .as_ref()
            .map(|status| status.current_profile)
            .unwrap_or(PROFILE_PERFORMANCE);
        let supported = self
            .status
            .as_ref()
            .map(|status| status.device.profile_ids.clone())
            .unwrap_or_default();

        let previous = if current != PROFILE_QUIET {
            current
        } else if self.stealth_previous != PROFILE_QUIET {
            self.stealth_previous
        } else if supported.contains(&PROFILE_PERFORMANCE) {
            PROFILE_PERFORMANCE
        } else {
            supported.first().copied().unwrap_or(PROFILE_PERFORMANCE)
        };

        let target = if was_on {
            if supported.contains(&previous) {
                previous
            } else if supported.contains(&PROFILE_PERFORMANCE) {
                PROFILE_PERFORMANCE
            } else {
                supported.first().copied().unwrap_or(PROFILE_PERFORMANCE)
            }
        } else {
            PROFILE_QUIET
        };

        let result = self.awcc.as_ref().map(|awcc| awcc.set_profile_id(target));
        match result {
            Some(Ok(id)) => {
                if !was_on {
                    self.stealth_previous = previous;
                }
                self.stealth_on = !was_on;
                self.error = None;
                self.notice = Some(if self.stealth_on {
                    "Stealth on: Quiet profile".to_string()
                } else {
                    format!("Stealth off: 0x{id:02X}")
                });
                log::info(format_args!(
                    "stealth applied: on={} profile=0x{id:02X}",
                    self.stealth_on
                ));
                self.refresh();
            }
            Some(Err(error)) => {
                log::error(format_args!("stealth target 0x{target:02X} failed: {error:#}"));
                self.notice = None;
                self.error = Some(format!("Stealth toggle failed: {error:#}"));
                self.invalidate();
            }
            None => {
                self.error = Some("The AWCC WMI interface is not available.".into());
                self.invalidate();
            }
        }
    }

    fn apply_tcc(&mut self) {
        if !self.can_write() {
            self.error = Some("Administrator access is required to change the CPU limit.".into());
            self.invalidate();
            return;
        }
        let max = self.tcc_max();
        if max <= 0 {
            self.error = Some("This firmware does not expose a settable CPU limit.".into());
            self.invalidate();
            return;
        }
        let offset = self.tcc_value();
        log::info(format_args!("tcc apply requested: offset={offset} max={max}"));
        let result = self.awcc.as_ref().map(|awcc| awcc.set_tcc_offset(offset));
        match result {
            Some(Ok(value)) => {
                log::info(format_args!("tcc offset applied: {value}"));
                self.error = None;
                self.notice = Some(format!("CPU thermal limit offset set to {value} C"));
                self.refresh();
            }
            Some(Err(error)) => {
                log::error(format_args!("tcc offset failed: {error:#}"));
                self.notice = None;
                self.error = Some(format!("CPU limit change failed: {error:#}"));
                self.invalidate();
            }
            None => {
                self.error = Some("The AWCC WMI interface is not available.".into());
                self.invalidate();
            }
        }
    }

    fn open_logs(&self) {
        let path = log::path();
        log::info(format_args!("open logs requested: {path}"));
        if !std::path::Path::new(&path).exists() {
            self.show_error("The log file has not been created yet.");
            return;
        }
        let editor = ui::wide("notepad.exe");
        let file = ui::wide(&path);
        let result = unsafe {
            ShellExecuteW(
                self.hwnd,
                null(),
                editor.as_ptr(),
                file.as_ptr(),
                null(),
                SW_SHOWNORMAL,
            )
        };
        if result as isize <= 32 {
            self.show_error("Could not open the log file.");
        }
    }

    fn elevate(&self) {
        log::info(format_args!("elevate requested"));
        let mut path = [0u16; 1024];
        let length = unsafe { GetModuleFileNameW(null_mut(), path.as_mut_ptr(), path.len() as u32) };
        if length == 0 || length as usize >= path.len() {
            self.show_error("Could not determine the executable path.");
            return;
        }
        path[length as usize] = 0;
        let operation = ui::wide("runas");
        let result = unsafe {
            ShellExecuteW(
                self.hwnd,
                operation.as_ptr(),
                path.as_ptr(),
                null(),
                null(),
                SW_SHOWNORMAL,
            )
        };
        if result as isize <= 32 {
            self.show_error("Could not restart elevated.");
            return;
        }
        log::info(format_args!("elevate launched new process; closing"));
        unsafe {
            PostMessageW(self.hwnd, WM_CLOSE, 0, 0);
        }
    }

    fn show_error(&self, message: &str) {
        log::error(format_args!("dialog: {message}"));
        let message = ui::wide(message);
        let title = ui::wide(APP_TITLE);
        unsafe {
            MessageBoxW(
                self.hwnd,
                message.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }

    fn ensure_buffer(&mut self, width: i32, height: i32) {
        if !self.buffer_dc.is_null() && self.buffer_w == width && self.buffer_h == height {
            return;
        }
        unsafe {
            if !self.buffer_dc.is_null() {
                SelectObject(self.buffer_dc, self.buffer_old);
                DeleteObject(self.buffer_bitmap as HGDIOBJ);
                DeleteDC(self.buffer_dc);
            }
            let screen = GetDC(self.hwnd);
            self.buffer_dc = CreateCompatibleDC(screen);
            self.buffer_bitmap = CreateCompatibleBitmap(screen, width, height);
            self.buffer_old = SelectObject(self.buffer_dc, self.buffer_bitmap);
            ReleaseDC(self.hwnd, screen);
        }
        self.buffer_w = width;
        self.buffer_h = height;
        self.painter = Painter::new(self.buffer_dc, self.scale);
        if self.painter.is_none() {
            log::error(format_args!("GDI+ painter could not be created"));
        }
    }

    fn paint(&mut self, hdc: HDC) {
        unsafe {
            let mut rect: RECT = std::mem::zeroed();
            GetClientRect(self.hwnd, &mut rect);
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            self.ensure_buffer(width, height);
            if self.buffer_dc.is_null() {
                return;
            }
            let Some(painter) = self.painter.as_ref() else {
                log::error(format_args!("paint: no painter"));
                return;
            };
            log::debug(format_args!("paint: {width}x{height}"));
            self.draw(painter);
            BitBlt(hdc, 0, 0, width, height, self.buffer_dc, 0, 0, SRCCOPY);
            log::debug(format_args!("paint: done"));
        }
    }

    fn draw(&self, painter: &Painter) {
        log::debug(format_args!("draw: start"));
        painter.fill_rect(Rect::new(0.0, 0.0, UI_WIDTH, UI_HEIGHT), theme::BG);

        self.draw_header(painter);
        self.draw_gauge_card(
            painter,
            self.layout.cpu,
            "CPU",
            "0x01",
            self.temp_of(0x01),
            self.loads.cpu,
        );
        self.draw_gauge_card(
            painter,
            self.layout.gpu,
            "GPU",
            "0x06",
            self.temp_of(0x06),
            self.loads.gpu,
        );
        log::debug(format_args!("draw: gauges"));
        self.draw_fans(painter);
        self.draw_profiles(painter);
        self.draw_tcc(painter);
        self.draw_footer(painter);
        log::debug(format_args!("draw: done"));
    }

    fn temp_of(&self, id: u8) -> Option<u8> {
        self.status
            .as_ref()?
            .temps
            .iter()
            .find(|temp| temp.id == id)
            .and_then(|temp| temp.celsius)
    }

    fn fan_of(&self, id: u8) -> Option<&awcc::FanStatus> {
        self.status
            .as_ref()?
            .fans
            .iter()
            .find(|fan| fan.id == id)
    }

    fn draw_header(&self, painter: &Painter) {
        painter.text(
            "AWCC Lite",
            Rect::new(PAD, 20.0, 400.0, 34.0),
            FontKind::Title,
            theme::TEXT,
            Align::Left,
        );
        painter.text(
            "Thermal and fan control for Alienware",
            Rect::new(PAD, 54.0, 400.0, 20.0),
            FontKind::Body,
            theme::TEXT_FAINT,
            Align::Left,
        );

        let (label, color) = if self.admin {
            ("Administrator", theme::GOOD)
        } else {
            ("Read-only", theme::WARN)
        };
        let pill_right = if self.admin {
            UI_WIDTH - PAD
        } else {
            UI_WIDTH - PAD - 102.0
        };
        let pill = Rect::new(pill_right - 132.0, 26.0, 132.0, 30.0);
        painter.fill_round_rect(pill, 15.0, mix(theme::CARD, color, 0.12));
        painter.stroke_round_rect(pill, 15.0, mix(theme::BORDER, color, 0.45), 1.0);
        painter.fill_circle((pill.x + 18.0, pill.y + 15.0), 4.0, color);
        painter.text(
            label,
            Rect::new(pill.x + 30.0, pill.y + 7.0, pill.w - 40.0, 18.0),
            FontKind::Label,
            theme::TEXT,
            Align::Left,
        );
        if !self.admin {
            self.draw_button(painter, self.layout.elevate, "Elevate", Hit::Elevate, true);
        }
    }

    fn draw_gauge_card(
        &self,
        painter: &Painter,
        card: Rect,
        title: &str,
        sensor: &str,
        temperature: Option<u8>,
        load: Option<u8>,
    ) {
        painter.fill_round_rect(card, 16.0, theme::CARD);
        painter.stroke_round_rect(card, 16.0, theme::BORDER, 1.0);

        painter.text(
            title,
            Rect::new(card.x + 22.0, card.y + 16.0, 120.0, 20.0),
            FontKind::Label,
            theme::TEXT,
            Align::Left,
        );
        painter.text(
            sensor,
            Rect::new(card.x + card.w - 82.0, card.y + 18.0, 60.0, 18.0),
            FontKind::Tiny,
            theme::TEXT_FAINT,
            Align::Right,
        );

        let center = (card.x + card.w / 2.0, card.y + 88.0);
        let radius = 54.0;
        let arc_rect = Rect::new(
            center.0 - radius,
            center.1 - radius,
            radius * 2.0,
            radius * 2.0,
        );
        painter.arc(arc_rect, 135.0, 270.0, theme::TRACK, 10.0);

        let value = temperature.unwrap_or(0);
        let ratio = (value as f32 / 100.0).clamp(0.0, 1.0);
        let color = temp_color(value);
        if temperature.is_some() && ratio > 0.0 {
            painter.arc(arc_rect, 135.0, 270.0 * ratio, color, 10.0);
        }
        painter.fill_circle(center, radius - 12.0, theme::CARD);

        painter.text(
            &match temperature {
                Some(value) => value.to_string(),
                None => "--".to_string(),
            },
            Rect::new(center.0 - 60.0, center.1 - 30.0, 120.0, 50.0),
            FontKind::Display,
            match temperature {
                Some(_) => theme::TEXT,
                None => theme::TEXT_FAINT,
            },
            Align::Center,
        );
        painter.text(
            "C",
            Rect::new(center.0 - 60.0, center.1 + 20.0, 120.0, 18.0),
            FontKind::Tiny,
            theme::TEXT_FAINT,
            Align::Center,
        );

        painter.text(
            "LOAD",
            Rect::new(card.x + 22.0, card.y + 140.0, 120.0, 16.0),
            FontKind::Section,
            theme::TEXT_FAINT,
            Align::Left,
        );
        painter.text(
            &match load {
                Some(value) => format!("{value}%"),
                None => "--".to_string(),
            },
            Rect::new(card.x + card.w - 122.0, card.y + 132.0, 100.0, 26.0),
            FontKind::Value,
            theme::ACCENT,
            Align::Right,
        );
        let bar = Rect::new(card.x + 22.0, card.y + 162.0, card.w - 44.0, 6.0);
        painter.fill_round_rect(bar, 3.0, theme::TRACK);
        if let Some(value) = load {
            let width = bar.w * (value as f32 / 100.0);
            painter.fill_round_rect(
                Rect::new(bar.x, bar.y, width.max(6.0), bar.h),
                3.0,
                theme::ACCENT,
            );
        }
    }

    fn draw_fans(&self, painter: &Painter) {
        let card = self.layout.fans;
        painter.fill_round_rect(card, 16.0, theme::CARD);
        painter.stroke_round_rect(card, 16.0, theme::BORDER, 1.0);
        painter.text(
            "FANS",
            Rect::new(card.x + 20.0, card.y + 14.0, 200.0, 18.0),
            FontKind::Section,
            theme::TEXT_FAINT,
            Align::Left,
        );

        let column_w = (card.w - 40.0 - 24.0) / 2.0;
        let entries = [("CPU", FAN_CPU), ("GPU", FAN_GPU)];
        for (index, (label, id)) in entries.iter().enumerate() {
            let x = card.x + 20.0 + index as f32 * (column_w + 24.0);
            let fan = self.fan_of(*id);
            painter.text(
                label,
                Rect::new(x, card.y + 36.0, 120.0, 18.0),
                FontKind::Label,
                theme::TEXT,
                Align::Left,
            );
            let rpm = fan.and_then(|fan| fan.rpm);
            let value = match rpm {
                Some(value) => value.to_string(),
                None => "--".to_string(),
            };
            painter.text(
                &value,
                Rect::new(x, card.y + 54.0, 160.0, 30.0),
                FontKind::Value,
                theme::TEXT,
                Align::Left,
            );
            let (text_width, _) = painter.measure(&value, FontKind::Value);
            painter.text(
                "rpm",
                Rect::new(x + text_width + 8.0, card.y + 64.0, 60.0, 18.0),
                FontKind::Tiny,
                theme::TEXT_FAINT,
                Align::Left,
            );

            let max = fan.and_then(|fan| fan.max_rpm).unwrap_or(0);
            let bar = Rect::new(x, card.y + 88.0, column_w - 40.0, 6.0);
            painter.fill_round_rect(bar, 3.0, theme::TRACK);
            if let (Some(value), true) = (rpm, max > 0) {
                let width = bar.w * (value as f32 / max as f32).clamp(0.0, 1.0);
                painter.fill_round_rect(
                    Rect::new(bar.x, bar.y, width.max(4.0), bar.h),
                    3.0,
                    theme::ACCENT,
                );
            }
            let range = match (fan.and_then(|fan| fan.min_rpm), fan.and_then(|fan| fan.max_rpm))
            {
                (Some(min), Some(max)) => format!("{min} - {max} rpm"),
                _ => String::new(),
            };
            painter.text(
                &range,
                Rect::new(x, card.y + 98.0, column_w, 16.0),
                FontKind::Tiny,
                theme::TEXT_FAINT,
                Align::Left,
            );
        }
    }

    fn draw_profiles(&self, painter: &Painter) {
        let card = self.layout.profile;
        painter.fill_round_rect(card, 16.0, theme::CARD);
        painter.stroke_round_rect(card, 16.0, theme::BORDER, 1.0);
        painter.text(
            "THERMAL PROFILE",
            Rect::new(card.x + 20.0, card.y + 20.0, 260.0, 18.0),
            FontKind::Section,
            theme::TEXT_FAINT,
            Align::Left,
        );

        let current = self
            .status
            .as_ref()
            .map(|status| status.current_profile)
            .unwrap_or(0xFF);
        let supported = self
            .status
            .as_ref()
            .map(|status| status.device.profile_ids.clone())
            .unwrap_or_default();
        let locked = self.stealth_on || !self.can_write();

        for (index, profile) in PROFILES.iter().enumerate() {
            let tile = self.layout.tiles[index];
            let active = profile.id == current;
            let offered = profile.id == PROFILE_CUSTOM || supported.contains(&profile.id);
            let hovered = self.hover == Hit::Profile(index) || self.pressed == Hit::Profile(index);

            let background = if active {
                theme::CARD_ACTIVE
            } else if hovered && offered {
                theme::CARD_HOVER
            } else {
                theme::CARD
            };
            painter.fill_round_rect(tile, 12.0, background);
            if active {
                painter.stroke_round_rect(tile, 12.0, theme::ACCENT, 1.5);
            } else if hovered && offered {
                painter.stroke_round_rect(tile, 12.0, theme::BORDER_HOVER, 1.0);
            } else {
                painter.stroke_round_rect(tile, 12.0, theme::BORDER, 1.0);
            }

            let name_color = if !offered {
                theme::TEXT_FAINT
            } else if locked && !active {
                theme::TEXT_DIM
            } else if active {
                theme::ACCENT
            } else {
                theme::TEXT
            };
            let hint_color = if locked && !active {
                mix(theme::TEXT_FAINT, theme::BG, 0.4)
            } else {
                theme::TEXT_FAINT
            };
            painter.text(
                profile.label,
                Rect::new(tile.x + 16.0, tile.y + 12.0, tile.w - 32.0, 20.0),
                FontKind::Label,
                name_color,
                Align::Left,
            );
            painter.text(
                MODE_HINTS[index],
                Rect::new(tile.x + 16.0, tile.y + 34.0, tile.w - 32.0, 18.0),
                FontKind::Tiny,
                hint_color,
                Align::Left,
            );
            if active {
                painter.fill_circle((tile.x + tile.w - 18.0, tile.y + 20.0), 4.0, theme::ACCENT);
            }
        }

        let switch = self.layout.stealth;
        let on = self.stealth_on;
        painter.fill_round_rect(
            switch,
            14.0,
            if on {
                theme::ACCENT_DIM
            } else {
                theme::TRACK
            },
        );
        let knob_x = if on {
            switch.x + switch.w - 14.0
        } else {
            switch.x + 14.0
        };
        painter.fill_circle((knob_x, switch.y + switch.h / 2.0), 9.0, theme::TEXT);
        painter.text(
            "STEALTH",
            Rect::new(
                switch.x - 96.0,
                switch.y + 6.0,
                84.0,
                18.0,
            ),
            FontKind::Section,
            if on { theme::ACCENT } else { theme::TEXT_FAINT },
            Align::Right,
        );
    }

    fn draw_tcc(&self, painter: &Painter) {
        let card = self.layout.tcc;
        painter.fill_round_rect(card, 16.0, theme::CARD);
        painter.stroke_round_rect(card, 16.0, theme::BORDER, 1.0);
        painter.text(
            "CPU THERMAL LIMIT (TCC)",
            Rect::new(card.x + 20.0, card.y + 16.0, 300.0, 18.0),
            FontKind::Section,
            theme::TEXT_FAINT,
            Align::Left,
        );

        let max = self.tcc_max();
        let current = self.tcc_current();
        let pending = self.tcc_value();

        if self.tcc.is_some() {
            painter.text(
                &format!("+{pending} C"),
                Rect::new(card.x + card.w - 220.0, card.y + 12.0, 200.0, 30.0),
                FontKind::Value,
                if pending == current {
                    theme::TEXT
                } else {
                    theme::ACCENT
                },
                Align::Right,
            );
            let limit = self
                .tcc
                .as_ref()
                .and_then(|tcc| tcc.max_temperature)
                .unwrap_or(0);
            painter.text(
                &format!("CPU limit {limit} C  -  device {current} C"),
                Rect::new(card.x + 20.0, card.y + 38.0, 300.0, 18.0),
                FontKind::Tiny,
                theme::TEXT_FAINT,
                Align::Left,
            );
        } else {
            painter.text(
                if self.tcc_failed {
                    "not exposed by this firmware"
                } else {
                    "reading..."
                },
                Rect::new(card.x + card.w - 320.0, card.y + 18.0, 300.0, 20.0),
                FontKind::Tiny,
                theme::TEXT_FAINT,
                Align::Right,
            );
        }

        let track = self.layout.tcc_track;
        painter.fill_round_rect(track, 4.0, theme::TRACK);
        if max > 0 {
            let usable = track.w - 18.0;
            let ratio = (pending as f32 / max as f32).clamp(0.0, 1.0);
            let filled = 18.0 + usable * ratio;
            painter.fill_round_rect(
                Rect::new(track.x, track.y, filled, track.h),
                4.0,
                theme::ACCENT,
            );
            let knob_x = track.x + 9.0 + usable * ratio;
            painter.fill_circle((knob_x, track.y + track.h / 2.0), 9.0, theme::TEXT);
        }

        let apply = self.layout.tcc_apply;
        let changed = pending != current && max > 0;
        let enabled = self.can_write() && max > 0 && changed;
        let hovered = self.hover == Hit::TccApply || self.pressed == Hit::TccApply;
        painter.fill_round_rect(
            apply,
            10.0,
            if enabled {
                if hovered {
                    mix(theme::ACCENT, theme::TEXT, 0.15)
                } else {
                    theme::ACCENT
                }
            } else {
                theme::TRACK
            },
        );
        painter.text(
            "Apply",
            apply,
            FontKind::Label,
            if enabled {
                theme::BG
            } else {
                theme::TEXT_FAINT
            },
            Align::Center,
        );

        painter.text(
            "Warning: AWCC background service overwrites this offset on profile switch.",
            Rect::new(card.x + 20.0, card.y + 88.0, card.w - 40.0, 18.0),
            FontKind::Tiny,
            theme::WARN,
            Align::Left,
        );
    }

    fn draw_footer(&self, painter: &Painter) {
        let footer = self.layout.footer;
        let interface = self
            .awcc
            .as_ref()
            .map(|awcc| awcc.instance_name.as_str())
            .unwrap_or("no interface");

        let mut now: SYSTEMTIME = unsafe { std::mem::zeroed() };
        unsafe {
            GetLocalTime(&mut now);
        }
        let clock = format!(
            "{:02}:{:02}:{:02}",
            now.wHour, now.wMinute, now.wSecond
        );

        let (message, color) = if let Some(error) = &self.error {
            (error.clone(), theme::BAD)
        } else if let Some(notice) = &self.notice {
            (notice.clone(), theme::GOOD)
        } else {
            (format!("{interface}  -  updated {clock}"), theme::TEXT_FAINT)
        };
        painter.text(
            &message,
            Rect::new(footer.x, footer.y + 8.0, 420.0, 20.0),
            FontKind::Tiny,
            color,
            Align::Left,
        );

        self.draw_button(painter, self.layout.refresh, "Refresh", Hit::Refresh, true);
        self.draw_button(painter, self.layout.logs, "Open logs", Hit::Logs, true);
    }

    fn draw_button(
        &self,
        painter: &Painter,
        rect: Rect,
        label: &str,
        hit: Hit,
        enabled: bool,
    ) {
        let hovered = self.hover == hit || self.pressed == hit;
        painter.fill_round_rect(
            rect,
            10.0,
            if enabled && hovered {
                theme::CARD_HOVER
            } else {
                theme::CARD
            },
        );
        painter.stroke_round_rect(
            rect,
            10.0,
            if enabled && hovered {
                theme::BORDER_HOVER
            } else {
                theme::BORDER
            },
            1.0,
        );
        painter.text(
            label,
            rect,
            FontKind::Label,
            if enabled {
                theme::TEXT_DIM
            } else {
                theme::TEXT_FAINT
            },
            Align::Center,
        );
    }
}

fn temp_color(value: u8) -> u32 {
    let value = value as f32;
    if value <= 60.0 {
        theme::ACCENT
    } else if value <= 80.0 {
        mix(theme::ACCENT, theme::WARN, (value - 60.0) / 20.0)
    } else {
        mix(theme::WARN, theme::BAD, ((value - 80.0) / 20.0).min(1.0))
    }
}

fn status_summary(status: &Status) -> String {
    let temps = status
        .temps
        .iter()
        .map(|temp| match temp.celsius {
            Some(value) => format!("{} {value}C", temp.label),
            None => format!("{} --C", temp.label),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let fans = status
        .fans
        .iter()
        .map(|fan| {
            let rpm = fan
                .rpm
                .map(|value| format!("{value}rpm"))
                .unwrap_or_else(|| "--rpm".to_string());
            format!("{} {rpm}", fan.label)
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "profile=0x{:02X} ({}) balanced=0x{:02X} ({}) temps=[{temps}] fans=[{fans}]",
        status.current_profile,
        status.current_profile_name,
        status.balanced_profile,
        status.balanced_profile_name
    )
}

unsafe fn app_ptr(hwnd: HWND) -> *mut App {
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App }
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match message {
            WM_NCCREATE => {
                let create = &*(lparam as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            WM_CREATE => {
                let app = app_ptr(hwnd);
                if !app.is_null() {
                    let dpi = GetDpiForSystem();
                    let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
                    (*app).init(hwnd, scale);
                }
                0
            }
            WM_ERASEBKGND => 1,
            WM_PAINT => {
                let app = app_ptr(hwnd);
                if !app.is_null() {
                    let mut ps: PAINTSTRUCT = std::mem::zeroed();
                    let hdc = BeginPaint(hwnd, &mut ps);
                    (*app).paint(hdc);
                    EndPaint(hwnd, &ps);
                }
                0
            }
            WM_TIMER => {
                if wparam == TIMER_ID {
                    let app = app_ptr(hwnd);
                    if !app.is_null() {
                        (*app).refresh();
                    }
                }
                0
            }
            WM_MOUSEMOVE => {
                let app = app_ptr(hwnd);
                if !app.is_null() {
                    let x = (lparam & 0xFFFF) as f32 / (*app).scale;
                    let y = ((lparam >> 16) & 0xFFFF) as f32 / (*app).scale;
                    (*app).on_mouse_move(x, y);
                }
                0
            }
            WM_LBUTTONDOWN => {
                let app = app_ptr(hwnd);
                if !app.is_null() {
                    let x = (lparam & 0xFFFF) as f32 / (*app).scale;
                    let y = ((lparam >> 16) & 0xFFFF) as f32 / (*app).scale;
                    (*app).on_mouse_down(x, y);
                }
                0
            }
            WM_LBUTTONUP => {
                let app = app_ptr(hwnd);
                if !app.is_null() {
                    let x = (lparam & 0xFFFF) as f32 / (*app).scale;
                    let y = ((lparam >> 16) & 0xFFFF) as f32 / (*app).scale;
                    (*app).on_mouse_up(x, y);
                }
                0
            }
            WM_CAPTURECHANGED => {
                let app = app_ptr(hwnd);
                if !app.is_null() {
                    (*app).dragging = false;
                }
                0
            }
            WM_CLOSE => {
                log::info(format_args!("WM_CLOSE"));
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                log::info(format_args!("WM_DESTROY"));
                KillTimer(hwnd, TIMER_ID);
                PostQuitMessage(0);
                0
            }
            WM_NCDESTROY => {
                let app = app_ptr(hwnd);
                if !app.is_null() {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    let app = Box::from_raw(app);
                    if !app.buffer_dc.is_null() {
                        SelectObject(app.buffer_dc, app.buffer_old);
                        DeleteObject(app.buffer_bitmap as HGDIOBJ);
                        DeleteDC(app.buffer_dc);
                    }
                }
                DefWindowProcW(hwnd, message, wparam, lparam)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }
}

struct ComGuard;

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

fn run() -> Result<()> {
    log::info(format_args!("run: creating window"));
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let com = unsafe { CoInitializeEx(null(), COINIT_MULTITHREADED as u32) };
    log::info(format_args!("CoInitializeEx(MTA) -> 0x{com:08X}"));
    if com < 0 && com != RPC_E_CHANGED_MODE {
        bail!("CoInitializeEx failed: 0x{com:08X}");
    }
    let _com = (com >= 0).then_some(ComGuard);

    let Some(_gdiplus) = GdiPlusGuard::start() else {
        bail!("GDI+ could not be started");
    };

    let instance = unsafe { GetModuleHandleW(null()) };
    let class_name = ui::wide(CLASS_NAME);
    let title = ui::wide(APP_TITLE);
    let cursor = unsafe { LoadCursorW(null_mut(), IDC_ARROW) };
    let icon = unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };

    let window_class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: icon,
        hCursor: cursor,
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: class_name.as_ptr(),
    };

    if unsafe { RegisterClassW(&window_class) } == 0 {
        bail!("RegisterClassW failed: {}", std::io::Error::last_os_error());
    }

    let style = WS_OVERLAPPEDWINDOW & !WS_THICKFRAME & !WS_MAXIMIZEBOX;
    let app = Box::into_raw(Box::new(App::new()));
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            720,
            860,
            null_mut(),
            null_mut(),
            instance,
            app as *const c_void,
        )
    };

    if hwnd.is_null() {
        unsafe {
            drop(Box::from_raw(app));
        }
        bail!(
            "CreateWindowExW failed: {}",
            std::io::Error::last_os_error()
        );
    }

    unsafe {
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);

        let mut message: MSG = std::mem::zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    Ok(())
}

fn relaunch_elevated() -> Result<()> {
    let mut path = [0u16; 1024];
    let length = unsafe { GetModuleFileNameW(null_mut(), path.as_mut_ptr(), path.len() as u32) };
    if length == 0 || length as usize >= path.len() {
        bail!("could not determine the executable path");
    }
    path[length as usize] = 0;

    let operation = ui::wide("runas");
    let result = unsafe {
        ShellExecuteW(
            null_mut(),
            operation.as_ptr(),
            path.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize <= 32 {
        bail!(
            "administrator rights are required; elevation was cancelled ({})",
            std::io::Error::last_os_error()
        );
    }
    Ok(())
}

fn main() {
    log::init();
    std::panic::set_hook(Box::new(|info| {
        log::error(format_args!("panic: {info}"));
    }));
    let admin = unsafe { IsUserAnAdmin() } != 0;
    let no_elevate = std::env::var_os("AWCC_NO_ELEVATE").is_some();
    log::info(format_args!(
        "main: admin={admin} AWCC_NO_ELEVATE={no_elevate}"
    ));

    if !admin && !no_elevate {
        match relaunch_elevated() {
            Ok(()) => {
                log::info(format_args!("elevation launched; exiting original process"));
                return;
            }
            Err(error) => {
                log::warn(format_args!("elevation unavailable: {error:#}"));
            }
        }
    }

    if let Err(error) = run() {
        log::error(format_args!("run failed: {error:#}"));
        let message = ui::wide(&format!("{error:#}"));
        let title = ui::wide(APP_TITLE);
        unsafe {
            MessageBoxW(
                null_mut(),
                message.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}
