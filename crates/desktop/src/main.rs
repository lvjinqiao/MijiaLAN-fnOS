#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "windows", test))]
mod menu_selection;
#[cfg(target_os = "windows")]
mod panel_placement;
mod tray;
mod ui;
#[cfg(target_os = "windows")]
mod windows_icons;
#[cfg(target_os = "windows")]
mod windows_panel;
use gpui::*;
use gpui_component::Root;
use mijialan_core::Engine;
use mijialan_core::localization::text as t;
use mijialan_homekit::HomeKit;
use std::{
    borrow::Cow,
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use std::{cell::RefCell, rc::Rc};

actions!(
    mijialan,
    [
        Quit,
        Refresh,
        AddDevice,
        Settings,
        Dismiss,
        Back,
        LanguageSystem,
        LanguageChinese,
        LanguageEnglish
    ]
);
#[derive(Clone, Default)]
pub struct Assets {
    dynamic: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
}
impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        if let Some(data) = self.dynamic.lock().unwrap().get(path) {
            return Ok(Some(Cow::Owned(data.clone())));
        }
        #[cfg(target_os = "windows")]
        if let Some(data) = windows_icons::load(path) {
            return Ok(Some(Cow::Borrowed(data)));
        }
        gpui_component_assets::Assets.load(path)
    }
    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        gpui_component_assets::Assets.list(path)
    }
}
impl Assets {
    #[cfg(target_os = "windows")]
    fn symbol(&self, name: &str) -> Option<String> {
        Some(windows_icons::symbol_path(name))
    }
    fn qr(&self, text: &str, _id: u64) -> Option<String> {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hash);
        let id = hash.finish();
        let key = format!("private/qr-{id}.svg");
        if self.dynamic.lock().unwrap().contains_key(&key) {
            return Some(key);
        }
        let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
        let svg = code
            .render::<qrcode::render::svg::Color>()
            .min_dimensions(240, 240)
            .quiet_zone(true)
            // GPUI's SVG element uses the alpha channel as an icon mask.
            // Keep the light modules transparent; the view supplies a white background.
            .light_color(qrcode::render::svg::Color("none"))
            .build();
        let key = format!("private/qr-{id}.svg");
        let mut dynamic = self.dynamic.lock().unwrap();
        dynamic.retain(|name, _| !name.starts_with("private/qr-"));
        dynamic.insert(key.clone(), svg.into_bytes());
        Some(key)
    }
    #[cfg(target_os = "macos")]
    fn symbol(&self, name: &str) -> Option<String> {
        use base64::Engine as _;
        let key = format!("system-symbols/{name}.svg");
        if self.dynamic.lock().unwrap().contains_key(&key) {
            return Some(key);
        }
        let png = macos::symbol_png(name)?;
        let data = base64::engine::general_purpose::STANDARD.encode(png);
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="80" viewBox="0 0 80 80"><image width="80" height="80" preserveAspectRatio="xMidYMid meet" href="data:image/png;base64,{data}"/></svg>"#
        );
        self.dynamic
            .lock()
            .unwrap()
            .insert(key.clone(), svg.into_bytes());
        Some(key)
    }
}
fn main() {
    if let Err(error) = run() {
        eprintln!("MijiaLAN Rust: {error}");
        std::process::exit(1);
    }
}
fn trace_panel(event: &str) {
    if let Some(path) = std::env::var_os("MIJIALAN_PANEL_LOG") {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(
                file,
                "{} {event}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            );
        }
    }
}
fn run() -> anyhow::Result<()> {
    trace_panel("startup_engine");
    let demo = std::env::args().any(|a| a == "--demo");
    let engine = Arc::new(if demo {
        Engine::demo_control_center()
    } else {
        Engine::open()?
    });
    let bridge = Arc::new(HomeKit::new(engine.clone()));
    let _poller = engine.start_polling();
    let assets = Assets::default();
    trace_panel("startup_platform");
    let application = Application::new().with_assets(assets.clone());
    trace_panel("startup_platform_ready");
    application.on_reopen(|cx| {
        if cx.has_global::<ReopenState>() {
            let show = cx.global::<ReopenState>().0.clone();
            show(cx);
        }
    });
    application.run(move |cx| {
        trace_panel("startup_widgets");
        gpui_component::init(cx);
        {
            use gpui_component::{Theme, ThemeMode};
            let appearance = if demo {
                std::env::var("MIJIALAN_UI_THEME").unwrap_or_default()
            } else {
                engine.store.config().appearance
            };
            let mode = match appearance.as_str() {
                "light" => ThemeMode::Light,
                "dark" => ThemeMode::Dark,
                _ => cx.window_appearance().into(),
            };
            Theme::change(mode, None, cx);
            if demo && let Ok(language) = std::env::var("MIJIALAN_UI_LANGUAGE") {
                mijialan_core::localization::configure(&language);
            }
        }
        #[cfg(target_os = "macos")]
        macos::setup();
        gpui_component::set_locale(if mijialan_core::localization::english() {
            "en"
        } else {
            "zh-CN"
        });
        cx.on_action(|_: &Quit, cx: &mut App| cx.quit());
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("cmd-r", Refresh, Some("MijiaLAN")),
            KeyBinding::new("f5", Refresh, Some("MijiaLAN")),
            KeyBinding::new("cmd-n", AddDevice, Some("MijiaLAN")),
            KeyBinding::new("ctrl-n", AddDevice, Some("MijiaLAN")),
            KeyBinding::new("cmd-,", Settings, Some("MijiaLAN")),
            KeyBinding::new("ctrl-,", Settings, Some("MijiaLAN")),
            KeyBinding::new("escape", Dismiss, Some("MijiaLAN")),
            KeyBinding::new("cmd-w", Dismiss, Some("MijiaLAN")),
            KeyBinding::new("cmd-left", Back, Some("MijiaLAN")),
        ]);
        trace_panel("startup_menus");
        install_menus(cx);
        trace_panel("startup_menus_ready");
        let system_engine = engine.clone();
        cx.on_action(move |_: &LanguageSystem, cx| select_language(&system_engine, "", cx));
        let chinese_engine = engine.clone();
        cx.on_action(move |_: &LanguageChinese, cx| select_language(&chinese_engine, "zh-CN", cx));
        let english_engine = engine.clone();
        cx.on_action(move |_: &LanguageEnglish, cx| select_language(&english_engine, "en", cx));
        let display_size = cx
            .primary_display()
            .map(|display| display.bounds().size)
            .unwrap_or(size(px(1280.), px(900.)));
        let start_maximized = display_size.width < px(1132.) || display_size.height < px(856.);
        let bounds = if start_maximized {
            Bounds::maximized(None, cx)
        } else {
            Bounds::centered(None, size(px(1100.), px(760.)), cx)
        };
        let minimum_size = size(
            px(820.).min((display_size.width - px(32.)).max(px(320.))),
            px(560.).min((display_size.height - px(96.)).max(px(240.))),
        );
        if !demo && engine.store.config().bridge_enabled {
            let bridge = bridge.clone();
            cx.background_executor()
                .spawn(async move {
                    let _ = bridge.start();
                })
                .detach();
        }
        let session = Rc::new(RefCell::new(ui::UiSession::default()));
        trace_panel("startup_tray");
        let tray = tray::create();
        trace_panel("startup_tray_ready");
        let receiver = match tray {
            Ok(tray) => {
                #[cfg(target_os = "macos")]
                macos::status_symbol(&tray.icon, !engine.store.config().devices.is_empty());
                #[cfg(target_os = "macos")]
                cx.set_global(macos::EventMonitors::new(tray.sender, tray.icon.clone()));
                #[cfg(not(target_os = "macos"))]
                let _ = tray.sender;
                cx.set_global(TrayState(tray.icon));
                Some(tray.commands)
            }
            Err(error) => {
                eprintln!("系统托盘不可用：{error}");
                None
            }
        };
        let has_tray = receiver.is_some();
        let command_session = session.clone();
        let create = Rc::new(move |cx: &mut App| {
            let engine_view = engine.clone();
            let bridge_view = bridge.clone();
            let assets = assets.clone();
            let session = session.clone();
            let panel_height = session.borrow().panel_height();
            let mut options = WindowOptions {
                window_bounds: Some(if start_maximized {
                    WindowBounds::Maximized(bounds)
                } else {
                    WindowBounds::Windowed(bounds)
                }),
                window_min_size: Some(minimum_size),
                titlebar: Some(TitlebarOptions {
                    title: Some("MijiaLAN Rust".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            if cfg!(any(target_os = "macos", target_os = "windows")) {
                options.kind = WindowKind::PopUp;
                options.titlebar = None;
                options.window_background = if cfg!(target_os = "macos") {
                    WindowBackgroundAppearance::Transparent
                } else {
                    WindowBackgroundAppearance::Opaque
                };
                options.window_bounds = Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(360.), px(panel_height)),
                    cx,
                )));
                options.window_min_size = Some(size(px(360.), px(100.)));
                options.is_resizable = false;
                options.is_minimizable = false;
                options.is_movable = false;
            }
            trace_panel("create");
            cx.open_window(options, move |window, cx| {
                #[cfg(target_os = "macos")]
                macos::position(
                    window,
                    cx.has_global::<TrayState>()
                        .then(|| &cx.global::<TrayState>().0),
                );
                #[cfg(target_os = "windows")]
                windows_panel::position(
                    window,
                    cx.has_global::<TrayState>()
                        .then(|| &cx.global::<TrayState>().0),
                    cx,
                );
                let view = cx.new(|cx| {
                    ui::Dashboard::new(engine_view, bridge_view, assets, session, window, cx)
                });
                cx.new(|cx| Root::new(view, window, cx))
            })
        });
        let current = Rc::new(RefCell::new(None::<gpui::WindowHandle<Root>>));
        #[cfg(target_os = "macos")]
        let wait_for_tray = has_tray && !macos::tray_ready(&cx.global::<TrayState>().0);
        #[cfg(not(target_os = "macos"))]
        let wait_for_tray = false;
        if wait_for_tray {
            let create = create.clone();
            let current = current.clone();
            cx.spawn(async move |cx| {
                // AppKit places the status item after the first event-loop pass.
                // Create the panel after its anchor exists, so GPUI's initial
                // content and screen bounds are correct on Retina displays.
                for _ in 0..120 {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(16))
                        .await;
                    #[cfg(target_os = "macos")]
                    let ready = cx
                        .update(|cx| macos::tray_ready(&cx.global::<TrayState>().0))
                        .unwrap_or(true);
                    #[cfg(not(target_os = "macos"))]
                    let ready = true;
                    if ready {
                        break;
                    }
                }
                let _ = cx.update(|cx| {
                    if current.borrow().is_none() {
                        match create(cx) {
                            Ok(window) => {
                                *current.borrow_mut() = Some(window);
                                cx.activate(true);
                            }
                            Err(error) => {
                                eprintln!("无法打开窗口：{error}");
                                cx.quit();
                            }
                        }
                    }
                });
            })
            .detach();
        } else {
            match create(cx) {
                Ok(window) => {
                    trace_panel("startup_window_ready");
                    *current.borrow_mut() = Some(window);
                    cx.activate(true);
                }
                Err(error) => {
                    eprintln!("无法打开窗口：{error}");
                    cx.quit();
                    return;
                }
            }
        }
        let reopen = {
            let current = current.clone();
            let create = create.clone();
            move |cx: &mut App| {
                let activated = current.borrow().as_ref().is_some_and(|handle| {
                    handle
                        .update(cx, |_, window, cx| {
                            #[cfg(not(target_os = "windows"))]
                            let _ = cx;
                            #[cfg(target_os = "windows")]
                            windows_panel::position(
                                window,
                                cx.has_global::<TrayState>()
                                    .then(|| &cx.global::<TrayState>().0),
                                cx,
                            );
                            window.activate_window();
                        })
                        .is_ok()
                });
                if !activated {
                    *current.borrow_mut() = create(cx).ok();
                }
                cx.activate(true);
            }
        };
        let show: Rc<dyn Fn(&mut App)> = Rc::new(reopen);
        if let Some(receiver) = receiver {
            let show = show.clone();
            cx.spawn(async move |cx| {
                while let Ok(command) = receiver.recv().await {
                    let _ = cx.update(|cx| match command {
                        tray::Command::Show => show(cx),
                        tray::Command::Toggle => {
                            trace_panel("tray_toggle");
                            let closed = cfg!(any(target_os = "macos", target_os = "windows"))
                                && current.borrow().as_ref().is_some_and(|handle| {
                                    handle
                                        .update(cx, |_, window, _| window.remove_window())
                                        .is_ok()
                                });
                            if !closed {
                                show(cx);
                            }
                        }
                        tray::Command::Hide => {
                            if cfg!(any(target_os = "macos", target_os = "windows"))
                                && let Some(handle) = current.borrow().as_ref()
                            {
                                trace_panel("hide");
                                let _ = handle.update(cx, |_, window, _| window.remove_window());
                            }
                        }
                        tray::Command::Settings => {
                            command_session.borrow_mut().settings();
                            show(cx);
                        }
                        tray::Command::Quit => cx.quit(),
                    });
                }
            })
            .detach();
        }
        cx.set_global(ReopenState(show));
        cx.on_window_closed(move |cx| {
            if !has_tray && cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
    Ok(())
}

struct TrayState(#[allow(dead_code)] tray_icon::TrayIcon);
impl Global for TrayState {}
struct ReopenState(Rc<dyn Fn(&mut App)>);
impl Global for ReopenState {}

fn select_language(engine: &Engine, language: &str, cx: &mut App) {
    if let Err(error) = engine.store.update(|c| c.language = language.into()) {
        eprintln!("{error}");
        return;
    }
    mijialan_core::localization::configure(language);
    gpui_component::set_locale(if mijialan_core::localization::english() {
        "en"
    } else {
        "zh-CN"
    });
    install_menus(cx);
    cx.refresh_windows();
}
fn install_menus(cx: &mut App) {
    cx.set_menus(vec![
        Menu {
            name: "MijiaLAN Rust".into(),
            items: vec![
                MenuItem::action(t("设置…"), Settings),
                MenuItem::submenu(Menu {
                    name: "语言 / Language".into(),
                    items: vec![
                        MenuItem::action(t("跟随系统"), LanguageSystem),
                        MenuItem::action("简体中文", LanguageChinese),
                        MenuItem::action("English", LanguageEnglish),
                    ],
                }),
                MenuItem::separator(),
                MenuItem::action(t("退出 MijiaLAN Rust"), Quit),
            ],
        },
        Menu {
            name: t("设备").into(),
            items: vec![
                MenuItem::action(t("添加设备…"), AddDevice),
                MenuItem::action(t("刷新状态"), Refresh),
            ],
        },
    ]);
}
