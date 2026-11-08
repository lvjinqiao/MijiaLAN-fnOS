//! Native panel placement and application lifetime; device logic remains in the shared Rust core.
mod menu;
pub use menu::{defer_panel_dismiss, is_tracking_menu, show_popup_menu};

use gpui::Window;
use objc2::{MainThreadMarker, msg_send, rc::Retained, runtime::AnyObject, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSColor, NSEvent, NSEventMask, NSScreen, NSView,
    NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ptr::NonNull;

/// AppKit owns the handlers until the monitor tokens are removed on the main thread.
/// Only outside mouse clicks and this application's Escape key are observed.
pub struct EventMonitors {
    outside: Option<Retained<AnyObject>>,
    escape: Option<Retained<AnyObject>>,
}
impl gpui::Global for EventMonitors {}
impl EventMonitors {
    pub fn new(
        sender: async_channel::Sender<crate::tray::Command>,
        tray: tray_icon::TrayIcon,
    ) -> Self {
        let outside_sender = sender.clone();
        let outside_handler = block2::RcBlock::new(move |_: NonNull<NSEvent>| {
            if !pointer_over_tray(Some(&tray)) && !defer_panel_dismiss() {
                let _ = outside_sender.try_send(crate::tray::Command::Hide);
            }
        });
        let outside = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(
            NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown,
            &outside_handler,
        );
        let escape_handler = block2::RcBlock::new(move |event: NonNull<NSEvent>| {
            // AppKit provides a live event for the duration of this callback.
            let key = unsafe { event.as_ref() };
            if key.keyCode() == 53
                && !is_tracking_menu()
                && let Some(mtm) = MainThreadMarker::new()
                && let Some(window) = key.window(mtm)
                && window
                    .styleMask()
                    .contains(NSWindowStyleMask::NonactivatingPanel)
            {
                // Escape first cancels an in-progress input method composition.
                let composing = window.firstResponder().is_some_and(|responder| {
                    responder.respondsToSelector(sel!(hasMarkedText))
                        && unsafe { msg_send![&*responder, hasMarkedText] }
                });
                if !composing {
                    let _ = sender.try_send(crate::tray::Command::Hide);
                    return std::ptr::null_mut();
                }
            }
            event.as_ptr()
        });
        // The callback returns either the original live event or null to consume Escape.
        let escape = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(
                NSEventMask::KeyDown,
                &escape_handler,
            )
        };
        Self { outside, escape }
    }
}
impl Drop for EventMonitors {
    fn drop(&mut self) {
        for token in [&self.outside, &self.escape].into_iter().flatten() {
            // Both tokens were returned by AppKit monitor registration above.
            unsafe { NSEvent::removeMonitor(token) };
        }
    }
}

pub fn setup() {
    if let Some(mtm) = MainThreadMarker::new() {
        NSApplication::sharedApplication(mtm)
            .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }
}
pub fn status_symbol(tray: &tray_icon::TrayIcon, has_devices: bool) {
    use objc2_app_kit::NSImage;
    use objc2_foundation::NSString;
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(button) = tray.ns_status_item().and_then(|item| item.button(mtm)) else {
        return;
    };
    if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(if has_devices { "homekit" } else { "house" }),
        None,
    ) {
        image.setTemplate(true);
        button.setImage(Some(&image));
    }
}
fn native(window: &Window) -> Option<objc2::rc::Retained<NSWindow>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    // GPUI owns the NSView and this function only borrows it on the main thread.
    let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    view.window()
}
fn anchor(tray: Option<&tray_icon::TrayIcon>) -> Option<(NSRect, NSRect)> {
    let mtm = MainThreadMarker::new()?;
    let button = tray?.ns_status_item()?.button(mtm)?;
    let window = button.window()?;
    // Keep AppKit screen coordinates in logical points. tray-icon's portable
    // rectangle mixes display pixel height and points on Retina displays.
    let icon = window.convertRectToScreen(button.convertRect_toView(button.bounds(), None));
    let screens = NSScreen::screens(mtm);
    let screen = window.screen().or_else(|| {
        screens.iter().find(|screen| {
            let frame = screen.frame();
            let x = icon.origin.x + icon.size.width / 2.;
            let y = icon.origin.y + icon.size.height / 2.;
            x >= frame.origin.x
                && x <= frame.origin.x + frame.size.width
                && y >= frame.origin.y
                && y <= frame.origin.y + frame.size.height
        })
    })?;
    let visible = screen.visibleFrame();
    // AppKit initially creates the status window at (0, 0), then places it in
    // the menu bar on a later run-loop pass. Do not anchor to that placeholder.
    if !window.isVisible()
        || icon.origin.y + icon.size.height < visible.origin.y + visible.size.height
    {
        return None;
    }
    Some((icon, visible))
}
pub fn tray_ready(tray: &tray_icon::TrayIcon) -> bool {
    anchor(Some(tray)).is_some()
}

