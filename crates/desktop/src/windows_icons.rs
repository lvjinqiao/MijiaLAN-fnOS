//! Lucide 0.468.0 (ISC), embedded for the Windows compact panel.
pub fn load(path: &str) -> Option<&'static [u8]> {
    match path {
        "windows/icons/air-vent.svg" => Some(include_bytes!(
            "../../../resources/windows/icons/air-vent.svg"
        )),
        "windows/icons/carrot.svg" => Some(include_bytes!(
            "../../../resources/windows/icons/carrot.svg"
        )),
        "windows/icons/coffee.svg" => Some(include_bytes!(
            "../../../resources/windows/icons/coffee.svg"
        )),
        "windows/icons/cooking-pot.svg" => Some(include_bytes!(
            "../../../resources/windows/icons/cooking-pot.svg"
        )),
        "windows/icons/fan.svg" => Some(include_bytes!("../../../resources/windows/icons/fan.svg")),
        "windows/icons/hard-drive.svg" => Some(include_bytes!(
            "../../../resources/windows/icons/hard-drive.svg"
        )),
        "windows/icons/house.svg" => {
            Some(include_bytes!("../../../resources/windows/icons/house.svg"))
        }
        "windows/icons/lamp-desk.svg" => Some(include_bytes!(
            "../../../resources/windows/icons/lamp-desk.svg"
        )),
        "windows/icons/layout-grid.svg" => Some(include_bytes!(
            "../../../resources/windows/icons/layout-grid.svg"
        )),
        "windows/icons/lock-keyhole.svg" => Some(include_bytes!(
            "../../../resources/windows/icons/lock-keyhole.svg"
        )),
        "windows/icons/microwave.svg" => Some(include_bytes!(
            "../../../resources/windows/icons/microwave.svg"
        )),
        "windows/icons/plug.svg" => {
            Some(include_bytes!("../../../resources/windows/icons/plug.svg"))
        }
        "windows/icons/power.svg" => {
            Some(include_bytes!("../../../resources/windows/icons/power.svg"))
        }
        "windows/icons/refresh-cw.svg" => Some(include_bytes!(
            "../../../resources/windows/icons/refresh-cw.svg"
        )),
        "windows/icons/server.svg" => Some(include_bytes!(
            "../../../resources/windows/icons/server.svg"
        )),
        "windows/icons/wifi-off.svg" => Some(include_bytes!(
            "../../../resources/windows/icons/wifi-off.svg"
        )),
        "windows/icons/wind.svg" => {
            Some(include_bytes!("../../../resources/windows/icons/wind.svg"))
        }
        _ => None,
    }
}

pub fn symbol_path(name: &str) -> String {
    match name {
        "power" => "windows/icons/power.svg",
        "lamp.desk" => "windows/icons/lamp-desk.svg",
        "air.conditioner.horizontal" => "windows/icons/air-vent.svg",
        "fan" => "windows/icons/fan.svg",
        "air.purifier" => "windows/icons/wind.svg",
        "powerplug" => "windows/icons/plug.svg",
        "cooktop" => "windows/icons/cooking-pot.svg",
        "mug" => "windows/icons/coffee.svg",
        "carrot" => "windows/icons/carrot.svg",
        "oven" => "windows/icons/microwave.svg",
        "square.grid.2x2" => "windows/icons/layout-grid.svg",
        "house" | "homekit" => "windows/icons/house.svg",
        "lock" => "windows/icons/lock-keyhole.svg",
        "server.rack" => "windows/icons/server.svg",
        "wifi.slash" => "windows/icons/wifi-off.svg",
        "arrow.clockwise" => "windows/icons/refresh-cw.svg",
        "externaldrive" => "windows/icons/hard-drive.svg",
        "chevron.left" => "icons/chevron-left.svg",
        "chevron.right" => "icons/chevron-right.svg",
        "chevron.down" => "icons/chevron-down.svg",
        "chevron.up" => "icons/chevron-up.svg",
        "chevron.up.chevron.down" => "icons/chevrons-up-down.svg",
        "checkmark" => "icons/check.svg",
        "xmark" => "icons/close.svg",
        "gearshape" => "icons/settings.svg",
        "paintpalette" => "icons/palette.svg",
        "moon" => "icons/moon.svg",
        "plus" => "icons/plus.svg",
        "minus" => "icons/minus.svg",
        "info.circle" => "icons/info.svg",
        "exclamationmark.triangle" => "icons/triangle-alert.svg",
        _ => "windows/icons/layout-grid.svg",
    }
    .into()
}
