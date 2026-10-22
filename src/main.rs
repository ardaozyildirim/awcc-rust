#![cfg_attr(not(test), windows_subsystem = "windows")]

mod awcc;
mod load;
mod log;
mod ui;

use anyhow::{Context, Result, bail};
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
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_NOREPEAT, RegisterHotKey, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT,
    TrackMouseEvent, UnregisterHotKey, VK_F1, VK_F2, VK_F3, VK_F6, VK_F7,
};
use windows_sys::Win32::UI::Shell::{IsUserAnAdmin, ShellExecuteW};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const CLASS_NAME: &str = "AwccLiteWindow";
const APP_TITLE: &str = "Alienware Command Center";
const TIMER_ID: usize = 1;
const POLL_MS: u32 = 2000;
const WM_MOUSELEAVE: u32 = 0x02A3;

const HOTKEY_F6: i32 = 1001;
const HOTKEY_F7: i32 = 1002;
const HOTKEY_M1: i32 = 1011;
const HOTKEY_M1_ALT: i32 = 1012;
const HOTKEY_M2: i32 = 1021;
const HOTKEY_M2_ALT: i32 = 1022;
const HOTKEY_M3: i32 = 1031;
const HOTKEY_M3_ALT: i32 = 1032;

const UI_WIDTH: f32 = 700.0;
const UI_HEIGHT: f32 = 836.0;

const PAD: f32 = 24.0;

