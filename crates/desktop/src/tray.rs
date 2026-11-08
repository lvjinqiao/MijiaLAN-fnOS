use mijialan_core::localization::text as t;
use tray_icon::{
    TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem},
};

pub enum Command {
    Show,
    Toggle,
    Hide,
    Settings,
    Quit,
}
pub struct Tray {
    pub icon: TrayIcon,
    pub commands: async_channel::Receiver<Command>,
    pub sender: async_channel::Sender<Command>,
}
pub fn create() -> anyhow::Result<Tray> {
    let menu = Menu::new();
    let open = MenuItem::new(t("打开 MijiaLAN Rust"), true, None);
    let quit = MenuItem::new(t("退出"), true, None);
    let settings = MenuItem::new(t("设置…"), true, None);
    menu.append(&open)?;
    menu.append(&settings)?;
    menu.append(&quit)?;
    let (sender, receiver) = async_channel::unbounded();
    let open_id = open.id().clone();
    let quit_id = quit.id().clone();
    let settings_id = settings.id().clone();
    let events = sender.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == open_id {
            let _ = events.try_send(Command::Show);
        } else if event.id == quit_id {
            let _ = events.try_send(Command::Quit);
        } else if event.id == settings_id {
            let _ = events.try_send(Command::Settings);
        }
    }));
    let clicks = sender.clone();
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if matches!(
            event,
            TrayIconEvent::Click {
                button: tray_icon::MouseButton::Left,
                button_state: tray_icon::MouseButtonState::Up,
                ..
            }
        ) {
            let _ = clicks.try_send(Command::Toggle);
        } else if matches!(
            event,
            TrayIconEvent::Click {
                button: tray_icon::MouseButton::Right,
                ..
            }
        ) {
            let _ = clicks.try_send(Command::Hide);
        }
    }));
    #[cfg(not(target_os = "windows"))]
    let mut pixels = vec![0u8; 32 * 32 * 4];
    #[cfg(not(target_os = "windows"))]
    for y in 0..32 {
        for x in 0..32 {
            let dx = x as f64 - 15.5;
            let dy = y as f64 - 16.5;
            let ring = (dx * dx + dy * dy).sqrt();
            let stem = (14..=17).contains(&x) && (3..=16).contains(&y);
            if (9. ..=12.).contains(&ring) && !(y < 9 && (10..=21).contains(&x)) || stem {
                let index = (y * 32 + x) * 4;
                pixels[index..index + 4].copy_from_slice(&[30, 166, 148, 255]);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    let icon = tray_icon::Icon::from_rgba(pixels, 32, 32)?;
    #[cfg(target_os = "windows")]
    let icon = tray_icon::Icon::from_resource(1, Some((32, 32))).or_else(|_| {
        tray_icon::Icon::from_rgba(
            include_bytes!("../resources/windows/app-tray.rgba").to_vec(),
            32,
            32,
        )
    })?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip(t("MijiaLAN Rust · 局域网设备控制"))
        .with_icon(icon)
        .build()?;
    Ok(Tray {
        icon: tray,
        commands: receiver,
        sender,
    })
}
