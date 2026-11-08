//! System popup menus. AppKit owns their appearance, tracking and accessibility.
use gpui::{App, Bounds, Pixels, Window};
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained, sel,
};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSApplication, NSControlStateValueOn, NSMenu, NSMenuItem, NSView,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::cell::Cell;

thread_local! {
    static TRACKING: Cell<bool> = const { Cell::new(false) };
    static DISMISS_PANEL: Cell<bool> = const { Cell::new(false) };
}

pub fn is_tracking_menu() -> bool {
    TRACKING.with(Cell::get)
}

/// An outside click still closes the panel, after AppKit has dismissed its menu.
pub fn defer_panel_dismiss() -> bool {
    let tracking = is_tracking_menu();
    if tracking {
        DISMISS_PANEL.with(|dismiss| dismiss.set(true));
    }
    tracking
}

struct Tracking;
impl Tracking {
    fn begin() -> Option<Self> {
        if TRACKING.with(|tracking| tracking.replace(true)) {
            return None;
        }
        DISMISS_PANEL.with(|dismiss| dismiss.set(false));
        Some(Self)
    }
}
impl Drop for Tracking {
    fn drop(&mut self) {
        TRACKING.with(|tracking| tracking.set(false));
        DISMISS_PANEL.with(|dismiss| dismiss.set(false));
    }
}

#[derive(Default)]
struct MenuTargetState {
    selected: Cell<Option<usize>>,
}

define_class!(
    // NSObject has no subclassing requirements; ivars only store a selection.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = MenuTargetState]
    struct MijiaLANRustMenuTarget;

    unsafe impl NSObjectProtocol for MijiaLANRustMenuTarget {}

    impl MijiaLANRustMenuTarget {
        #[unsafe(method(choose:))]
        fn choose(&self, item: &NSMenuItem) {
            self.ivars().selected.set(usize::try_from(item.tag()).ok());
        }
    }
);

/// Queue native tracking outside GPUI's App/Window borrow: NSMenu runs a nested
/// event loop which may dispatch GPUI focus, input and redraw callbacks.
pub fn show_popup_menu(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    titles: Vec<String>,
    selected: usize,
    dark: bool,
    cx: &mut App,
    on_select: impl FnOnce(usize, &mut App) + 'static,
) {
    if titles.is_empty() || bounds.size.width <= gpui::px(0.) || bounds.size.height <= gpui::px(0.)
    {
        return;
    }
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // GPUI owns this view; retain it until native tracking has finished.
    let Some(view) = (unsafe { Retained::retain(handle.ns_view.cast::<NSView>().as_ptr()) }) else {
        return;
    };
    let Some(native_window) = view.window() else {
        return;
    };
    let Some(tracking) = Tracking::begin() else {
        return;
    };
    let target = MijiaLANRustMenuTarget::alloc(mtm).set_ivars(MenuTargetState::default());
    // The class inherits NSObject's standard initializer.
    let target: Retained<MijiaLANRustMenuTarget> = unsafe { msg_send![super(target), init] };
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(""));
    menu.setAutoenablesItems(false);
    menu.setMinimumWidth(f64::from(f32::from(bounds.size.width)));
    // AppKit's immutable appearance-name constants are available on supported macOS versions.
    let appearance = unsafe {
        NSAppearance::appearanceNamed(if dark {
            NSAppearanceNameDarkAqua
        } else {
            NSAppearanceNameAqua
        })
    };
    menu.setAppearance(appearance.as_deref());
    for (index, title) in titles.into_iter().enumerate() {
        // choose: has the standard menu action signature; target is retained
        // alongside the menu until popUpMenu... returns.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(&title),
                Some(sel!(choose:)),
                &NSString::from_str(""),
            )
        };
        item.setTag(index as isize);
        item.setEnabled(true);
        if index == selected {
            item.setState(NSControlStateValueOn);
        }
        unsafe { item.setTarget(Some(&target)) };
        menu.addItem(&item);
    }
    let handle = Window::window_handle(window);
    let focus = window.focused(cx);
    let mut app = cx.to_async();
    cx.foreground_executor()
        .spawn(async move {
            if !native_window.isVisible() {
                return;
            }
            // GPUI bounds use top-left logical points. NSMenu expects the
            // rendering view's coordinates, which can be unflipped on macOS.
            let x = f64::from(f32::from(bounds.origin.x));
            let top = f64::from(f32::from(bounds.origin.y));
            let y = if view.isFlipped() {
                top
            } else {
                view.bounds().size.height - top
            };
            let item = menu.itemAtIndex(selected as isize);
            let was_key = native_window.isKeyWindow();
            crate::trace_panel("native_menu_open");
            menu.popUpMenuPositioningItem_atLocation_inView(
                item.as_deref(),
                NSPoint::new(x, y),
                Some(&view),
            );
            let selection = target.ivars().selected.get();
            let dismiss = DISMISS_PANEL.with(Cell::get)
                || !NSApplication::sharedApplication(mtm).isActive()
                || (was_key && !native_window.isKeyWindow());
            drop(tracking);
            crate::trace_panel(if selection.is_some() {
                "native_menu_selected"
            } else {
                "native_menu_cancelled"
            });
            let _ = handle.update(&mut app, |_, window, cx| {
                if let Some(index) = selection {
                    on_select(index, cx);
                }
                if dismiss {
                    window.remove_window();
                } else {
                    // AppKit restores the presenting window after tracking.
                    // Activating it here would steal focus from an outside click.
                    if let Some(focus) = focus {
                        focus.focus(window);
                    }
                    window.refresh();
                }
            });
        })
        .detach();
}