fn configure_panel_surface(window: &NSWindow) {
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    window.setHasShadow(true);
    if let Some(content) = window.contentView() {
        content.setWantsLayer(true);
        if let Some(layer) = content.layer() {
            // GPUI Component's Root paints a rectangular theme background;
            // GPUI overflow clipping is rectangular too. Clip the entire
            // native content tree, including the Metal backing layer, to the
            // same 18 pt radius as the compact panel, on every page.
            layer.setOpaque(false);
            layer.setCornerRadius(18.);
            layer.setMasksToBounds(true);
            crate::trace_panel(&format!(
                "surface opaque={} radius={} clipped={}",
                window.isOpaque(),
                layer.cornerRadius(),
                layer.masksToBounds()
            ));
        }
    }
    window.invalidateShadow();
}

pub fn position(window: &Window, tray: Option<&tray_icon::TrayIcon>) -> bool {
    let Some(window) = native(window) else {
        return false;
    };
    window.setStyleMask(NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel);
    configure_panel_surface(&window);
    let Some((icon, visible)) = anchor(tray) else {
        return false;
    };
    let width = 360.;
    let height = window.frame().size.height.min(visible.size.height - 16.);
    let x = (icon.origin.x + icon.size.width / 2. - width / 2.).clamp(
        visible.origin.x + 8.,
        visible.origin.x + visible.size.width - width - 8.,
    );
    let y = (icon.origin.y - height - 6.).clamp(
        visible.origin.y + 8.,
        visible.origin.y + visible.size.height - height - 8.,
    );
    crate::trace_panel(&format!(
        "position icon={icon:?} frame=({x}, {y}, {width}, {height})"
    ));
    window.setFrame_display(
        NSRect::new(NSPoint::new(x, y), NSSize::new(width, height)),
        true,
    );
    true
}
pub fn resize_panel(window: &Window, height: f32, cx: &gpui::App) {
    let Some(native_window) = native(window) else {
        return;
    };
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // Retain the GPUI rendering view while AppKit finishes the size change.
    let Some(view) = (unsafe { Retained::retain(handle.ns_view.cast::<NSView>().as_ptr()) }) else {
        return;
    };
    let frame = native_window.frame();
    let Some(screen) = native_window.screen() else {
        return;
    };
    let visible = screen.visibleFrame();
    let height = f64::from(height)
        .ceil()
        .clamp(100., (visible.size.height - 16.).min(660.));
    if (frame.size.height - height).abs() < 0.5 {
        return;
    }
    let top = frame.origin.y + frame.size.height;
    let y = (top - height).clamp(
        visible.origin.y + 8.,
        visible.origin.y + visible.size.height - height - 8.,
    );
    let handle = window.window_handle();
    let mut app = cx.to_async();
    // AppKit invokes GPUI's resize callback synchronously. Defer until the
    // current App/Window borrow ends, and retain the top edge below the menu.
    cx.foreground_executor()
        .spawn(async move {
            if !native_window.isVisible() {
                return;
            }
            native_window.setFrame_display(
                NSRect::new(NSPoint::new(frame.origin.x, y), NSSize::new(360., height)),
                true,
            );
            let size = NSSize::new(360., height);
            view.setFrameSize(size);
            view.setBoundsSize(size);
            // GPUI 0.2 may receive the child resize before AppKit updates its
            // content container. Refresh the final viewport/backing metrics.
            view.viewDidChangeBackingProperties();
            configure_panel_surface(&native_window);
            crate::trace_panel(&format!(
                "resize frame={:?} content={:?} bounds={:?} visible={visible:?}",
                native_window.frame(),
                native_window.contentView().map(|v| v.frame()),
                native_window.contentView().map(|v| v.bounds())
            ));
            let _ = handle.update(&mut app, |_, window, cx| {
                gpui_component::Root::update(window, cx, |_, _, cx| cx.notify());
                window.refresh();
            });
        })
        .detach();
}

/// Resolve SF Symbols through AppKit at runtime; no Apple symbol assets are bundled.
pub fn symbol_png(name: &str) -> Option<Vec<u8>> {
    use objc2::AnyThread;
    use objc2_app_kit::{
        NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSImageSymbolConfiguration,
    };
    use objc2_foundation::{NSDictionary, NSString};
    let _mtm = MainThreadMarker::new()?;
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(name),
        None,
    )
    .or_else(|| {
        NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str("square.grid.2x2"),
            None,
        )
    })?;
    let configuration = NSImageSymbolConfiguration::configurationWithPointSize_weight(64., 0.);
    let image = image.imageWithSymbolConfiguration(&configuration)?;
    let tiff = image.TIFFRepresentation()?;
    let bitmap = NSBitmapImageRep::initWithData(NSBitmapImageRep::alloc(), &tiff)?;
    // The empty properties dictionary is valid for a PNG representation.
    let png = unsafe {
        bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }?;
    Some(png.to_vec())
}
pub fn pointer_over_tray(tray: Option<&tray_icon::TrayIcon>) -> bool {
    let Some((icon, _)) = anchor(tray) else {
        return false;
    };
    let point = NSEvent::mouseLocation();
    point.x >= icon.origin.x - 3.
        && point.x <= icon.origin.x + icon.size.width + 3.
        && point.y >= icon.origin.y - 3.
        && point.y <= icon.origin.y + icon.size.height + 3.
}