const MODE_HINTS: [&str; 6] = [
    "Battery Saver",
    "Whisper Silent",
    "Everyday Default",
    "Boosted Clocks",
    "Turbo Max Speed",
    "Custom Profile",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ActiveTab {
    Performance,
    Macros,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    None,
    TabPerf,
    TabMacros,
    Profile(usize),
    Stealth,
    TccTrack,
    TccApply,
    TriggerM1,
    TriggerM2,
    TriggerM3,
    SetM1(usize),
    SetM2(usize),
    SetM3(usize),
    Refresh,
    Logs,
    Elevate,
}

struct Layout {
    tab_perf: Rect,
    tab_macros: Rect,
    cpu: Rect,
    gpu: Rect,
    fans: Rect,
    profile: Rect,
    tcc: Rect,
    tiles: [Rect; 6],
    stealth: Rect,
    tcc_track: Rect,
    tcc_apply: Rect,
    m1_card: Rect,
    m1_btn: Rect,
    m1_opts: [Rect; 4],
    m2_card: Rect,
    m2_btn: Rect,
    m2_opts: [Rect; 4],
    m3_card: Rect,
    m3_btn: Rect,
    m3_opts: [Rect; 4],
    refresh: Rect,
    logs: Rect,
    elevate: Rect,
    footer: Rect,
}

fn build_layout() -> Layout {
    let card_w = UI_WIDTH - PAD * 2.0;
    let half = (card_w - 16.0) / 2.0;

    let y_tab = 66.0;
    let tab_bar_w = 440.0;
    let tab_bar_h = 36.0;
    let tab_w = (tab_bar_w - 6.0) / 2.0;
    let tab_h = tab_bar_h - 6.0;
    let tab_perf = Rect::new(PAD + 3.0, y_tab + 3.0, tab_w, tab_h);
    let tab_macros = Rect::new(PAD + 3.0 + tab_w, y_tab + 3.0, tab_w, tab_h);

    let y_cpu = 112.0;
    let h_cpu = 176.0;
    let y_fans = y_cpu + h_cpu + 12.0;
    let h_fans = 126.0;
    let y_profile = y_fans + h_fans + 12.0;
    let h_profile = 206.0;
    let y_tcc = y_profile + h_profile + 12.0;
    let h_tcc = 116.0;
    let y_footer = y_tcc + h_tcc + 14.0;

    let inner = card_w - 40.0;
    let tile_w = (inner - 24.0) / 3.0;
    let tile_h = 62.0;
    let mut tiles = [Rect::new(0.0, 0.0, 0.0, 0.0); 6];
    for (index, tile) in tiles.iter_mut().enumerate() {
        let column = index % 3;
        let row = index / 3;
        *tile = Rect::new(
            PAD + 20.0 + column as f32 * (tile_w + 12.0),
            y_profile + 52.0 + row as f32 * (tile_h + 10.0),
            tile_w,
            tile_h,
        );
    }

    // Alienware 16 Aurora AC16250 Dedicated Macro Cards (M1, M2, M3)
    let y_m1 = 110.0;
    let h_m = 202.0;
    let y_m2 = y_m1 + h_m + 14.0;
    let y_m3 = y_m2 + h_m + 14.0;

    let m1_card = Rect::new(PAD, y_m1, card_w, h_m);
    let m2_card = Rect::new(PAD, y_m2, card_w, h_m);
    let m3_card = Rect::new(PAD, y_m3, card_w, h_m);

    let m1_btn = Rect::new(PAD + card_w - 20.0 - 120.0, y_m1 + 20.0, 120.0, 34.0);
    let m2_btn = Rect::new(PAD + card_w - 20.0 - 120.0, y_m2 + 20.0, 120.0, 34.0);
    let m3_btn = Rect::new(PAD + card_w - 20.0 - 120.0, y_m3 + 20.0, 120.0, 34.0);

    let opt_w = (card_w - 40.0 - 30.0) / 4.0;
    let opt_h = 56.0;
    let mut m1_opts = [Rect::new(0.0, 0.0, 0.0, 0.0); 4];
    let mut m2_opts = [Rect::new(0.0, 0.0, 0.0, 0.0); 4];
    let mut m3_opts = [Rect::new(0.0, 0.0, 0.0, 0.0); 4];
    for i in 0..4 {
        let x = PAD + 20.0 + i as f32 * (opt_w + 10.0);
        m1_opts[i] = Rect::new(x, y_m1 + 124.0, opt_w, opt_h);
        m2_opts[i] = Rect::new(x, y_m2 + 124.0, opt_w, opt_h);
        m3_opts[i] = Rect::new(x, y_m3 + 124.0, opt_w, opt_h);
    }

    Layout {
        tab_perf,
        tab_macros,
        cpu: Rect::new(PAD, y_cpu, half, h_cpu),
        gpu: Rect::new(PAD + half + 16.0, y_cpu, half, h_cpu),
        fans: Rect::new(PAD, y_fans, card_w, h_fans),
        profile: Rect::new(PAD, y_profile, card_w, h_profile),
        tcc: Rect::new(PAD, y_tcc, card_w, h_tcc),
        tiles,
        stealth: Rect::new(PAD + card_w - 20.0 - 54.0, y_profile + 18.0, 54.0, 28.0),
        tcc_track: Rect::new(PAD + 20.0, y_tcc + 64.0, 392.0, 8.0),
        tcc_apply: Rect::new(PAD + card_w - 20.0 - 96.0, y_tcc + 50.0, 96.0, 34.0),
        m1_card,
        m1_btn,
        m1_opts,
        m2_card,
        m2_btn,
        m2_opts,
        m3_card,
        m3_btn,
        m3_opts,
        refresh: Rect::new(UI_WIDTH - PAD - 84.0, y_footer, 84.0, 32.0),
        logs: Rect::new(UI_WIDTH - PAD - 84.0 - 10.0 - 108.0, y_footer, 108.0, 32.0),
        elevate: Rect::new(UI_WIDTH - PAD - 92.0, 20.0, 92.0, 32.0),
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
    active_tab: ActiveTab,
    m1_action: usize,
    m2_action: usize,
    m3_action: usize,
    hover: Hit,
    pressed: Hit,
    dragging: bool,
    layout: Layout,
    buffer_dc: HDC,
    buffer_bitmap: HBITMAP,
    buffer_old: HGDIOBJ,
    buffer_w: i32,
    buffer_h: i32,
    painter: Option<Painter>,
}

impl App {
    fn new() -> Self {
        let admin = unsafe { IsUserAnAdmin() } != 0;
        Self {
            hwnd: null_mut(),
            scale: 1.0,
            admin,
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
            active_tab: ActiveTab::Performance,
            m1_action: 0,
            m2_action: 0,
            m3_action: 0,
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

            // Register Alienware keyboard hotkeys
            // F6: Thermal Profile Cycle
            // F7: Stealth Mode Toggle
            RegisterHotKey(hwnd, HOTKEY_F6, MOD_NOREPEAT, VK_F6 as u32);
            RegisterHotKey(hwnd, HOTKEY_F7, MOD_NOREPEAT, VK_F7 as u32);
        }

        self.update_macro_hotkeys();
        self.refresh();
    }

    fn update_macro_hotkeys(&self) {
        if self.hwnd.is_null() {
            return;
        }
        unsafe {
            // M1 (F1 / Alt+F1)
            UnregisterHotKey(self.hwnd, HOTKEY_M1);
            UnregisterHotKey(self.hwnd, HOTKEY_M1_ALT);
            if self.m1_action > 0 {
                RegisterHotKey(self.hwnd, HOTKEY_M1, MOD_NOREPEAT, VK_F1 as u32);
                RegisterHotKey(self.hwnd, HOTKEY_M1_ALT, MOD_ALT | MOD_NOREPEAT, VK_F1 as u32);
            }

            // M2 (F2 / Alt+F2)
            UnregisterHotKey(self.hwnd, HOTKEY_M2);
            UnregisterHotKey(self.hwnd, HOTKEY_M2_ALT);
            if self.m2_action > 0 {
                RegisterHotKey(self.hwnd, HOTKEY_M2, MOD_NOREPEAT, VK_F2 as u32);
                RegisterHotKey(self.hwnd, HOTKEY_M2_ALT, MOD_ALT | MOD_NOREPEAT, VK_F2 as u32);
            }

            // M3 (F3 / Alt+F3)
            UnregisterHotKey(self.hwnd, HOTKEY_M3);
            UnregisterHotKey(self.hwnd, HOTKEY_M3_ALT);
            if self.m3_action > 0 {
                RegisterHotKey(self.hwnd, HOTKEY_M3, MOD_NOREPEAT, VK_F3 as u32);
                RegisterHotKey(self.hwnd, HOTKEY_M3_ALT, MOD_ALT | MOD_NOREPEAT, VK_F3 as u32);
            }
        }
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
    }

    fn tcc_current(&self) -> i32 {
        self.tcc.as_ref().and_then(|tcc| tcc.offset).unwrap_or(0)
    }

    fn tcc_value(&self) -> i32 {
        if self.dragging {
            self.tcc_pending
        } else {
            self.tcc_current()
        }
    }

    fn hit_test(&self, x: f32, y: f32) -> Hit {
        let layout = &self.layout;
        if layout.tab_perf.contains(x, y) {
            return Hit::TabPerf;
        }
        if layout.tab_macros.contains(x, y) {
            return Hit::TabMacros;
        }
        if layout.refresh.contains(x, y) {
            return Hit::Refresh;
        }
        if layout.logs.contains(x, y) {
            return Hit::Logs;
        }
        if !self.admin && layout.elevate.contains(x, y) {
            return Hit::Elevate;
        }

        match self.active_tab {
            ActiveTab::Performance => {
                for (index, tile) in layout.tiles.iter().enumerate() {
                    if tile.contains(x, y) {
                        return Hit::Profile(index);
                    }
                }
                if layout.stealth.contains(x, y) {
                    return Hit::Stealth;
                }
                if layout.tcc_track.contains(x, y)
                    || (x >= layout.tcc_track.x && x <= layout.tcc_track.x + layout.tcc_track.w && (y - (layout.tcc_track.y + 4.0)).abs() <= 12.0)
                {
                    return Hit::TccTrack;
                }
                if layout.tcc_apply.contains(x, y) {
                    return Hit::TccApply;
                }
            }
            ActiveTab::Macros => {
                if layout.m1_btn.contains(x, y) {
                    return Hit::TriggerM1;
                }
                if layout.m2_btn.contains(x, y) {
                    return Hit::TriggerM2;
                }
                if layout.m3_btn.contains(x, y) {
                    return Hit::TriggerM3;
                }
                for (i, opt) in layout.m1_opts.iter().enumerate() {
                    if opt.contains(x, y) {
                        return Hit::SetM1(i);
                    }
                }
                for (i, opt) in layout.m2_opts.iter().enumerate() {
                    if opt.contains(x, y) {
                        return Hit::SetM2(i);
                    }
                }
                for (i, opt) in layout.m3_opts.iter().enumerate() {
                    if opt.contains(x, y) {
                        return Hit::SetM3(i);
                    }
                }
            }
        }

        Hit::None
    }

    fn on_mouse_move(&mut self, x: f32, y: f32) {
        if self.dragging {
            let track = self.layout.tcc_track;
            let ratio = ((x - track.x) / track.w).clamp(0.0, 1.0);
            let max = self.tcc_max();
            self.tcc_pending = (ratio * max as f32).round() as i32;
            self.invalidate();
            return;
        }

        let hit = self.hit_test(x, y);
        if hit != self.hover {
            self.hover = hit;
            self.invalidate();
        }
    }

    fn on_mouse_leave(&mut self) {
        if self.hover != Hit::None {
            self.hover = Hit::None;
            self.invalidate();
        }
    }

    fn on_mouse_down(&mut self, x: f32, y: f32) {
        let hit = self.hit_test(x, y);
        self.pressed = hit;

        if hit == Hit::TccTrack {
            self.dragging = true;
            unsafe {
                SetCapture(self.hwnd);
            }
            let track = self.layout.tcc_track;
            let ratio = ((x - track.x) / track.w).clamp(0.0, 1.0);
            let max = self.tcc_max();
            self.tcc_pending = (ratio * max as f32).round() as i32;
            self.invalidate();
            return;
        }

        self.invalidate();
    }

    fn on_mouse_up(&mut self, x: f32, y: f32) {
        if self.dragging {
            self.dragging = false;
            unsafe {
                ReleaseCapture();
            }
            self.pressed = Hit::None;
            self.invalidate();
            return;
        }

        let hit = self.hit_test(x, y);
        if hit == self.pressed && hit != Hit::None {
            self.activate(hit);
        }
        self.pressed = Hit::None;
        self.invalidate();
    }

    fn activate(&mut self, hit: Hit) {
        match hit {
            Hit::TabPerf => {
                self.active_tab = ActiveTab::Performance;
                self.invalidate();
            }
            Hit::TabMacros => {
                self.active_tab = ActiveTab::Macros;
                self.invalidate();
            }
            Hit::Profile(index) => self.apply_profile(index),
            Hit::Stealth => self.toggle_stealth(),
            Hit::TccApply => self.apply_tcc(),
            Hit::TriggerM1 => self.trigger_m1(),
            Hit::TriggerM2 => self.trigger_m2(),
            Hit::TriggerM3 => self.trigger_m3(),
            Hit::SetM1(idx) => {
                self.m1_action = idx;
                self.update_macro_hotkeys();
                self.notice = Some(if idx == 0 {
                    "M1 (F1) macro assignment removed (F1 is standard)".into()
                } else {
                    "M1 (F1) macro assigned & hotkey active".into()
                });
                self.invalidate();
            }
            Hit::SetM2(idx) => {
                self.m2_action = idx;
                self.update_macro_hotkeys();
                self.notice = Some(if idx == 0 {
                    "M2 (F2) macro assignment removed (F2 is standard)".into()
                } else {
                    "M2 (F2) macro assigned & hotkey active".into()
                });
                self.invalidate();
            }
            Hit::SetM3(idx) => {
                self.m3_action = idx;
                self.update_macro_hotkeys();
                self.notice = Some(if idx == 0 {
                    "M3 (F3) macro assignment removed (F3 is standard)".into()
                } else {
                    "M3 (F3) macro assigned & hotkey active".into()
                });
                self.invalidate();
            }
            Hit::Refresh => self.refresh(),
            Hit::Logs => self.open_logs(),
            Hit::Elevate => self.elevate(),
            Hit::None | Hit::TccTrack => {}
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
            .unwrap_or(0xFF);
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
            0xA0
        };

        let target = if was_on { previous } else { PROFILE_QUIET };
        log::info(format_args!(
            "stealth toggle: was_on={was_on} target=0x{target:02X}"
        ));

        let result = self
            .awcc
            .as_ref()
            .map(|awcc| awcc.set_profile_id(target));
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

    fn cycle_profile(&mut self) {
        if !self.can_write() {
            self.error = Some("Administrator access is required to cycle profiles.".into());
            self.invalidate();
            return;
        }
        if self.stealth_on {
            self.toggle_stealth();
            return;
        }
        let supported = self
            .status
            .as_ref()
            .map(|s| s.device.profile_ids.clone())
            .unwrap_or_default();
        if supported.is_empty() {
            return;
        }
        let current = self.status.as_ref().map(|s| s.current_profile).unwrap_or(0);
        let order = [
            awcc::PROFILE_QUIET,
            0xA0, // Balanced
            awcc::PROFILE_PERFORMANCE,
            0xA4, // Overdrive
            0xA2, // Battery
        ];
        let active_list: Vec<u8> = order.iter().copied().filter(|id| supported.contains(id)).collect();
        let list = if active_list.is_empty() { supported } else { active_list };
        let next_idx = match list.iter().position(|&id| id == current) {
            Some(pos) => (pos + 1) % list.len(),
            None => 0,
        };
        let target_id = list[next_idx];
        if let Some(pos) = PROFILES.iter().position(|p| p.id == target_id) {
            self.apply_profile(pos);
            self.notice = Some(format!("F6 Hotkey: Cycled to {} mode", PROFILES[pos].label));
        }
    }

    fn trigger_m1(&mut self) {
        match self.m1_action {
            0 => {
                self.notice = Some("M1 is unassigned (Standard F1)".into());
                self.invalidate();
            }
            1 => {
                if let Some(pos) = PROFILES.iter().position(|p| p.id == 0xA4) {
                    self.apply_profile(pos);
                    self.notice = Some("M1 (F1): Turbo Overdrive Profile Applied".into());
                }
            }
            2 => {
                let _ = self.awcc.as_ref().map(|awcc| awcc.set_tcc_offset(15));
                self.notice = Some("M1 (F1): Max Cooling (TCC 15°C limit) Applied".into());
                self.refresh();
            }
            3 => {
                let op = ui::wide("open");
                let target = ui::wide("taskmgr.exe");
                unsafe {
                    ShellExecuteW(null_mut(), op.as_ptr(), target.as_ptr(), null(), null(), SW_SHOWNORMAL);
                }
                self.notice = Some("M1 (F1): Launched Task Manager".into());
                self.invalidate();
            }
            _ => {}
        }
    }

    fn trigger_m2(&mut self) {
        match self.m2_action {
            0 => {
                self.notice = Some("M2 is unassigned (Standard F2)".into());
                self.invalidate();
            }
            1 => {
                if let Some(pos) = PROFILES.iter().position(|p| p.id == awcc::PROFILE_QUIET) {
                    self.apply_profile(pos);
                    self.notice = Some("M2 (F2): Whisper Quiet (Silent 0 dB) Applied".into());
                }
            }
            2 => {
                self.toggle_stealth();
                self.notice = Some("M2 (F2): Stealth Mode Toggled".into());
            }
            3 => {
                if let Some(pos) = PROFILES.iter().position(|p| p.id == 0xA2) {
                    self.apply_profile(pos);
                    self.notice = Some("M2 (F2): Eco Battery Profile Applied".into());
                }
            }
            _ => {}
        }
    }

    fn trigger_m3(&mut self) {
        match self.m3_action {
            0 => {
                self.notice = Some("M3 is unassigned (Standard F3)".into());
                self.invalidate();
            }
            1 => {
                if let Some(pos) = PROFILES.iter().position(|p| p.id == 0xA0) {
                    self.apply_profile(pos);
                    self.notice = Some("M3 (F3): Balanced Everyday Profile Applied".into());
                }
            }
            2 => {
                if let Some(pos) = PROFILES.iter().position(|p| p.id == awcc::PROFILE_PERFORMANCE) {
                    self.apply_profile(pos);
                    self.notice = Some("M3 (F3): Performance Boost Mode Applied".into());
                }
            }
            3 => {
                let _ = self.awcc.as_ref().map(|awcc| awcc.set_tcc_offset(0));
                self.notice = Some("M3 (F3): CPU Thermal Limit Reset to Factory (0°C)".into());
                self.refresh();
            }
            _ => {}
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
        let wide_path = ui::wide(&path);
        let operation = ui::wide("open");
        unsafe {
            ShellExecuteW(
                null_mut(),
                operation.as_ptr(),
                editor.as_ptr(),
                wide_path.as_ptr(),
                null(),
                SW_SHOWNORMAL,
            );
        }
    }

    fn elevate(&self) {
        log::info(format_args!("elevate requested from UI"));
        if let Err(error) = relaunch_elevated() {
            log::error(format_args!("elevation failed: {error:#}"));
            self.show_error(&format!("Elevation failed: {error:#}"));
        } else {
            unsafe {
                PostMessageW(self.hwnd, WM_CLOSE, 0, 0);
            }
        }
    }

    fn show_error(&self, message: &str) {
        let wide_msg = ui::wide(message);
        let title = ui::wide(APP_TITLE);
        unsafe {
            MessageBoxW(
                self.hwnd,
                wide_msg.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }

    fn invalidate(&self) {
        if !self.hwnd.is_null() {
            unsafe {
                InvalidateRect(self.hwnd, null(), 0);
            }
        }
    }

    fn paint(&mut self, hdc: HDC) {
        let mut client: RECT = unsafe { std::mem::zeroed() };
        unsafe {
            GetClientRect(self.hwnd, &mut client);
        }
        let width = client.right - client.left;
        let height = client.bottom - client.top;
        if width <= 0 || height <= 0 {
            return;
        }

        if self.buffer_dc.is_null() || self.buffer_w != width || self.buffer_h != height {
            unsafe {
                if !self.buffer_dc.is_null() {
                    SelectObject(self.buffer_dc, self.buffer_old);
                    DeleteObject(self.buffer_bitmap as HGDIOBJ);
                    DeleteDC(self.buffer_dc);
                }
                self.buffer_dc = CreateCompatibleDC(hdc);
                self.buffer_bitmap = CreateCompatibleBitmap(hdc, width, height);
                self.buffer_old = SelectObject(self.buffer_dc, self.buffer_bitmap as HGDIOBJ);
                self.buffer_w = width;
                self.buffer_h = height;
                self.painter = Painter::new(self.buffer_dc, self.scale);
            }
        }

        let Some(painter) = self.painter.as_ref() else {
            return;
        };

        let screen = Rect::new(0.0, 0.0, UI_WIDTH, UI_HEIGHT);
        painter.fill_rect(screen, theme::BG);

        self.draw_header(painter);
        self.draw_tabs(painter);

        match self.active_tab {
            ActiveTab::Performance => {
                self.draw_telemetry(painter);
                self.draw_fans(painter);
                self.draw_profiles(painter);
                self.draw_tcc(painter);
            }
            ActiveTab::Macros => {
                self.draw_macros_tab(painter);
            }
        }

        self.draw_footer(painter);

        unsafe {
            BitBlt(
                hdc,
                0,
                0,
                width,
                height,
                self.buffer_dc,
                0,
                0,
                SRCCOPY,
            );
        }
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
            "ALIENWARE 16 AURORA AC16250",
            Rect::new(PAD, 16.0, 320.0, 16.0),
            FontKind::Tiny,
            theme::ACCENT,
            Align::Left,
        );
        painter.text(
            "COMMAND CENTER",
            Rect::new(PAD, 32.0, 320.0, 30.0),
            FontKind::Title,
            theme::TEXT,
            Align::Left,
        );
        painter.fill_circle((PAD + 276.0, 48.0), 4.0, theme::ACCENT);
        painter.text(
            "HARDWARE ACPI ACTIVE",
            Rect::new(PAD + 288.0, 40.0, 180.0, 16.0),
            FontKind::Tiny,
            theme::TEXT_FAINT,
            Align::Left,
        );

        let (label, color) = if self.admin {
            ("ADMINISTRATOR", theme::GOOD)
        } else {
            ("READ-ONLY", theme::WARN)
        };
        let pill_right = if self.admin {
            UI_WIDTH - PAD
        } else {
            UI_WIDTH - PAD - 102.0
        };
        let pill = Rect::new(pill_right - 142.0, 20.0, 142.0, 32.0);
        painter.fill_round_rect(pill, 16.0, mix(theme::CARD, color, 0.15));
        painter.stroke_glow_round_rect(pill, 16.0, color, theme::ACCENT_GLOW);
        painter.fill_circle((pill.x + 18.0, pill.y + 16.0), 4.0, color);
        painter.text(
            label,
            Rect::new(pill.x + 28.0, pill.y, pill.w - 36.0, pill.h),
            FontKind::Label,
            theme::TEXT,
            Align::Left,
        );
        if !self.admin {
            self.draw_button(painter, self.layout.elevate, "Elevate", Hit::Elevate, true);
        }
    }

    fn draw_tabs(&self, painter: &Painter) {
        let is_perf = self.active_tab == ActiveTab::Performance;
        let is_macro = self.active_tab == ActiveTab::Macros;

        let perf_box = self.layout.tab_perf;
        let macro_box = self.layout.tab_macros;
        let bar = Rect::new(
            perf_box.x - 3.0,
            perf_box.y - 3.0,
            perf_box.w + macro_box.w + 6.0,
            perf_box.h + 6.0,
        );

        painter.fill_round_rect(bar, 8.0, theme::TAB_BG);
        painter.stroke_round_rect(bar, 8.0, theme::BORDER, 1.0);

        if is_perf {
            painter.fill_round_rect(perf_box, 6.0, theme::TAB_ACTIVE);
            painter.stroke_round_rect(perf_box, 6.0, theme::BORDER_HOVER, 1.0);
        }
        painter.text(
            "THERMALS & FANS",
            perf_box,
            FontKind::Label,
            if is_perf { theme::TEXT } else { theme::TEXT_DIM },
            Align::Center,
        );

        if is_macro {
            painter.fill_round_rect(macro_box, 6.0, theme::TAB_ACTIVE);
            painter.stroke_round_rect(macro_box, 6.0, theme::BORDER_HOVER, 1.0);
        }
        painter.text(
            "MACRO KEYS (M1, M2, M3)",
            macro_box,
            FontKind::Label,
            if is_macro { theme::TEXT } else { theme::TEXT_DIM },
            Align::Center,
        );
    }

    fn draw_telemetry(&self, painter: &Painter) {
        let cpu_temp = self.temp_of(awcc::SENSOR_CPU);
        let gpu_temp = self.temp_of(awcc::SENSOR_GPU);
        let cpu_load = self.loads.cpu.map(|load| load.clamp(0, 100));
        let gpu_load = self.loads.gpu.map(|load| load.clamp(0, 100));

        self.draw_gauge_card(
            painter,
            self.layout.cpu,
            "CPU TELEMETRY",
            "SENSOR 0x01",
            cpu_temp,
            cpu_load,
        );
        self.draw_gauge_card(
            painter,
            self.layout.gpu,
            "GPU TELEMETRY",
            "SENSOR 0x06",
            gpu_temp,
            gpu_load,
        );
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
            Rect::new(card.x + 22.0, card.y + 16.0, 140.0, 20.0),
            FontKind::Label,
            theme::TEXT,
            Align::Left,
        );
        painter.text(
            sensor,
            Rect::new(card.x + card.w - 130.0, card.y + 18.0, 110.0, 18.0),
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
            painter.arc(arc_rect, 135.0, 270.0 * ratio, mix(color, theme::BG, 0.45), 14.0);
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
            "FAN COOLING & TACHOMETERS",
            Rect::new(card.x + 20.0, card.y + 14.0, 300.0, 18.0),
            FontKind::Section,
            theme::TEXT_FAINT,
            Align::Left,
        );

        let sub_w = (card.w - 40.0 - 16.0) / 2.0;
        let sub_h = 76.0;
        let entries = [("CPU FAN", FAN_CPU), ("GPU FAN", FAN_GPU)];

        for (index, (label, id)) in entries.iter().enumerate() {
            let x = card.x + 20.0 + index as f32 * (sub_w + 16.0);
            let y = card.y + 36.0;
            let sub_rect = Rect::new(x, y, sub_w, sub_h);

            painter.fill_round_rect(sub_rect, 10.0, theme::BG);
            painter.stroke_round_rect(sub_rect, 10.0, theme::BORDER, 1.0);

            let fan = self.fan_of(*id);
            let rpm = fan.and_then(|fan| fan.rpm);
            let max_rpm = fan.and_then(|fan| fan.max_rpm).unwrap_or(6000);

            painter.text(
                label,
                Rect::new(x + 14.0, y + 10.0, 90.0, 18.0),
                FontKind::Label,
                theme::TEXT,
                Align::Left,
            );

            match rpm {
                Some(0) => {
                    painter.fill_circle((x + sub_w - 115.0, y + 19.0), 3.0, theme::GOOD);
                    painter.text(
                        "PASSIVE 0 dB",
                        Rect::new(x + sub_w - 110.0, y + 10.0, 96.0, 18.0),
                        FontKind::Tiny,
                        theme::GOOD,
                        Align::Right,
                    );
                    painter.text(
                        "0 RPM",
                        Rect::new(x + 14.0, y + 30.0, 90.0, 22.0),
                        FontKind::Value,
                        theme::TEXT_DIM,
                        Align::Left,
                    );
                    painter.text(
                        "Passive (<60°C)",
                        Rect::new(x + 105.0, y + 32.0, sub_w - 119.0, 18.0),
                        FontKind::Tiny,
                        theme::TEXT_FAINT,
                        Align::Right,
                    );
                }
                Some(v) => {
                    painter.fill_circle((x + sub_w - 96.0, y + 19.0), 3.0, theme::ACCENT);
                    painter.text(
                        "COOLING",
                        Rect::new(x + sub_w - 90.0, y + 10.0, 76.0, 18.0),
                        FontKind::Tiny,
                        theme::ACCENT,
                        Align::Right,
                    );
                    painter.text(
                        &format!("{v} RPM"),
                        Rect::new(x + 14.0, y + 30.0, 110.0, 22.0),
                        FontKind::Value,
                        theme::ACCENT,
                        Align::Left,
                    );
                    let pct = if max_rpm > 0 {
                        ((v as f32 / max_rpm as f32) * 100.0).round() as u32
                    } else {
                        0
                    };
                    painter.text(
                        &format!("{pct}% · Max {max_rpm} RPM"),
                        Rect::new(x + 120.0, y + 32.0, sub_w - 134.0, 18.0),
                        FontKind::Tiny,
                        theme::TEXT_FAINT,
                        Align::Right,
                    );
                }
                None => {
                    painter.text(
                        "OFFLINE",
                        Rect::new(x + sub_w - 90.0, y + 10.0, 76.0, 18.0),
                        FontKind::Tiny,
                        theme::TEXT_FAINT,
                        Align::Right,
                    );
                    painter.text(
                        "-- RPM",
                        Rect::new(x + 14.0, y + 30.0, 100.0, 22.0),
                        FontKind::Value,
                        theme::TEXT_FAINT,
                        Align::Left,
                    );
                }
            }

            let bar = Rect::new(x + 14.0, y + 56.0, sub_w - 28.0, 6.0);
            painter.fill_round_rect(bar, 3.0, theme::TRACK);
            if let Some(v) = rpm.filter(|&v| v > 0 && max_rpm > 0) {
                let fill_w = (bar.w * (v as f32 / max_rpm as f32).clamp(0.0, 1.0)).max(4.0);
                painter.fill_round_rect(
                    Rect::new(bar.x, bar.y, fill_w, bar.h),
                    3.0,
                    theme::ACCENT,
                );
            }
        }
    }

    fn draw_profiles(&self, painter: &Painter) {
        let card = self.layout.profile;
        painter.fill_round_rect(card, 16.0, theme::CARD);
        painter.stroke_round_rect(card, 16.0, theme::BORDER, 1.0);
        painter.text(
            "THERMAL PROFILES  [F6 CYCLE]",
            Rect::new(card.x + 20.0, card.y + 20.0, 300.0, 18.0),
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
                painter.stroke_glow_round_rect(tile, 12.0, theme::ACCENT, theme::ACCENT_GLOW);
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

            let tag = match profile.id {
                0xA4 => "TURBO",
                PROFILE_QUIET => "WHISPER",
                0xA0 => "BALANCED",
                PROFILE_PERFORMANCE => "BOOST",
                0xA2 => "ECO",
                _ => "CUSTOM",
            };
            let tag_color = if active { theme::ACCENT } else { theme::TEXT_FAINT };
            painter.text(
                tag,
                Rect::new(tile.x + tile.w - 76.0, tile.y + 12.0, 64.0, 16.0),
                FontKind::Tiny,
                tag_color,
                Align::Right,
            );

            painter.text(
                profile.label,
                Rect::new(tile.x + 14.0, tile.y + 12.0, tile.w - 92.0, 20.0),
                FontKind::Label,
                name_color,
                Align::Left,
            );
            painter.text(
                MODE_HINTS[index],
                Rect::new(tile.x + 14.0, tile.y + 34.0, tile.w - 28.0, 18.0),
                FontKind::Tiny,
                hint_color,
                Align::Left,
            );
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
            "STEALTH [F7]",
            Rect::new(
                switch.x - 110.0,
                switch.y + 6.0,
                100.0,
                18.0,
            ),
            FontKind::Label,
            if on { theme::ACCENT } else { theme::TEXT_FAINT },
            Align::Right,
        );
    }

    fn draw_macros_tab(&self, painter: &Painter) {
        let m1_names = ["None (Kaldır)", "Turbo Overdrive", "Max Cooling (-15°C)", "Task Manager"];
        let m1_hints = ["F1 Normal Windows", "100% Fans & Clocks", "Drop CPU limit to 85°C", "Open Windows Taskmgr"];

        let m2_names = ["None (Kaldır)", "Whisper Quiet", "Stealth Toggle", "Eco Battery"];
        let m2_hints = ["F2 Normal Windows", "Silent 0 dB Curve", "Toggle Stealth Mode", "Power Saving Limit"];

        let m3_names = ["None (Kaldır)", "Balanced Daily", "Performance Boost", "Factory Defaults"];
        let m3_hints = ["F3 Normal Windows", "Standard Factory Curve", "Enhanced Clocks", "Reset TCC & Profile"];

        let cards = [
            ("M1", "KEY F1 / ALT+F1", self.layout.m1_card, self.layout.m1_btn, &self.layout.m1_opts, self.m1_action, &m1_names, &m1_hints, Hit::TriggerM1),
            ("M2", "KEY F2 / ALT+F2", self.layout.m2_card, self.layout.m2_btn, &self.layout.m2_opts, self.m2_action, &m2_names, &m2_hints, Hit::TriggerM2),
            ("M3", "KEY F3 / ALT+F3", self.layout.m3_card, self.layout.m3_btn, &self.layout.m3_opts, self.m3_action, &m3_names, &m3_hints, Hit::TriggerM3),
        ];

        for (key_id, hotkey_desc, card, btn, opts, selected_action, names, hints, trigger_hit) in cards {
            painter.fill_round_rect(card, 16.0, theme::CARD);
            painter.stroke_round_rect(card, 16.0, theme::BORDER, 1.0);

            // Key Badge
            let badge = Rect::new(card.x + 20.0, card.y + 18.0, 52.0, 26.0);
            painter.fill_round_rect(badge, 6.0, theme::CARD_ACTIVE);
            painter.stroke_glow_round_rect(badge, 6.0, theme::ACCENT, theme::ACCENT_GLOW);
            painter.text(key_id, badge, FontKind::Label, theme::ACCENT, Align::Center);

            painter.text(
                hotkey_desc,
                Rect::new(card.x + 82.0, card.y + 18.0, 160.0, 16.0),
                FontKind::Tiny,
                theme::TEXT_FAINT,
                Align::Left,
            );

            let is_assigned = selected_action > 0;
            let status_text = if is_assigned {
                format!("ACTIVE: {}", names[selected_action])
            } else {
                "UNASSIGNED (Standard Windows Key)".to_string()
            };
            let status_color = if is_assigned { theme::GOOD } else { theme::TEXT_FAINT };

            painter.text(
                &status_text,
                Rect::new(card.x + 82.0, card.y + 34.0, 320.0, 18.0),
                FontKind::Label,
                status_color,
                Align::Left,
            );

            // Trigger Button
            let btn_label = if is_assigned { "Run Macro" } else { "Unassigned" };
            let btn_enabled = is_assigned;
            let btn_hovered = btn_enabled && (self.hover == trigger_hit || self.pressed == trigger_hit);
            painter.fill_round_rect(
                btn,
                8.0,
                if btn_hovered { theme::ACCENT } else if btn_enabled { theme::CARD_HOVER } else { theme::TRACK },
            );
            painter.stroke_round_rect(btn, 8.0, theme::BORDER, 1.0);
            painter.text(
                btn_label,
                btn,
                FontKind::Label,
                if btn_hovered { theme::BG } else if btn_enabled { theme::TEXT } else { theme::TEXT_FAINT },
                Align::Center,
            );

            // Header for actions
            painter.text(
                "SELECT ASSIGNED ACTION",
                Rect::new(card.x + 20.0, card.y + 96.0, 240.0, 16.0),
                FontKind::Tiny,
                theme::TEXT_FAINT,
                Align::Left,
            );

            // Option selection pills
            for (idx, opt_rect) in opts.iter().enumerate() {
                let active = idx == selected_action;
                let opt_hovered = match trigger_hit {
                    Hit::TriggerM1 => self.hover == Hit::SetM1(idx),
                    Hit::TriggerM2 => self.hover == Hit::SetM2(idx),
                    Hit::TriggerM3 => self.hover == Hit::SetM3(idx),
                    _ => false,
                };

                let pill_color = if active {
                    if idx == 0 { mix(theme::CARD, theme::BAD, 0.2) } else { theme::CARD_ACTIVE }
                } else if opt_hovered {
                    theme::CARD_HOVER
                } else {
                    theme::BG
                };
                let border_color = if active {
                    if idx == 0 { theme::BAD } else { theme::ACCENT }
                } else {
                    theme::BORDER
                };
                painter.fill_round_rect(*opt_rect, 10.0, pill_color);
                if active {
                    if idx == 0 {
                        painter.stroke_round_rect(*opt_rect, 10.0, theme::BAD, 1.5);
                    } else {
                        painter.stroke_glow_round_rect(*opt_rect, 10.0, theme::ACCENT, theme::ACCENT_GLOW);
                    }
                } else {
                    painter.stroke_round_rect(*opt_rect, 10.0, border_color, 1.0);
                }

                painter.text(
                    names[idx],
                    Rect::new(opt_rect.x + 8.0, opt_rect.y + 10.0, opt_rect.w - 16.0, 18.0),
                    FontKind::Label,
                    if active {
                        if idx == 0 { theme::BAD } else { theme::ACCENT }
                    } else {
                        theme::TEXT
                    },
                    Align::Left,
                );
                painter.text(
                    hints[idx],
                    Rect::new(opt_rect.x + 8.0, opt_rect.y + 30.0, opt_rect.w - 16.0, 16.0),
                    FontKind::Tiny,
                    theme::TEXT_FAINT,
                    Align::Left,
                );
            }
        }
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
            let limit = self
                .tcc
                .as_ref()
                .and_then(|tcc| tcc.max_temperature)
                .unwrap_or(100);
            let throttle_target = limit - pending;
            painter.text(
                &format!("{throttle_target}°C (-{pending}°C)"),
                Rect::new(card.x + card.w - 240.0, card.y + 12.0, 220.0, 30.0),
                FontKind::Value,
                if pending == current {
                    theme::TEXT
                } else {
                    theme::ACCENT
                },
                Align::Right,
            );
            painter.text(
                &format!("TJMax: {limit}°C  ·  Thermal Throttle Cap: {throttle_target}°C"),
                Rect::new(card.x + 20.0, card.y + 38.0, 420.0, 18.0),
                FontKind::Tiny,
                theme::TEXT_DIM,
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
            (
                format!("ACPI: {interface}  ·  F6: Cycle  ·  F7: Stealth  ·  {clock}"),
                theme::TEXT_FAINT,
            )
        };
        painter.text(
            &message,
            Rect::new(footer.x, footer.y + 8.0, footer.w - 210.0, 20.0),
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
                if hovered {
                    theme::TEXT
                } else {
                    theme::TEXT_DIM
                }
            } else {
                theme::TEXT_FAINT
            },
            Align::Center,
        );
    }
}

fn status_summary(status: &Status) -> String {
    let mode_name = format!("{} (0x{:02X})", status.current_profile_name, status.current_profile);
    let balanced_name = format!("{} (0x{:02X})", status.balanced_profile_name, status.balanced_profile);

    let temps: Vec<String> = status
        .temps
        .iter()
        .map(|temp| {
            let val = temp
                .celsius
                .map(|val| format!("{val}C"))
                .unwrap_or_else(|| "N/A".into());
            format!("{} {val}", temp.label)
        })
        .collect();

    let fans: Vec<String> = status
        .fans
        .iter()
        .map(|fan| {
            let val = fan
                .rpm
                .map(|val| format!("{val}rpm"))
                .unwrap_or_else(|| "N/A".into());
            format!("{} {val}", fan.label)
        })
        .collect();

    format!(
        "profile={mode_name} balanced={balanced_name} temps=[{}] fans=[{}]",
        temps.join(", "),
        fans.join(", ")
    )
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
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut tme);
                    let x = (lparam & 0xFFFF) as f32 / (*app).scale;
                    let y = ((lparam >> 16) & 0xFFFF) as f32 / (*app).scale;
                    (*app).on_mouse_move(x, y);
                }
                0
            }
            WM_MOUSELEAVE => {
                let app = app_ptr(hwnd);
                if !app.is_null() {
                    (*app).on_mouse_leave();
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
            WM_HOTKEY => {
                let id = wparam as i32;
                let app = app_ptr(hwnd);
                if !app.is_null() {
                    match id {
                        HOTKEY_F6 => (*app).cycle_profile(),
                        HOTKEY_F7 => (*app).toggle_stealth(),
                        HOTKEY_M1 | HOTKEY_M1_ALT => (*app).trigger_m1(),
                        HOTKEY_M2 | HOTKEY_M2_ALT => (*app).trigger_m2(),
                        HOTKEY_M3 | HOTKEY_M3_ALT => (*app).trigger_m3(),
                        _ => {}
                    }
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
                UnregisterHotKey(hwnd, HOTKEY_F6);
                UnregisterHotKey(hwnd, HOTKEY_F7);
                UnregisterHotKey(hwnd, HOTKEY_M1);
                UnregisterHotKey(hwnd, HOTKEY_M2);
                UnregisterHotKey(hwnd, HOTKEY_M3);
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

fn run() -> Result<()> {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let hr = CoInitializeEx(null(), COINIT_MULTITHREADED as u32);
        log::info(format_args!("CoInitializeEx(MTA) -> 0x{hr:08X}"));
    }

    let _guard = GdiPlusGuard::start().context("GDI+ initialization failed")?;
    let instance = unsafe { GetModuleHandleW(null()) };
    let class_name = ui::wide(CLASS_NAME);
    let title = ui::wide(APP_TITLE);

    let icon = unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };
    let cursor = unsafe { LoadCursorW(null_mut(), IDC_ARROW) };

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
        CoUninitialize();
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
