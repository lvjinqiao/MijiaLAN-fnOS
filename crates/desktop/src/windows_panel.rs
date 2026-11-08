//! Native notification-area panel placement, clipping, and region menu.
use crate::panel_placement::{self, Rect};
use gpui::{App, Bounds, Pixels, Window};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::atomic::{AtomicBool, Ordering};
use windows::{
    Win32::{
        Foundation::{HWND, POINT, RECT},
        Graphics::Gdi::{
            ClientToScreen, CreateRoundRectRgn, DeleteObject, GetMonitorInfoW,
            MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect, MonitorFromWindow,
            SetWindowRgn,
        },
        UI::{
            HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
            WindowsAndMessaging::{
                AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, GetForegroundWindow,
                HWND_TOPMOST, IsWindow, MF_CHECKED, MF_STRING, SWP_NOACTIVATE, SetWindowPos,
                TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
            },
        },
    },
    core::HSTRING,
};

static TRACKING_MENU: AtomicBool = AtomicBool::new(false);
pub fn is_tracking_menu() -> bool {
    TRACKING_MENU.load(Ordering::Relaxed)
}

fn native(window: &Window) -> Option<HWND> {
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(HWND(handle.hwnd.get() as *mut _)),
        _ => None,
    }
}
fn icon_rect(tray: Option<&tray_icon::TrayIcon>) -> Option<Rect> {
    let icon = tray?.rect()?;
    let left = icon.position.x.round() as i32;
    let top = icon.position.y.round() as i32;
    Some(Rect {
        left,
        top,
        right: left + icon.size.width as i32,
        bottom: top + icon.size.height as i32,
    })
}
pub fn pointer_over_tray(tray: Option<&tray_icon::TrayIcon>) -> bool {
    let Some(icon) = icon_rect(tray) else {
        return false;
    };
    let mut point = POINT::default();
    // Both coordinates are physical screen pixels in this per-monitor aware process.
    (unsafe { GetCursorPos(&mut point).is_ok() })
        && point.x >= icon.left
        && point.x < icon.right
        && point.y >= icon.top
        && point.y < icon.bottom
}
pub fn position(window: &Window, tray: Option<&tray_icon::TrayIcon>, cx: &App) {
    resize_at_tray(window, f32::from(window.bounds().size.height), tray, cx);
}
pub fn resize_panel(window: &Window, height: f32, cx: &App) {
    let tray = cx
        .has_global::<crate::TrayState>()
        .then(|| &cx.global::<crate::TrayState>().0);
    resize_at_tray(window, height, tray, cx);
}
fn resize_at_tray(window: &Window, height: f32, tray: Option<&tray_icon::TrayIcon>, cx: &App) {
    let Some(hwnd) = native(window) else {
        return;
    };
    let icon = icon_rect(tray);
    unsafe {
        let monitor = if let Some(icon) = icon {
            MonitorFromRect(
                &RECT {
                    left: icon.left,
                    top: icon.top,
                    right: icon.right,
                    bottom: icon.bottom,
                },
                MONITOR_DEFAULTTONEAREST,
            )
        } else {
            MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST)
        };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let mut dpi_x = 96;
        let mut dpi_y = 96;
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        let scale = dpi_x.max(96) as f32 / 96.;
        let work = info.rcWork;
        let bounds = panel_placement::place(
            Rect {
                left: work.left,
                top: work.top,
                right: work.right,
                bottom: work.bottom,
            },
            icon,
            scale,
            height,
        );
        let width = bounds.right - bounds.left;
        let height = bounds.bottom - bounds.top;
        let handle = window.window_handle();
        let mut app = cx.to_async();
        // SetWindowPos sends WM_SIZE/WM_MOVE synchronously. GPUI's callbacks
        // must be able to borrow this Window to update its viewport, so apply
        // native changes after the caller's App/Window update has finished.
        cx.foreground_executor()
            .spawn(async move {
                if !IsWindow(Some(hwnd)).as_bool() || handle.update(&mut app, |_, _, _| {}).is_err()
                {
                    return;
                }
                if SetWindowPos(
                    hwnd,
                    Some(HWND_TOPMOST),
                    bounds.left,
                    bounds.top,
                    width,
                    height,
                    SWP_NOACTIVATE,
                )
                .is_err()
                {
                    return;
                }
                // DWM disables native corner rounding in VMs. Clip to the
                // same radius as the shared GPUI panel on Windows 10/11.
                let diameter = (36. * scale).round() as i32;
                let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, diameter, diameter);
                if !region.0.is_null() && SetWindowRgn(hwnd, Some(region), true) == 0 {
                    let _ = DeleteObject(region.into());
                }
                let _ = handle.update(&mut app, |_, window, cx| {
                    crate::trace_panel(&format!(
                        "windows_position {bounds:?} dpi={dpi_x} viewport={:?}",
                        window.bounds().size
                    ));
                    gpui_component::Root::update(window, cx, |_, _, cx| cx.notify());
                    window.refresh();
                });
            })
            .detach();
    }
}

pub fn show_popup_menu(
    window: &Window,
    bounds: Bounds<Pixels>,
    titles: Vec<String>,
    selected: usize,
    _dark: bool,
    cx: &mut App,
    select: impl FnOnce(usize, &mut App) + 'static,
) {
    let Some(hwnd) = native(window) else {
        return;
    };
    let scale = window.scale_factor();
    let mut point = POINT {
        x: (f32::from(bounds.left()) * scale).round() as i32,
        y: (f32::from(bounds.bottom()) * scale).round() as i32,
    };
    if !unsafe { ClientToScreen(hwnd, &mut point).as_bool() } {
        return;
    }
    let handle = window.window_handle();
    let app = cx.to_async();
    // TrackPopupMenu runs a nested Windows message loop, which also polls
    // GPUI tasks (including input cursor blinking). App::defer still holds
    // the AppCell borrow while flushing effects, so it is not sufficient.
    // Schedule native tracking directly on the foreground executor, with
    // no App or Window borrow held until the menu has returned.
    cx.foreground_executor()
        .spawn(async move {
            if !unsafe { IsWindow(Some(hwnd)).as_bool() }
                || TRACKING_MENU.swap(true, Ordering::Relaxed)
            {
                return;
            }
            crate::trace_panel("windows_menu_open");
            let chosen = unsafe {
                CreatePopupMenu().ok().and_then(|menu| {
                    for (index, title) in titles.iter().enumerate() {
                        let flags = MF_STRING
                            | if index == selected {
                                MF_CHECKED
                            } else {
                                Default::default()
                            };
                        if AppendMenuW(menu, flags, index + 1, &HSTRING::from(title)).is_err() {
                            let _ = DestroyMenu(menu);
                            return None;
                        }
                    }
                    let chosen = TrackPopupMenu(
                        menu,
                        TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                        point.x,
                        point.y,
                        None,
                        hwnd,
                        None,
                    )
                    .0;
                    let _ = DestroyMenu(menu);
                    crate::menu_selection::selected_index(chosen, titles.len())
                })
            };
            TRACKING_MENU.store(false, Ordering::Relaxed);
            crate::trace_panel("windows_menu_close");
            let _ = app.update(move |cx| {
                if handle.update(cx, |_, _, _| {}).is_err() {
                    return;
                }
                if let Some(chosen) = chosen {
                    select(chosen, cx);
                }
                if unsafe { GetForegroundWindow() } != hwnd {
                    let _ = handle.update(cx, |_, window, _| window.remove_window());
                }
            });
        })
        .detach();
}
