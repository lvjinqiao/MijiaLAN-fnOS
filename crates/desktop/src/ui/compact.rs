//! Compact control center shared by macOS menu bar and Windows notification area.
//! All commands still use the shared engine and existing HomeKit identity.
use super::*;
#[cfg(target_os = "macos")]
use crate::macos as native_panel;
#[cfg(target_os = "windows")]
use crate::windows_panel as native_panel;
use gpui_component::{
    Sizable,
    button::ButtonCustomVariant,
    input::InputEvent,
    menu::{ContextMenuExt, DropdownMenu, PopupMenuItem},
};
use std::{cell::Cell, collections::BTreeSet};

#[derive(Default)]
pub(super) struct PanelSession {
    selected_offline: Option<String>,
    expanded: BTreeSet<String>,
    region: Option<String>,
    manual: [String; 3],
    last_scan: Option<std::time::Instant>,
    numeric_drafts: BTreeMap<String, String>,
    heights: BTreeMap<String, f32>,
}
impl PanelSession {
    pub(super) fn height(&self, page: &Page) -> f32 {
        self.heights
            .get(&format!("{page:?}"))
            .copied()
            .unwrap_or(495.)
    }
}
#[derive(Default)]
pub(super) struct PanelState {
    measured_height: Rc<Cell<f32>>,
    inputs: BTreeMap<String, Entity<InputState>>,
    edited: BTreeSet<String>,
    dragging: Option<String>,
    pending: BTreeMap<String, (String, String, Value)>,
    sending: BTreeSet<String>,
    remove_confirmation: Option<String>,
    reset_confirmation: bool,
    focuses: BTreeMap<String, FocusHandle>,
    scanning: bool,
    status_has_devices: Option<bool>,
}
#[derive(Clone, Copy)]
struct Palette {
    panel: Hsla,
    tile: Hsla,
    subtle: Hsla,
    icon: Hsla,
    primary: Hsla,
    secondary: Hsla,
    tertiary: Hsla,
    blue: Hsla,
    line: Hsla,
    dark: bool,
}
impl Palette {
    fn new(cx: &App) -> Self {
        let dark = cx.theme().mode.is_dark();
        Self {
            panel: rgb(if dark { 0x28282c } else { 0xfafafc }).into(),
            tile: rgb(if dark { 0x3b3b3f } else { 0xe9e9eb }).into(),
            subtle: rgb(if dark { 0x333337 } else { 0xf1f1f3 }).into(),
            icon: rgb(if dark { 0x535357 } else { 0xdcdce0 }).into(),
            primary: rgb(if dark { 0xdddddf } else { 0x252527 }).into(),
            secondary: rgb(if dark { 0x9d9d9f } else { 0x7d7d82 }).into(),
            tertiary: rgb(if dark { 0x747478 } else { 0xaaaab0 }).into(),
            blue: rgb(if dark { 0x009aff } else { 0x007aff }).into(),
            line: rgb(if dark { 0x47474b } else { 0xdddddf }).into(),
            dark,
        }
    }
}
fn l(zh: &str, en: &str) -> String {
    if mijialan_core::localization::english() {
        en.into()
    } else {
        zh.into()
    }
}
fn label(value: impl Into<SharedString>, size: f32, color: Hsla) -> Div {
    div()
        .text_size(px(size))
        .line_height(px(size + 3.))
        .text_color(color)
        .child(value.into())
}
fn divider(p: Palette) -> Div {
    div().h(px(0.5)).flex_shrink_0().bg(p.line)
}
fn group(p: Palette) -> Div {
    div()
        .flex()
        .flex_col()
        .w_full()
        .rounded(px(12.))
        .bg(p.subtle)
        .overflow_hidden()
        .border(px(0.5))
        .border_color(p.line.opacity(0.5))
}
fn navigation_row(id: impl Into<SharedString>, p: Palette) -> Stateful<Div> {
    let id = id.into();
    let highlight_group: SharedString = format!("navigation-row:{id}").into();
    div()
        .id(id.clone())
        .group(highlight_group.clone())
        .relative()
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(12.))
        .py(px(11.))
        .cursor_pointer()
        .child(
            // Round the highlight itself: GPUI's rectangular overflow clip
            // cannot preserve a parent card's curved corners.
            div()
                .id(SharedString::from(format!("navigation-row-highlight:{id}")))
                .absolute()
                .left(px(6.))
                .right(px(6.))
                .top(px(3.))
                .bottom(px(3.))
                .rounded(px(7.))
                .group_hover(highlight_group.clone(), move |s| {
                    s.bg(p.primary.opacity(0.025))
                })
                .group_active(highlight_group, move |s| s.bg(p.primary.opacity(0.07))),
        )
}
fn section(title: impl Into<SharedString>, p: Palette) -> Div {
    div().flex().flex_col().gap(px(8.)).child(
        label(title, 13., p.primary)
            .font_weight(FontWeight::SEMIBOLD)
            .pl(px(3.)),
    )
}
fn number(value: f64) -> String {
    if value.fract().abs() < 0.001 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}
fn device_name(device: &mijialan_core::Device) -> String {
    let mut name = device.name.as_str();
    let room_prefix = format!("{}-", device.room);
    if !device.room.is_empty() && name.starts_with(&room_prefix) {
        name = &name[room_prefix.len()..];
    }
    for prefix in ["米家智能", "米家"] {
        if let Some(short) = name.strip_prefix(prefix) {
            name = short;
            break;
        }
    }
    let short = name.trim();
    if short.is_empty() {
        device.name.clone()
    } else {
        short.into()
    }
}
fn tile_name(device: &mijialan_core::Device) -> String {
    let title = device_name(device);
    let compact: String = title.chars().filter(|c| !c.is_whitespace()).collect();
    let factory = match device.model.as_str() {
        "zhimi.airpurifier.rma1" => {
            Some(("空气净化器4Lite", l("净化器 4 Lite", "Purifier 4 Lite")))
        }
        "chunmi.health_pot.a1" => Some(("多功能养生壶", l("养生壶", "Health Kettle"))),
        "careli.fryer.maf07c" => Some(("空气炸锅5.5L可视版", l("空气炸锅", "Air Fryer"))),
        "chunmi.cooker.cmwy3" => Some(("微压IH电饭煲3L", l("电饭煲 3L", "Rice Cooker 3L"))),
        "dmaker.fan.p28" => Some(("直流变频循环扇落地式", l("循环扇", "Circulation Fan"))),
        _ => None,
    };
    factory
        .filter(|(original, _)| compact == *original)
        .map(|(_, short)| short)
        .unwrap_or(title)
}
fn symbol(state: &DeviceState) -> &'static str {
    match state.spec.kind.as_str() {
        "light" => "lamp.desk",
        "air-conditioner" => "air.conditioner.horizontal",
        "fan" => "fan",
        "air-purifier" => "air.purifier",
        "outlet" | "switch" => "powerplug",
        "cooker" => "cooktop",
        "health-pot" => "mug",
        "juicer" => "carrot",
        "air-fryer" => "oven",
        _ => "square.grid.2x2",
    }
}
fn primary_power(state: &DeviceState) -> Option<&Property> {
    state
        .spec
        .properties
        .iter()
        .find(|p| p.name == "on" && p.service != "indicator-light" && p.writable)
}
fn power_value(state: &DeviceState) -> Option<bool> {
    primary_power(state)
        .and_then(|p| state.values.get(&p.key()))
        .and_then(Value::as_bool)
}
fn issue(state: &DeviceState) -> (String, String, bool) {
    let raw = state.error.as_deref().unwrap_or_default();
    if raw.contains("Token") || raw.contains("凭据") || raw.contains("校验") {
        return (
            l("需要重新导入", "Re-import Required"),
            l(
                "设备有回应，但保存的访问凭据无法通过验证。可在设置中重新导入设备。",
                "The device responds, but its saved credentials could not be verified. Re-import it in Settings.",
            ),
            true,
        );
    }
    if raw.is_empty() {
        return (
            l("检测中", "Checking"),
            l(
                "正在检查设备连接，状态会自动更新。",
                "Checking the connection. Status will update automatically.",
            ),
            false,
        );
    }
    if raw.contains("No route") || raw.contains("unreachable") {
        return (
            l("无法连接", "Cannot Connect"),
            l(
                "当前网络无法到达这台设备。请检查设备供电、Wi-Fi 和路由器连接。",
                "This device cannot be reached from the current network. Check its power, Wi-Fi, and router connection.",
            ),
            false,
        );
    }
    (
        l("无响应", "No Response"),
        l(
            "设备没有回应，可能已离线或 IP 地址改变。可以重试，或重新扫描局域网。",
            "The device did not respond. It may be offline or have a new IP address. Retry or scan the LAN again.",
        ),
        false,
    )
}
fn choice_title(choice: &mijialan_core::spec::Choice) -> String {
    if mijialan_core::localization::english() {
        return choice.description_en.clone();
    }
    // Some legacy MIoT specifications only provide English choice names.
    match choice.description.to_ascii_lowercase().as_str() {
        "auto" => "自动",
        "cool" => "制冷",
        "dry" => "除湿",
        "heat" => "制热",
        "fan" => "送风",
        "low" => "低",
        "medium" => "中",
        "high" => "高",
        "off" => "关闭",
        "on" => "开启",
        _ => return choice.description.clone(),
    }
    .into()
}
fn property_value(property: &Property, value: &Value) -> String {
    property
        .choices
        .iter()
        .find(|c| c.value == *value)
        .map(choice_title)
        .unwrap_or_else(|| property.display(value))
}
fn summary(state: &DeviceState) -> String {
    if !state.online {
        return issue(state).0;
    }
    if power_value(state) == Some(false) {
        return l("已关闭", "Off");
    }
    let read = |name: &str| {
        state
            .spec
            .properties
            .iter()
            .find(|p| p.name == name)
            .and_then(|p| state.values.get(&p.key()).map(|v| (p, v)))
    };
    match state.spec.kind.as_str() {
        "light" => {
            if let Some((_, v)) = read("brightness") {
                return l(
                    &format!("亮度 {}%", number(v.as_f64().unwrap_or(0.))),
                    &format!("Brightness {}%", number(v.as_f64().unwrap_or(0.))),
                );
            }
        }
        "air-conditioner" => {
            let mode = read("mode")
                .map(|(p, v)| property_value(p, v))
                .unwrap_or(l("已开启", "On"));
            return read("target-temperature")
                .and_then(|(_, v)| v.as_f64())
                .map(|v| format!("{mode} {}°", number(v)))
                .unwrap_or(mode);
        }
        "fan" => {
            if let Some((_, v)) = read("speed-level").or_else(|| read("fan-level")) {
                return l(
                    &format!("风速 {} 级", number(v.as_f64().unwrap_or(0.))),
                    &format!("Fan speed {}", number(v.as_f64().unwrap_or(0.))),
                );
            }
        }
        _ => {}
    }
    if let Some((p, v)) = read("status") {
        return property_value(p, v);
    }
    l("已开启", "On")
}

impl Dashboard {
    pub(super) fn compact_cleanup_edits(&mut self) {
        for key in self
            .compact
            .sending
            .iter()
            .chain(self.compact.pending.keys())
            .chain(self.compact.dragging.iter())
        {
            self.drafts.remove(key);
        }
    }
    fn compact_context_menu(
        &self,
        tile: Stateful<Div>,
        did: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let included = self
            .engine
            .store
            .config()
            .devices
            .iter()
            .find(|d| d.did == did)
            .is_some_and(|d| d.bridged);
        tile.context_menu(move |menu, _, _| {
            let target = weak.clone();
            let bridge_did = did.clone();
            let retry = weak.clone();
            let retry_did = did.clone();
            menu.item(
                PopupMenuItem::new(if included {
                    l("不放进「家庭」", "Remove from Home")
                } else {
                    l("放进「家庭」", "Show in Home")
                })
                .on_click(move |_, _, cx| {
                    let _ = target.update(cx, |this, cx| {
                        this.compact_set_bridged(&bridge_did, !included, cx)
                    });
                }),
            )
            .item(
                PopupMenuItem::new(l("重新连接", "Reconnect")).on_click(move |_, _, cx| {
                    let _ = retry.update(cx, |this, cx| {
                        this.compact_retry(vec![retry_did.clone()], cx)
                    });
                }),
            )
        })
        .into_any_element()
    }
    fn compact_menu_button(
        &self,
        id: impl Into<SharedString>,
        title: impl Into<SharedString>,
        p: Palette,
        _primary: bool,
    ) -> Button {
        Button::new(id.into())
            .label(title.into())
            .small()
            .h(px(20.))
            .rounded(px(4.))
            .text_size(px(11.))
            .bg(p.icon.opacity(0.7))
            .text_color(p.primary)
    }
    fn compact_home_management(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(cx);
        let info = self.bridge.info();
        let config = self.engine.store.config();
        let selected = config.devices.iter().filter(|d| d.bridged).count();
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .px(px(16.))
            .pt(px(6.))
            .pb(px(16.))
            .child(
                section(l("家庭桥接", "Home Bridge"), p)
                    .child(group(p).child(self.compact_bridge_toggle(cx))),
            );
        if let Some(error) = info.error {
            body = body.child(label(error, 12., rgb(0xff453a).into()));
        }
        if info.running && !info.paired {
            body=body.child(section(l("添加到「家庭」","Add to Home"),p)
                .child(group(p).child(div().flex().flex_col().items_center().gap(px(10.)).p(px(14.))
                    .when_some(self.assets.qr(&info.uri,0),|d,path|d.child(div().size(px(144.)).p(px(6.)).rounded(px(8.)).bg(rgb(0xffffff))
                        .child(svg().path(path).size_full().text_color(rgb(0x000000)))))
                    .child(label(info.code,20.,p.primary).font_family("Menlo").font_weight(FontWeight::SEMIBOLD))
                    .child(label(l("打开 iPhone「家庭」App，点 + → 添加配件，扫描二维码。","Open Home on your iPhone, tap + → Add Accessory, then scan this code."),12.,p.secondary).text_center())
                    .child(label(l("提示「未认证配件」时，选择「仍然添加」。","If an uncertified accessory warning appears, choose Add Anyway."),11.,p.secondary).text_center()))));
        }
        let mut devices = group(p);
        for (index, device) in config.devices.iter().enumerate() {
            if index > 0 {
                devices = devices.child(divider(p).mx(px(12.)));
            }
            let did = device.did.clone();
            let bridged = device.bridged;
            let state = self.engine.state(&did);
            devices = devices.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(12.))
                    .py(px(10.))
                    .when_some(state.as_ref(), |d, state| {
                        d.child(self.compact_icon(symbol(state), 21., p.secondary))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(label(device_name(device), 13., p.primary).truncate())
                            .child(label(device.room.clone(), 11., p.secondary)),
                    )
                    .child(
                        self.compact_toggle(
                            SharedString::from(format!("bridge-device:{did}")),
                            bridged,
                            true,
                            false,
                            p,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.compact_set_bridged(&did, !bridged, cx)
                        })),
                    ),
            );
        }
        body=body.child(section(l(&format!("在「家庭」中显示 · {selected} 台"),&format!("Shown in Home · {selected}")),p)
            .child(if config.devices.is_empty(){
                label(l("请先返回设置导入米家设备。","Return to Settings to import your Mijia devices first."),12.,p.secondary)
            }else{devices}))
            .child(label(l("关闭已加入的设备，会同时移除它在「家庭」中的房间、场景和自动化。已原生接入 HomeKit 的设备可关闭，避免重复。",
                "Turning off an included device also removes its room, scenes, and automations from Home. Turn off devices already connected through native HomeKit to avoid duplicates."),11.,p.secondary))
            .child(section(l("连接说明","Connection Details"),p).child(label(l("这台电脑和 iPhone 需要连接同一局域网。离家控制和自动化需要家居中枢（HomePod 或 Apple TV）。",
                "This computer and your iPhone must be on the same local network. Remote access and automations require a home hub (HomePod or Apple TV)."),11.,p.secondary)));
        if info.paired {
            body = body.child(divider(p)).child(
                div()
                    .id("reset-pairing")
                    .cursor_pointer()
                    .child(label(
                        l("重置 HomeKit 配对…", "Reset HomeKit Pairing…"),
                        12.,
                        rgb(0xff453a).into(),
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.compact.reset_confirmation = true;
                        cx.notify();
                    })),
            );
        }
        if self.compact.reset_confirmation {
            body=body.child(group(p).p(px(12.)).gap(px(10.))
                .child(label(l("重置 HomeKit 配对？「家庭」会把桥当成新配件，房间、场景和自动化需要重新设置。",
                    "Reset HomeKit Pairing? Home will treat this bridge as a new accessory. Rooms, scenes, and automations will need to be set up again."),12.,p.primary))
                .child(div().flex().justify_end().gap(px(8.))
                    .child(self.compact_button("cancel-reset",l("取消","Cancel"),p,false).on_click(cx.listener(|this,_,_,cx|{this.compact.reset_confirmation=false;cx.notify();})))
                    .child(self.compact_button("confirm-reset",l("重置","Reset"),p,false).text_color(rgb(0xff453a)).on_click(cx.listener(|this,_,_,cx|{
                        this.compact.reset_confirmation=false;let bridge=this.bridge.clone();
                        this.run(cx,move|_|{bridge.reset_pairing().map_err(|e|mijialan_core::Error::Validation(e.to_string()))?;Ok(l("配对已重置","Pairing reset"))});
                    })))));
        }
        body.into_any_element()
    }
    fn compact_device_management(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(cx);
        let config = self.engine.store.config();
        let mut rows = group(p);
        for (index, device) in config.devices.iter().enumerate() {
            if index > 0 {
                rows = rows.child(divider(p).mx(px(12.)));
            }
            let did = device.did.clone();
            let state = self.engine.state(&did);
            rows = rows.child(
                navigation_row(SharedString::from(format!("managed:{did}")), p)
                    .when_some(state.as_ref(), |d, state| {
                        d.child(self.compact_icon(symbol(state), 23., p.secondary))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(label(device_name(device), 13., p.primary).truncate())
                            .child(
                                label(
                                    format!(
                                        "{} · {}",
                                        device.room,
                                        state
                                            .as_ref()
                                            .map(summary)
                                            .unwrap_or(l("检测中", "Checking"))
                                    ),
                                    11.,
                                    p.secondary,
                                )
                                .truncate(),
                            ),
                    )
                    .child(self.compact_icon("chevron.right", 10., p.tertiary))
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.go(Page::Detail(did.clone()), cx)),
                    ),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .px(px(16.))
            .pt(px(6.))
            .pb(px(16.))
            .child(label(
                l(
                    &format!("已导入 {} 台设备", config.devices.len()),
                    &format!("{} devices imported", config.devices.len()),
                ),
                12.,
                p.secondary,
            ))
            .child(rows)
            .child(self.compact_nav_row(
                "discover-devices",
                l("重新扫描局域网", "Scan the LAN Again"),
                "arrow.clockwise",
                None,
                Page::Discover,
                cx,
            ))
            .child(label(
                l(
                    "设备凭据只保存在本机。日常控制通过局域网完成。",
                    "Device credentials stay on this computer. Everyday control stays on the LAN.",
                ),
                11.,
                p.secondary,
            ))
            .into_any_element()
    }
    fn compact_import(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(cx);
        let job = self.session.borrow().login.clone();
        let progress = job.snapshot();
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .px(px(16.))
            .pt(px(6.))
            .pb(px(16.))
            .child(label(
                l(
                    "使用米家 App 扫码，确认后自动导入设备。",
                    "Scan with the Mi Home app and confirm to import your devices.",
                ),
                12.,
                p.secondary,
            ))
            .child(self.compact_region_picker(cx));
        if let Some(path) = self.qr.clone() {
            body = body.child(
                div().flex().justify_center().py(px(8.)).child(
                    div()
                        .size(px(192.))
                        .p(px(8.))
                        .rounded(px(8.))
                        .bg(rgb(0xffffff))
                        .child(svg().path(path).size_full().text_color(rgb(0x000000))),
                ),
            );
        }
        let text = match progress {
            mijialan_core::import_job::Progress::Waiting(_) => {
                l("等待手机确认…", "Waiting for phone confirmation…")
            }
            mijialan_core::import_job::Progress::Fetching => {
                l("正在获取二维码…", "Requesting QR code…")
            }
            mijialan_core::import_job::Progress::Importing => {
                l("正在导入设备…", "Importing devices…")
            }
            _ => l("点击下方按钮获取登录二维码", "Get a login QR code below"),
        };
        body = body.child(label(text, 12., p.secondary).text_center());
        if !job.busy() {
            body = body.child(
                self.compact_button(
                    "start-import",
                    l("获取登录二维码", "Get login QR code"),
                    p,
                    true,
                )
                .on_click(cx.listener(|this, _, _, cx| this.login(cx))),
            );
        } else {
            body = body.child(
                self.compact_button("cancel-import", l("取消", "Cancel"), p, false)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.session.borrow().login.cancel();
                        this.qr = None;
                        this.go(Page::Settings, cx);
                    })),
            );
        }
        body.child(label(
            l(
                "扫码读取设备信息，后续控制只走局域网。",
                "Scan to import. All later control stays on the LAN.",
            ),
            11.,
            p.secondary,
        ))
        .into_any_element()
    }
    fn compact_manual(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(cx);
        let can_add = self.busy == 0
            && !self.host.read(cx).value().is_empty()
            && !self.token.read(cx).value().is_empty();
        let field = |title: String, input: &Entity<InputState>| {
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(label(title, 12., p.primary).font_weight(FontWeight::MEDIUM))
                .child(Input::new(input).small())
        };
        div().flex().flex_col().gap(px(14.)).px(px(16.)).pt(px(6.)).pb(px(16.))
            .child(label(l("已有设备 IP 和 token 时，可以手动添加。","Add a device using its IP address and token."),12.,p.secondary))
            .child(group(p).p(px(14.)).gap(px(12.))
                .child(field(l("名称（可选）","Name (Optional)"),&self.name))
                .child(field(l("IP 地址","IP Address"),&self.host))
                .child(field(l("设备 Token","Device Token"),&self.token))
                .child(self.compact_button("manual-add",if self.busy>0{l("正在添加…","Adding…")}else{l("添加设备","Add Device")},p,true)
                    .opacity(if can_add{1.}else{0.45})
                    .on_click(cx.listener(move|this,_,window,cx|if can_add{this.add(window,cx);}))))
            .child(label(l("设备和这台电脑 需要在同一局域网。重新配网可能使旧 token 失效，可重新扫码导入。",
                "The device and this computer must be on the same LAN. Setting up the device again may invalidate its token; re-import it if needed."),11.,p.secondary))
            .when(!self.error&&self.status.starts_with("已添加"),|d|d.child(label(t(&self.status),12.,rgb(0x30d158).into())))
            .into_any_element()
    }
    fn compact_discovery(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(cx);
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .px(px(16.))
            .pt(px(6.))
            .pb(px(16.))
            .child(label(
                l(
                    "广播查找 miIO 设备，并按设备 ID 更新已知 IP。",
                    "Find miIO devices and update known IP addresses by device ID.",
                ),
                12.,
                p.secondary,
            ))
            .child(
                self.compact_button(
                    "search-lan",
                    if self.busy > 0 {
                        l("正在扫描…", "Scanning…")
                    } else {
                        l("搜索局域网", "Search local network")
                    },
                    p,
                    true,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.busy == 0 {
                        this.discover(cx);
                    }
                })),
            );
        for device in &self.found {
            body = body.child(group(p).p(px(12.)).child(self.compact_info_row(
                "IP".into(),
                device.host.to_string(),
                p,
            )));
        }
        body.into_any_element()
    }
    pub(super) fn compact_render(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = Palette::new(cx);
        let has_devices = !self.engine.store.config().devices.is_empty();
        if self.compact.status_has_devices != Some(has_devices) {
            self.compact.status_has_devices = Some(has_devices);
            if cx.has_global::<crate::TrayState>() {
                #[cfg(target_os = "macos")]
                crate::macos::status_symbol(&cx.global::<crate::TrayState>().0, has_devices);
            }
        }

        if self.busy == 0 {
            self.compact.scanning = false;
        }
        let home = self.page == Page::Devices;
        let header = self.compact_header(cx);
        let body = match self.page.clone() {
            Page::Devices => self.compact_home(cx),
            Page::Detail(did) => self.compact_detail(&did, window, cx),
            Page::Settings => self.compact_settings(cx),
            Page::Bridge => self.compact_home_management(cx),
            Page::DevicesManagement => self.compact_device_management(cx),
            Page::Login => self.compact_import(cx),
            Page::Add => self.compact_manual(cx),
            Page::Discover => self.compact_discovery(cx),
        };
        let scroll = self
            .session
            .borrow_mut()
            .scrolls
            .entry(format!("{:?}", self.page))
            .or_default()
            .clone();
        let measured = self.compact.measured_height.clone();
        let session = self.session.clone();
        let page_key = format!("{:?}", self.page);
        let header_height = if home { 64. } else { 48. };
        let footer_height = if home { 31. } else { 0. };
        let mut contents = div()
            .relative()
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .when(self.error, |d| {
                d.child(
                    label(t(&self.status), 11., rgb(0xff9f0a).into())
                        .px(px(16.))
                        .py(px(8.)),
                )
            })
            .child(body);
        contents = contents.child(
            canvas(
                move |bounds, window, cx| {
                    let content_height = f32::from(bounds.size.height);
                    let target =
                        (content_height.max(90.) + header_height + footer_height).min(660.);
                    if (measured.get() - target).abs() > 0.5 {
                        measured.set(target);
                        session
                            .borrow_mut()
                            .compact
                            .heights
                            .insert(page_key.clone(), target);
                        let handle = window.window_handle();
                        cx.defer(move |cx| {
                            let _ = handle.update(cx, |_, window, cx| {
                                native_panel::resize_panel(window, target, cx)
                            });
                        });
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        );
        let mut panel = div()
            .id("compact-control-center")
            .relative()
            .key_context("MijiaLAN")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Refresh, _, cx| this.compact_scan(cx)))
            .on_action(cx.listener(|this, _: &AddDevice, _, cx| this.go(Page::Add, cx)))
            .on_action(cx.listener(|this, _: &Settings, _, cx| this.go(Page::Settings, cx)))
            .on_action(cx.listener(|_, _: &Dismiss, window, _| window.remove_window()))
            .on_action(cx.listener(|this, _: &crate::Back, _, cx| {
                let route = match this.page {
                    Page::Detail(_) | Page::Settings => Page::Devices,
                    _ => Page::Settings,
                };
                this.go(route, cx);
            }))
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(18.))
            .bg(p.panel)
            .text_color(p.primary)
            .font_family(if cfg!(target_os = "windows") {
                "Segoe UI"
            } else {
                ".AppleSystemUIFont"
            })
            .text_size(px(13.))
            .line_height(px(16.))
            .child(header)
            .child(
                div()
                    .id("compact-page-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .child(contents),
            );
        if home {
            let scan = self
                .session
                .borrow()
                .compact
                .last_scan
                .map(|at| {
                    if at.elapsed().as_secs() < 60 {
                        l("上次扫描 刚刚", "Scanned just now")
                    } else {
                        l(
                            &format!("扫描 {} 分钟前", at.elapsed().as_secs() / 60),
                            &format!("Scanned {} minutes ago", at.elapsed().as_secs() / 60),
                        )
                    }
                })
                .unwrap_or(l("尚未扫描", "Not Scanned Yet"));
            panel = panel.child(
                div()
                    .h(px(31.))
                    .flex_shrink_0()
                    .px(px(16.))
                    .py(px(9.))
                    .border_t(px(0.5))
                    .border_color(p.line)
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(7.))
                            .child(self.compact_icon("lock", 10., p.secondary))
                            .child(label(
                                if self.engine.is_demo() {
                                    l("演示设备 · 不控制真实设备", "Demo · No real device control")
                                } else {
                                    l("控制只走局域网", "LAN Control Only")
                                },
                                11.,
                                p.secondary,
                            )),
                    )
                    .child(label(scan, 11., p.secondary)),
            );
        }
        panel.into_any_element()
    }
    pub(super) fn compact_restore_manual(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.region = self
            .session
            .borrow()
            .compact
            .region
            .clone()
            .unwrap_or_else(|| "cn".into());
        let saved = self.session.borrow().compact.manual.clone();
        for (index, input) in [self.name.clone(), self.host.clone(), self.token.clone()]
            .into_iter()
            .enumerate()
        {
            input.update(cx, |state, cx| {
                state.set_value(saved[index].clone(), window, cx)
            });
            self.subscriptions.push(cx.subscribe(
                &input,
                move |this, input, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.session.borrow_mut().compact.manual[index] =
                            input.read(cx).value().to_string();
                    }
                },
            ));
        }
    }
    fn compact_numeric_input(
        &mut self,
        state: &DeviceState,
        property: &Property,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let full = format!("{}:{}", state.device.did, property.key());
        let actual = self
            .drafts
            .get(&full)
            .or_else(|| state.values.get(&property.key()))
            .and_then(Value::as_f64);
        if let Some(input) = self.compact.inputs.get(&full).cloned() {
            let focused = input.read(cx).focus_handle(cx).is_focused(window);
            let text = actual.map(number).unwrap_or_default();
            if !focused
                && !self.compact.edited.contains(&full)
                && input.read(cx).value().as_ref() != text
            {
                input.update(cx, |input, cx| input.set_value(text, window, cx));
            }
            return input;
        }
        let persisted = self
            .session
            .borrow()
            .compact
            .numeric_drafts
            .get(&full)
            .cloned();
        if persisted.is_some() {
            self.compact.edited.insert(full.clone());
        }
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(persisted.unwrap_or_else(|| actual.map(number).unwrap_or_default()))
                .placeholder("—")
        });
        let key = full.clone();
        let read_did = state.device.did.clone();
        let read_key = property.key();
        self.subscriptions.push(cx.subscribe(
            &input,
            move |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = input.read(cx).value().to_string();
                    let actual = this
                        .engine
                        .state(&read_did)
                        .and_then(|s| s.values.get(&read_key).and_then(Value::as_f64));
                    if text.trim().parse::<f64>().ok() == actual {
                        this.compact.edited.remove(&key);
                        this.session
                            .borrow_mut()
                            .compact
                            .numeric_drafts
                            .remove(&key);
                    } else {
                        this.compact.edited.insert(key.clone());
                        this.session
                            .borrow_mut()
                            .compact
                            .numeric_drafts
                            .insert(key.clone(), text);
                    }
                    cx.notify();
                }
            },
        ));
        let did = state.device.did.clone();
        let property = property.clone();
        self.subscriptions.push(cx.subscribe(
            &input,
            move |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    let text = input.read(cx).value().to_string();
                    this.compact_apply_number(&did, &property, &text, cx);
                }
            },
        ));
        self.compact.inputs.insert(full, input.clone());
        input
    }
    fn compact_apply_number(
        &mut self,
        did: &str,
        property: &Property,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        match text.trim().parse::<f64>() {
            Ok(value) if value.is_finite() => {
                let value = numeric(value, &property.format);
                if let Err(error) = property.validate(&value) {
                    self.error = true;
                    self.status = error.to_string();
                    cx.notify();
                    return;
                }
                self.compact
                    .edited
                    .remove(&format!("{did}:{}", property.key()));
                self.compact_submit(did.into(), property.key(), value, cx);
            }
            _ => {
                self.error = true;
                self.status = l("请输入有效数字", "Enter a valid number.");
                cx.notify();
            }
        }
    }
    /// One in-flight write per control, retaining only the latest drag value.
    /// Tasks keep a weak view; hiding the panel discards unsent edits.
    fn compact_submit(&mut self, did: String, key: String, value: Value, cx: &mut Context<Self>) {
        let full = format!("{did}:{key}");
        if !self.compact.sending.contains(&full)
            && self
                .engine
                .state(&did)
                .and_then(|s| s.values.get(&key).and_then(Value::as_f64))
                .zip(value.as_f64())
                .is_some_and(|(actual, requested)| actual == requested)
        {
            self.drafts.remove(&full);
            cx.notify();
            return;
        }
        self.compact.edited.remove(&full);
        self.session
            .borrow_mut()
            .compact
            .numeric_drafts
            .remove(&full);
        self.drafts.insert(full.clone(), value.clone());
        self.compact.pending.insert(full.clone(), (did, key, value));
        if !self.compact.sending.insert(full.clone()) {
            cx.notify();
            return;
        }
        self.error = false;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(80))
                    .await;
                let next = this.update(cx, |this, _| this.compact.pending.remove(&full));
                let Ok(Some((did, key, value))) = next else {
                    break;
                };
                let engine = match this.read_with(cx, |this, _| this.engine.clone()) {
                    Ok(engine) => engine,
                    Err(_) => break,
                };
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        if let Some(state) = engine.state(&did)
                            && state.spec.kind == "light"
                            && state
                                .spec
                                .properties
                                .iter()
                                .any(|p| p.key() == key && p.name == "brightness")
                            && power_value(&state) == Some(false)
                            && let Some(power) = primary_power(&state)
                        {
                            engine.write(&did, &power.key(), json!(true))?;
                        }
                        engine.write(&did, &key, value)?;
                        let _ = engine.refresh(&did);
                        Ok::<_, mijialan_core::Error>(())
                    })
                    .await;
                let keep_going = this
                    .update(cx, |this, cx| {
                        if let Err(error) = result {
                            this.error = true;
                            this.status = error.to_string();
                        }
                        let more = this.compact.pending.contains_key(&full);
                        if !more {
                            this.compact.sending.remove(&full);
                            if this.compact.dragging.as_ref() != Some(&full) {
                                this.drafts.remove(&full);
                            }
                        }
                        cx.notify();
                        more
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }
    fn compact_slider(
        &mut self,
        state: &DeviceState,
        property: &Property,
        filled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = Palette::new(cx);
        let full = format!("{}:{}", state.device.did, property.key());
        let Some([min, max, step]) = property.range.filter(|r| r[0] < r[1]) else {
            return div().into_any_element();
        };
        let current = self
            .drafts
            .get(&full)
            .or_else(|| state.values.get(&property.key()))
            .and_then(Value::as_f64);
        let fraction = ((current.unwrap_or(min) - min) / (max - min)).clamp(0., 1.) as f32;
        let enabled = state.online && property.writable;
        let bounds = Rc::new(Cell::new(Bounds::<Pixels>::default()));
        let measure = bounds.clone();
        let did = state.device.did.clone();
        let key = property.key();
        let format = property.format.clone();
        let move_bounds = bounds.clone();
        let move_full = full.clone();
        let move_did = did.clone();
        let move_key = key.clone();
        let move_format = format.clone();
        let up_full = full.clone();
        let up_did = did.clone();
        let up_key = key.clone();
        let out_full = full.clone();
        let focus = self
            .compact
            .focuses
            .entry(full.clone())
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone();
        let key_focus = focus.clone();
        let keyboard_did = did.clone();
        let keyboard_key = key.clone();
        let keyboard_format = format.clone();
        let mut bar = div()
            .id(SharedString::from(format!("compact-slider:{full}")))
            .relative()
            .w_full()
            .h(px(if filled { 48. } else { 20. }))
            .flex_shrink_0()
            .when(enabled, |d| d.cursor_pointer())
            .opacity(if enabled { 1. } else { 0.5 });
        if filled {
            bar = bar
                .rounded(px(12.))
                .overflow_hidden()
                .bg(linear_gradient(
                    90.,
                    linear_color_stop(if p.dark { rgb(0xeeeeef) } else { rgb(0x373739) }, fraction),
                    linear_color_stop(p.tile, (fraction + 0.00001).min(1.)),
                ))
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .px(px(14.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(self.compact_icon(
                            "sun.max",
                            18.,
                            if fraction > 0.14 {
                                if p.dark {
                                    rgb(0x18181a).into()
                                } else {
                                    rgb(0xffffff).into()
                                }
                            } else {
                                p.primary
                            },
                        ))
                        .child(
                            label(
                                current
                                    .map(|v| format!("{}%", number(v)))
                                    .unwrap_or(l("尚无读数", "No Reading")),
                                13.,
                                if fraction > 0.91 {
                                    if p.dark {
                                        rgb(0x18181a).into()
                                    } else {
                                        rgb(0xffffff).into()
                                    }
                                } else {
                                    p.primary
                                },
                            )
                            .font_weight(FontWeight::SEMIBOLD),
                        ),
                );
        } else {
            bar = bar
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(8.))
                        .h(px(4.))
                        .rounded_full()
                        .bg(p.icon),
                )
                .child(
                    div()
                        .absolute()
                        .left(relative(fraction))
                        .ml(px(-9.))
                        .top(px(1.))
                        .size(px(18.))
                        .rounded_full()
                        .bg(rgb(0xffffff))
                        .shadow_sm(),
                );
        }
        bar.track_focus(&key_focus)
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if !enabled {
                    return;
                }
                let delta = match event.keystroke.key.as_str() {
                    "left" => -step,
                    "right" => step,
                    _ => return,
                };
                let next = numeric(
                    (current.unwrap_or(min) + delta).clamp(min, max),
                    &keyboard_format,
                );
                this.compact_submit(keyboard_did.clone(), keyboard_key.clone(), next, cx);
                cx.stop_propagation();
            }))
            .child(
                canvas(
                    move |b, _, _| {
                        measure.set(b);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if !enabled {
                        return;
                    }
                    focus.focus(window);
                    this.compact.dragging = Some(full.clone());
                    let b = bounds.get();
                    let fraction =
                        ((event.position.x - b.left()) / b.size.width).clamp(0., 1.) as f64;
                    let value =
                        (min + ((fraction * (max - min)) / step).round() * step).clamp(min, max);
                    this.compact_submit(did.clone(), key.clone(), numeric(value, &format), cx);
                    cx.stop_propagation();
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if !enabled
                    || this.compact.dragging.as_ref() != Some(&move_full)
                    || event.pressed_button != Some(MouseButton::Left)
                {
                    return;
                }
                let b = move_bounds.get();
                let fraction = ((event.position.x - b.left()) / b.size.width).clamp(0., 1.) as f64;
                let value = numeric(
                    (min + ((fraction * (max - min)) / step).round() * step).clamp(min, max),
                    &move_format,
                );
                if this.drafts.get(&move_full) != Some(&value) {
                    this.compact_submit(move_did.clone(), move_key.clone(), value, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    if this.compact.dragging.as_ref() != Some(&up_full) {
                        return;
                    }
                    this.compact.dragging = None;
                    if let Some(value) = this.drafts.get(&up_full).cloned() {
                        this.compact_submit(up_did.clone(), up_key.clone(), value, cx);
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    if this.compact.dragging.as_ref() == Some(&out_full) {
                        this.compact.dragging = None;
                        cx.notify();
                    }
                }),
            )
            .into_any_element()
    }
    fn compact_property(
        &mut self,
        state: &DeviceState,
        property: &Property,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = Palette::new(cx);
        let full = format!("{}:{}", state.device.did, property.key());
        let value = self
            .drafts
            .get(&full)
            .or_else(|| state.values.get(&property.key()))
            .cloned();
        let compact = state.spec.kind == "light" && property.name == "color-temperature";
        let mut row = div()
            .flex()
            .flex_col()
            .gap(px(5.))
            .py(px(if compact { 0. } else { 4. }));
        let mut top = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(10.))
            .child(
                label(property.label().to_owned(), 12., p.primary)
                    .font_weight(FontWeight::MEDIUM)
                    .flex_1(),
            );
        if !property.writable {
            top = top.child(label(
                value
                    .as_ref()
                    .map(|v| property_value(property, v))
                    .unwrap_or(l("尚无读数", "No Reading")),
                12.,
                p.secondary,
            ));
        } else if property.format == "bool" {
            let on = value.as_ref().and_then(Value::as_bool);
            let did = state.device.did.clone();
            let key = property.key();
            let enabled = state.online && self.busy == 0 && on.is_some();
            top = top.child(
                self.compact_toggle(
                    SharedString::from(format!("property:{full}")),
                    on.unwrap_or(false),
                    enabled,
                    true,
                    p,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if enabled {
                        this.write(did.clone(), key.clone(), json!(!on.unwrap()), cx);
                    }
                })),
            );
        } else if !property.choices.is_empty() {
            let title = value
                .as_ref()
                .map(|v| property_value(property, v))
                .unwrap_or(l("请选择", "Select…"));
            let weak = cx.entity().downgrade();
            let did = state.device.did.clone();
            let property = property.clone();
            let cooking = ["cooker", "health-pot", "juicer", "air-fryer"]
                .contains(&property.service.as_str());
            let enabled = (state.online && self.busy == 0) || cooking;
            top = top.child(
                self.compact_menu_button(
                    SharedString::from(format!("choice:{full}")),
                    title,
                    p,
                    false,
                )
                .max_w(px(180.))
                .disabled(!enabled)
                .dropdown_menu(move |menu, _, _| {
                    let mut menu = menu;
                    for choice in &property.choices {
                        let target = weak.clone();
                        let did = did.clone();
                        let key = property.key();
                        let value = choice.value.clone();
                        let is_cooking = ["cooker", "health-pot", "juicer", "air-fryer"]
                            .contains(&property.service.as_str());
                        menu = menu.item(PopupMenuItem::new(choice_title(choice)).on_click(
                            move |_, _, cx| {
                                let _ = target.update(cx, |this, cx| {
                                    if !enabled {
                                        return;
                                    }
                                    if is_cooking {
                                        this.drafts.insert(format!("{did}:{key}"), value.clone());
                                        cx.notify();
                                    } else {
                                        this.write(did.clone(), key.clone(), value.clone(), cx);
                                    }
                                });
                            },
                        ));
                    }
                    menu
                }),
            );
        } else if property.range.is_some() {
            let input = self.compact_numeric_input(state, property, window, cx);
            let text = input.read(cx).value().to_string();
            let dirty = state.online
                && text.trim().parse::<f64>().ok() != value.as_ref().and_then(Value::as_f64);
            let did = state.device.did.clone();
            let prop = property.clone();
            let apply = input.clone();
            top = top.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .child(
                        Input::new(&input)
                            .small()
                            .appearance(false)
                            .h(px(16.))
                            .min_h(px(16.))
                            .max_h(px(16.))
                            .text_right()
                            .text_size(px(11.))
                            .px(px(5.))
                            .py_0()
                            .w(px(66.))
                            .rounded(px(4.))
                            .bg(p.icon.opacity(0.45))
                            .border(px(0.5))
                            .border_color(p.line)
                            .disabled(!state.online),
                    )
                    .when(!property.unit.is_empty(), |d| {
                        d.child(label(
                            if property.unit == "kelvin" {
                                "K".into()
                            } else {
                                property.unit.clone()
                            },
                            11.,
                            p.secondary,
                        ))
                    })
                    .child(
                        self.compact_button(
                            SharedString::from(format!("apply:{full}")),
                            l("应用", "Apply"),
                            p,
                            false,
                        )
                        .h(px(16.))
                        .text_size(px(11.))
                        .px(px(5.))
                        .opacity(if dirty && state.online { 1. } else { 0.4 })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if dirty {
                                let text = apply.read(cx).value().to_string();
                                this.compact_apply_number(&did, &prop, &text, cx);
                            }
                        })),
                    ),
            );
        }
        row = row.child(top);
        row.into_any_element()
    }
    fn compact_detail(
        &mut self,
        did: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(state) = self.engine.state(did) else {
            return div()
                .child(l("设备已移除", "Device removed"))
                .into_any_element();
        };
        let p = Palette::new(cx);
        let mut content = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .px(px(16.))
            .pt(px(8.))
            .pb(px(16.));
        if !state.online {
            content = content.child(self.compact_offline_card(&state, false, cx));
        }
        let mut excluded = BTreeSet::new();
        if let Some(power) = primary_power(&state) {
            excluded.insert(power.key());
        }
        if state.spec.kind == "light" {
            if let Some(brightness) = state
                .spec
                .properties
                .iter()
                .find(|p| p.name == "brightness" && p.writable)
            {
                excluded.insert(brightness.key());
                content = content.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(self.compact_slider(&state, brightness, true, cx))
                        .child(
                            label(
                                if power_value(&state) == Some(false) {
                                    l(
                                        "已关闭 · 拖动滑块即可开灯",
                                        "Off · Drag the slider to turn on",
                                    )
                                } else {
                                    l("拖动调节亮度", "Drag to adjust brightness")
                                },
                                12.,
                                p.secondary,
                            )
                            .text_center()
                            .h(px(16.)),
                        ),
                );
            }
            if let Some(temperature) = state
                .spec
                .properties
                .iter()
                .find(|p| p.name == "color-temperature" && p.writable)
            {
                excluded.insert(temperature.key());
                content = content.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(self.compact_property(&state, temperature, window, cx))
                        .child(self.compact_slider(&state, temperature, false, cx))
                        .child(
                            div()
                                .flex()
                                .justify_between()
                                .child(label(l("暖光", "Warm"), 11., p.secondary))
                                .child(label(l("冷光", "Cool"), 11., p.secondary)),
                        ),
                );
            }
        } else if state.spec.kind == "air-conditioner" {
            if let Some(target) = state
                .spec
                .properties
                .iter()
                .find(|p| p.name == "target-temperature")
            {
                excluded.insert(target.key());
                let value = state.values.get(&target.key()).and_then(Value::as_f64);
                let mut thermostat = div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(8.))
                    .pt(px(6.));
                let did = state.device.did.clone();
                let key = target.key();
                let prop = target.clone();
                for (index, delta) in [-1., 1.].into_iter().enumerate() {
                    if index == 1 {
                        thermostat = thermostat.child(
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .flex()
                                        .items_start()
                                        .child(
                                            label(
                                                value.map(number).unwrap_or("—".into()),
                                                52.,
                                                p.primary,
                                            )
                                            .font_weight(FontWeight::EXTRA_LIGHT),
                                        )
                                        .child(label("°C", 20., p.secondary).pt(px(7.))),
                                )
                                .child(label(summary(&state), 12., p.secondary)),
                        );
                    }
                    let did = did.clone();
                    let key = key.clone();
                    let prop = prop.clone();
                    let enabled = state.online && value.is_some();
                    thermostat = thermostat.child(
                        div()
                            .id(("temperature-step", index))
                            .size(px(38.))
                            .rounded_full()
                            .bg(p.tile)
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .child(self.compact_icon(
                                if delta < 0. { "minus" } else { "plus" },
                                16.,
                                p.primary,
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if enabled {
                                    let next = numeric(value.unwrap() + delta, &prop.format);
                                    if prop.validate(&next).is_ok() {
                                        this.write(did.clone(), key.clone(), next, cx);
                                    }
                                }
                            })),
                    );
                }
                content = content.child(thermostat);
            }
            if let Some(mode) = state
                .spec
                .properties
                .iter()
                .find(|p| p.name == "mode" && p.service == "air-conditioner")
            {
                excluded.insert(mode.key());
                let mut modes = div().flex().gap(px(6.));
                for (index, choice) in mode.choices.iter().enumerate() {
                    let selected = state.values.get(&mode.key()).and_then(Value::as_f64)
                        == choice.value.as_f64();
                    let color = if selected {
                        rgb(0xffffff).into()
                    } else {
                        p.secondary
                    };
                    let icon = match choice.value.as_i64() {
                        Some(0) => "sparkles",
                        Some(1) => "snowflake",
                        Some(2) => "drop",
                        Some(3) => "sun.max",
                        _ => "wind",
                    };
                    let did = state.device.did.clone();
                    let key = mode.key();
                    let value = choice.value.clone();
                    let enabled = state.online && self.busy == 0;
                    modes = modes.child(
                        div()
                            .id(("ac-mode", index))
                            .flex_1()
                            .py(px(9.))
                            .rounded(px(10.))
                            .bg(if selected { p.blue } else { p.subtle })
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(5.))
                            .cursor_pointer()
                            .opacity(if enabled { 1. } else { 0.5 })
                            .child(self.compact_icon(icon, 17., color))
                            .child(label(choice_title(choice), 12., color))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if enabled {
                                    this.write(did.clone(), key.clone(), value.clone(), cx);
                                }
                            })),
                    );
                }
                content = content.child(modes);
            }
            if let Some(fan) = state
                .spec
                .properties
                .iter()
                .find(|p| p.name == "fan-level" && p.service == "fan-control")
            {
                excluded.insert(fan.key());
                let options: Vec<_> = fan
                    .choices
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (choice_title(c), i.to_string()))
                    .collect();
                let selected = fan
                    .choices
                    .iter()
                    .position(|c| {
                        c.value.as_f64() == state.values.get(&fan.key()).and_then(Value::as_f64)
                    })
                    .map(|i| i.to_string())
                    .unwrap_or_default();
                let did = state.device.did.clone();
                let property = fan.clone();
                let enabled = state.online;
                content = content.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .child(label(l("风速", "Fan Speed"), 12., p.secondary))
                        .child(self.compact_segments(
                            "ac-fan",
                            options,
                            selected,
                            246.,
                            cx,
                            move |this, index, _, cx| {
                                if enabled
                                    && let Ok(index) = index.parse::<usize>()
                                    && let Some(choice) = property.choices.get(index)
                                {
                                    this.write(
                                        did.clone(),
                                        property.key(),
                                        choice.value.clone(),
                                        cx,
                                    );
                                }
                            },
                        )),
                );
            }
            if let Some(swing) = state
                .spec
                .properties
                .iter()
                .find(|p| p.name == "vertical-swing")
            {
                excluded.insert(swing.key());
                content = content.child(self.compact_property(&state, swing, window, cx));
            }
            content=content.child(label(l("家庭中的当前温度使用设定温度占位，此设备没有实测室温读数。",
                "Home uses the target temperature as a placeholder for the current temperature. This device does not report measured room temperature."),11.,p.secondary));
        }
        let remaining: Vec<_> = state
            .spec
            .properties
            .iter()
            .filter(|prop| (prop.readable || prop.writable) && !excluded.contains(&prop.key()))
            .cloned()
            .collect();
        if !remaining.is_empty() {
            let id = format!("{did}:controls");
            let special = state.spec.kind == "light" || state.spec.kind == "air-conditioner";
            let expanded = !special || self.session.borrow().compact.expanded.contains(&id);
            if special {
                content =
                    content.child(self.compact_disclosure(&id, l("更多控制", "More Controls"), cx));
            }
            if expanded {
                let mut rows = div().flex().flex_col().gap(px(8.));
                if ["cooker", "health-pot", "juicer", "air-fryer"]
                    .contains(&state.spec.kind.as_str())
                {
                    rows=rows.child(label(l("先选择下次菜单，再点开始。选择参数不会启动设备。","Choose the next menu, then press Start. Selecting settings does not start the device."),11.,p.secondary));
                }
                for prop in remaining {
                    rows = rows.child(self.compact_property(&state, &prop, window, cx));
                }
                content = content.child(rows);
            }
        }
        for action in &state.spec.actions {
            let did = did.to_string();
            let siid = action.siid;
            let aiid = action.aiid;
            let confirms_state = state.spec.kind == "vacuum";
            let inputs: Option<Vec<_>> = action
                .inputs
                .iter()
                .map(|prop| {
                    self.drafts
                        .get(&format!("{did}:{}", prop.key()))
                        .or_else(|| state.values.get(&prop.key()))
                        .cloned()
                })
                .collect();
            let enabled = state.online && inputs.is_some() && self.busy == 0;
            content = content.child(
                self.compact_button(
                    SharedString::from(format!("action:{siid}:{aiid}")),
                    action.label().to_owned(),
                    p,
                    false,
                )
                .opacity(if enabled { 1. } else { 0.4 })
                .on_click(cx.listener(move |this, _, _, cx| {
                    if enabled {
                        let did = did.clone();
                        let inputs = inputs.clone().unwrap_or_default();
                        this.run(cx, move |engine| {
                            engine.action(&did, siid, aiid, inputs)?;
                            Ok(if confirms_state {
                                l("设备状态已确认", "Device state confirmed")
                            } else {
                                l("设备已确认动作", "Device acknowledged the action")
                            })
                        });
                    }
                })),
            );
        }
        content = content.child(divider(p));
        let information = format!("{did}:information");
        content = content.child(self.compact_disclosure(
            &information,
            l("设备信息", "Device Information"),
            cx,
        ));
        if self
            .session
            .borrow()
            .compact
            .expanded
            .contains(&information)
        {
            let mut info = div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(self.compact_info_row(l("名称", "Name"), state.device.name.clone(), p))
                .child(self.compact_info_row(l("房间", "Room"), state.device.room.clone(), p))
                .child(self.compact_info_row(
                    l("IP 地址", "IP Address"),
                    state.device.host.to_string(),
                    p,
                ))
                .child(self.compact_info_row(l("型号", "Model"), state.device.model.clone(), p))
                .child(divider(p))
                .child(
                    label(l("当前读数", "Current Readings"), 11., p.primary)
                        .font_weight(FontWeight::MEDIUM),
                );
            for prop in state.spec.properties.iter().filter(|p| p.readable) {
                let value = state
                    .values
                    .get(&prop.key())
                    .map(|v| property_value(prop, v))
                    .unwrap_or(l("尚未读取", "Not Read Yet"));
                info = info.child(self.compact_info_row(prop.label().into(), value, p));
            }
            if !state.spec.compatibility.is_empty() {
                let compatibility = format!("{did}:compatibility");
                info = info.child(divider(p)).child(self.compact_disclosure(
                    &compatibility,
                    l("兼容性说明", "Compatibility"),
                    cx,
                ));
                if self
                    .session
                    .borrow()
                    .compact
                    .expanded
                    .contains(&compatibility)
                {
                    let english = mijialan_core::localization::english();
                    let mut notes = div().flex().flex_col().gap(px(10.))
                        .child(label(l("以下为当前版本的功能支持情况，无需手动确认。",
                            "Feature support in this version is listed below. No manual confirmation is needed."), 11., p.secondary));
                    for capability in &state.spec.compatibility {
                        notes = notes.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(3.))
                                .child(label(capability.title(english).to_owned(), 11., p.primary))
                                .child(label(
                                    capability.explanation(english).to_owned(),
                                    11.,
                                    p.secondary,
                                )),
                        );
                    }
                    info = info.child(notes);
                }
            }
            content = content.child(info);
        }
        let management = format!("{did}:management");
        content = content.child(self.compact_disclosure(
            &management,
            l("设备管理", "Device Management"),
            cx,
        ));
        if self.session.borrow().compact.expanded.contains(&management) {
            let bridged = self
                .engine
                .store
                .config()
                .devices
                .iter()
                .find(|d| d.did == state.device.did)
                .is_some_and(|d| d.bridged);
            let did = did.to_string();
            let remove = did.clone();
            content = content.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(label(l("在「家庭」中显示", "Show in Home"), 12., p.primary))
                    .child(
                        self.compact_toggle("show-device", bridged, true, true, p)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.compact_set_bridged(&did, !bridged, cx)
                            })),
                    ),
            );
            content = content.child(
                div()
                    .id("remove-device")
                    .cursor_pointer()
                    .child(label(
                        l("从清单移除", "Remove from List"),
                        12.,
                        rgb(0xff453a).into(),
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.compact.remove_confirmation = Some(remove.clone());
                        cx.notify();
                    })),
            );
        }
        if self.compact.remove_confirmation.as_deref() == Some(did) {
            let remove = did.to_string();
            content=content.child(group(p).p(px(12.)).gap(px(10.))
                .child(label(l("从设备清单移除？也会从此桥的家庭配件中移除。","Remove from Device List? It will also be removed from this bridge in Home."),12.,p.primary))
                .child(div().flex().justify_end().gap(px(8.))
                    .child(self.compact_button("cancel-remove",l("取消","Cancel"),p,false).on_click(cx.listener(|this,_,_,cx|{this.compact.remove_confirmation=None;cx.notify();})))
                    .child(self.compact_button("confirm-remove",l("移除","Remove"),p,false).text_color(rgb(0xff453a)).on_click(cx.listener(move|this,_,_,cx|{
                        this.compact.remove_confirmation=None;
                        let remove=remove.clone();
                        this.run(cx,move|engine|{engine.remove(&remove)?;Ok(l("设备已移除","Device removed"))});this.go(Page::Devices,cx);
                    })))));
        }
        content.into_any_element()
    }
    fn compact_set_bridged(&mut self, did: &str, bridged: bool, cx: &mut Context<Self>) {
        let did = did.to_string();
        self.run(cx, move |engine| {
            engine.set_bridged(&did, bridged)?;
            Ok(l("桥接选择已保存", "Bridge selection saved"))
        });
    }

    fn compact_icon(&self, name: &str, size: f32, color: Hsla) -> Div {
        div()
            .size(px(size))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .when_some(self.assets.symbol(name), |d, path| {
                d.child(
                    svg()
                        .path(path)
                        .size(px(size * 1.2))
                        .flex_shrink_0()
                        .text_color(color),
                )
            })
    }
    fn compact_icon_button(
        &self,
        id: impl Into<SharedString>,
        name: &str,
        p: Palette,
    ) -> Stateful<Div> {
        div()
            .id(id.into())
            .size(px(30.))
            .flex_shrink_0()
            .rounded(px(7.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .hover(move |s| s.bg(p.primary.opacity(0.055)))
            .child(self.compact_icon(name, 14., p.secondary))
    }
    fn compact_button(
        &self,
        id: impl Into<SharedString>,
        title: impl Into<SharedString>,
        p: Palette,
        primary: bool,
    ) -> Stateful<Div> {
        div()
            .id(id.into())
            .px(px(8.))
            .h(px(24.))
            .rounded(px(5.))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.))
            .line_height(px(15.))
            .bg(if primary {
                p.blue
            } else {
                p.icon.opacity(0.65)
            })
            .text_color(if primary {
                rgb(0xffffff).into()
            } else {
                p.primary
            })
            .hover(move |s| s.bg(if primary { p.blue.opacity(0.8) } else { p.icon }))
            .child(title.into())
    }
    fn compact_toggle(
        &self,
        id: impl Into<SharedString>,
        on: bool,
        enabled: bool,
        mini: bool,
        p: Palette,
    ) -> Stateful<Div> {
        let (w, h) = if mini { (26., 15.) } else { (32., 18.) };
        div()
            .id(id.into())
            .w(px(w))
            .h(px(h))
            .rounded(px(h / 2.))
            .flex_shrink_0()
            .bg(if on { p.blue } else { p.icon })
            .opacity(if enabled { 1. } else { 0.55 })
            .when(enabled, |d| d.cursor_pointer())
            .relative()
            .child(
                div()
                    .absolute()
                    .top(px(1.))
                    .left(px(if on { w - h + 1. } else { 1. }))
                    .size(px(h - 2.))
                    .rounded_full()
                    .bg(rgb(0xf3f3f4))
                    .shadow_sm(),
            )
    }
    fn compact_retry(&mut self, dids: Vec<String>, cx: &mut Context<Self>) {
        self.run(cx, move |engine| {
            for did in dids {
                let _ = engine.refresh(&did);
            }
            Ok(l("设备状态已刷新", "Device states refreshed"))
        });
    }
    fn compact_scan(&mut self, cx: &mut Context<Self>) {
        self.session.borrow_mut().compact.last_scan = Some(std::time::Instant::now());
        self.compact.scanning = true;
        if self.engine.is_demo() {
            cx.notify();
            return;
        }
        self.run(cx, |engine| {
            let found = discovery::discover(Duration::from_secs(3))?;
            engine.update_discovered_hosts(&found)?;
            engine.refresh_all();
            Ok(l("局域网扫描完成", "LAN scan complete"))
        });
    }
    fn compact_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(cx);
        if self.page == Page::Devices {
            let states = self.engine.states();
            let count = states.iter().filter(|s| s.online).count();
            return div()
                .h(px(64.))
                .flex_shrink_0()
                .pl(px(16.))
                .pr(px(12.))
                .pt(px(14.))
                .pb(px(12.))
                .flex()
                .justify_between()
                .items_start()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            label(
                                if self.engine.is_demo() {
                                    l("米家局域网桥 · 演示", "MijiaLAN · Demo")
                                } else {
                                    l("米家局域网桥", "MijiaLAN")
                                },
                                15.,
                                p.primary,
                            )
                            .font_weight(FontWeight::SEMIBOLD),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(div().size(px(7.)).rounded_full().bg(if count > 0 {
                                    rgb(0x30d158).into()
                                } else {
                                    p.secondary
                                }))
                                .child(label(
                                    if self.compact.scanning {
                                        l("正在扫描局域网…", "Scanning the LAN…")
                                    } else {
                                        l(
                                            &format!("{count} 台在线 · 共 {} 台", states.len()),
                                            &format!("{count} online · {} total", states.len()),
                                        )
                                    },
                                    12.,
                                    p.secondary,
                                )),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(2.))
                        .child(
                            self.compact_icon_button("compact-refresh", "arrow.clockwise", p)
                                .on_click(cx.listener(|this, _, _, cx| this.compact_scan(cx))),
                        )
                        .child(
                            self.compact_icon_button("compact-settings", "gearshape", p)
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.go(Page::Settings, cx)),
                                ),
                        ),
                )
                .into_any_element();
        }
        let (title, back, route) = match &self.page {
            Page::Detail(did) => (
                self.engine
                    .state(did)
                    .map(|s| device_name(&s.device))
                    .unwrap_or(l("设备", "Devices")),
                l("设备", "Devices"),
                Page::Devices,
            ),
            Page::Settings => (l("设置", "Settings"), l("设备", "Devices"), Page::Devices),
            Page::Bridge => (
                l("管理家庭设备", "Manage Home Devices"),
                l("设置", "Settings"),
                Page::Settings,
            ),
            Page::DevicesManagement => (
                l("米家设备", "Mijia Devices"),
                l("设置", "Settings"),
                Page::Settings,
            ),
            Page::Login => (
                l("扫码导入", "QR Import"),
                l("设置", "Settings"),
                Page::Settings,
            ),
            Page::Add => (
                l("手动添加设备", "Add Device Manually"),
                l("设置", "Settings"),
                Page::Settings,
            ),
            Page::Discover => (
                l("发现设备", "Discover Devices"),
                l("设置", "Settings"),
                Page::Settings,
            ),
            Page::Devices => unreachable!(),
        };
        let side = if mijialan_core::localization::english() {
            80.
        } else {
            62.
        };
        let mut trailing = div()
            .w(px(side))
            .h(px(30.))
            .flex()
            .items_center()
            .justify_end();
        if let Page::Detail(did) = &self.page
            && let Some(state) = self.engine.state(did)
            && let Some(power) = primary_power(&state)
        {
            if let Some(on) = power_value(&state) {
                let did = did.clone();
                let key = power.key();
                let enabled = state.online && self.busy == 0;
                trailing = trailing.child(
                    self.compact_toggle("header-power", on, enabled, true, p)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if enabled {
                                this.write(did.clone(), key.clone(), json!(!on), cx);
                            }
                        })),
                );
            } else {
                trailing = trailing.child(label(l("未知", "Unknown"), 11., p.secondary));
            }
        }
        div()
            .h(px(48.))
            .flex_shrink_0()
            .px(px(14.))
            .pt(px(10.))
            .pb(px(8.))
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                div()
                    .id("compact-back")
                    .w(px(side))
                    .h(px(30.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(self.compact_icon("chevron.left", 12., p.blue))
                    .child(label(back, 13., p.blue))
                    .on_click(cx.listener(move |this, _, _, cx| this.go(route.clone(), cx))),
            )
            .child(
                label(title, 15., p.primary)
                    .flex_1()
                    .text_center()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD),
            )
            .child(trailing)
            .into_any_element()
    }
    fn compact_home(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(cx);
        let states = self.engine.states();
        let online: Vec<_> = states.iter().filter(|s| s.online).collect();
        let offline: Vec<_> = states.iter().filter(|s| !s.online).collect();
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .px(px(12.))
            .pt(px(2.))
            .pb(px(12.));
        if states.is_empty() {
            body = body.child(
                div()
                    .py(px(30.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    .child(self.compact_icon("house", 30., p.secondary))
                    .child(
                        label(l("把设备带到控制中心", "Add Your Devices"), 13., p.primary)
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                    .child(
                        label(
                            l(
                                "导入米家设备后，即可在这里通过局域网控制。",
                                "Import your Mijia devices to control them here over the LAN.",
                            ),
                            12.,
                            p.secondary,
                        )
                        .text_center(),
                    )
                    .child(
                        self.compact_button(
                            "empty-setup",
                            l("导入设备", "Import Devices"),
                            p,
                            true,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.go(Page::Settings, cx))),
                    ),
            );
        } else if online.is_empty() {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .p(px(12.))
                    .child(self.compact_icon("wifi.slash", 20., p.secondary))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(label(
                                l("暂时没有在线设备", "No Devices Online"),
                                13.,
                                p.primary,
                            ))
                            .child(label(
                                l(
                                    "设备连接后会自动出现在这里",
                                    "Devices appear here when they reconnect.",
                                ),
                                11.,
                                p.secondary,
                            )),
                    ),
            );
        } else {
            let mut grid = div().flex().flex_wrap().gap(px(8.));
            for state in online {
                let did = state.device.did.clone();
                let open = did.clone();
                let key = primary_power(state).map(Property::key);
                let on = power_value(state);
                let is_on = on == Some(true);
                let mut subtitle = state.device.room.clone();
                if !subtitle.is_empty() {
                    subtitle.push_str(" · ");
                }
                subtitle.push_str(&summary(state));
                let tile = div()
                    .id(SharedString::from(format!("online:{did}")))
                    .w(px(164.))
                    .min_h(px(104.))
                    .p(px(12.))
                    .rounded(px(14.))
                    .bg(p.tile)
                    .relative()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .cursor_pointer()
                    .hover(move |s| s.bg(p.primary.opacity(0.12)))
                    .child(
                        div()
                            .size(px(32.))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(if is_on { p.blue } else { p.icon })
                            .child(self.compact_icon(
                                symbol(state),
                                20.,
                                if is_on {
                                    rgb(0xffffff).into()
                                } else {
                                    p.primary
                                },
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .w_full()
                            .child(
                                label(tile_name(&state.device), 14., p.primary)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .line_clamp(2),
                            )
                            .child(label(subtitle, 12., p.secondary).line_clamp(2)),
                    )
                    .child(
                        self.compact_icon_button(
                            SharedString::from(format!("detail:{did}")),
                            "chevron.right",
                            p,
                        )
                        .absolute()
                        .right(px(5.))
                        .top(px(5.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.go(Page::Detail(open.clone()), cx);
                        })),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.busy > 0 {
                            return;
                        }
                        if let (Some(key), Some(on)) = (&key, on) {
                            this.write(did.clone(), key.clone(), json!(!on), cx);
                        } else {
                            this.go(Page::Detail(did.clone()), cx);
                        }
                    }));
                grid = grid.child(self.compact_context_menu(tile, state.device.did.clone(), cx));
            }
            body = body.child(grid);
        }
        if !offline.is_empty() {
            let selected = self.session.borrow().compact.selected_offline.clone();
            let all_unknown = offline.iter().all(|s| s.error.is_none());
            let dids: Vec<_> = offline.iter().map(|s| s.device.did.clone()).collect();
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(4.))
                    .pt(px(2.))
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(
                                label(
                                    if all_unknown {
                                        l("正在检测", "Checking")
                                    } else {
                                        l("离线设备", "Offline Devices")
                                    },
                                    11.,
                                    p.secondary,
                                )
                                .font_weight(FontWeight::MEDIUM),
                            )
                            .child(label(offline.len().to_string(), 11., p.tertiary)),
                    )
                    .child(
                        div()
                            .id("retry-all")
                            .cursor_pointer()
                            .child(label(
                                if self.busy > 0 {
                                    l("正在重试…", "Retrying…")
                                } else {
                                    l("全部重试", "Retry All")
                                },
                                11.,
                                p.blue,
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if this.busy == 0 {
                                    this.compact_retry(dids.clone(), cx);
                                }
                            })),
                    ),
            );
            let mut grid = div().flex().flex_wrap().gap(px(8.));
            for state in &offline {
                let did = state.device.did.clone();
                let active = selected.as_deref() == Some(&did);
                let tile = div()
                    .id(SharedString::from(format!("offline:{did}")))
                    .w(px(106.66))
                    .h(px(75.))
                    .p(px(10.))
                    .rounded(px(10.))
                    .bg(p.subtle)
                    .border(px(if active { 1.5 } else { 0. }))
                    .border_color(p.blue)
                    .cursor_pointer()
                    .flex()
                    .flex_col()
                    .gap(px(7.))
                    .relative()
                    .hover(move |s| s.bg(p.tile))
                    .child(self.compact_icon(symbol(state), 18., p.secondary))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .w_full()
                            .child(
                                label(tile_name(&state.device), 11., p.secondary)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate(),
                            )
                            .child(
                                label(
                                    if state.device.room.is_empty() {
                                        l("未分配房间", "No Room Assigned")
                                    } else {
                                        state.device.room.clone()
                                    },
                                    10.,
                                    p.secondary,
                                )
                                .truncate(),
                            ),
                    )
                    .when(issue(state).2, |d| {
                        d.child(
                            div()
                                .absolute()
                                .right(px(9.))
                                .top(px(9.))
                                .size(px(6.))
                                .rounded_full()
                                .bg(rgb(0xff9f0a)),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let mut session = this.session.borrow_mut();
                        session.compact.selected_offline =
                            if session.compact.selected_offline.as_deref() == Some(&did) {
                                None
                            } else {
                                Some(did.clone())
                            };
                        cx.notify();
                    }));
                grid = grid.child(self.compact_context_menu(tile, state.device.did.clone(), cx));
            }
            body = body.child(grid);
            if let Some(state) = offline
                .into_iter()
                .find(|s| selected.as_deref() == Some(&s.device.did))
            {
                body = body.child(self.compact_offline_card(state, true, cx));
            }
        }
        body.into_any_element()
    }
    fn compact_disclosure(&self, id: &str, title: String, cx: &mut Context<Self>) -> Stateful<Div> {
        let p = Palette::new(cx);
        let expanded = self.session.borrow().compact.expanded.contains(id);
        let key = id.to_string();
        div()
            .id(SharedString::from(format!("disclosure:{id}")))
            .flex()
            .items_center()
            .gap(px(5.))
            .min_h(px(23.))
            .cursor_pointer()
            .child(self.compact_icon(
                if expanded {
                    "chevron.down"
                } else {
                    "chevron.right"
                },
                9.,
                p.primary,
            ))
            .child(label(title, 12., p.primary).font_weight(FontWeight::MEDIUM))
            .on_click(cx.listener(move |this, _, _, cx| {
                let mut session = this.session.borrow_mut();
                if !session.compact.expanded.remove(&key) {
                    session.compact.expanded.insert(key.clone());
                }
                cx.notify();
            }))
    }
    fn compact_offline_card(
        &self,
        state: &DeviceState,
        show_open: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let p = Palette::new(cx);
        let (title, message, attention) = issue(state);
        let did = state.device.did.clone();
        let open = did.clone();
        let error_key = format!("{did}:raw-error");
        let expanded = self.session.borrow().compact.expanded.contains(&error_key);
        div()
            .p(px(12.))
            .rounded(px(10.))
            .bg(p.subtle)
            .flex()
            .flex_col()
            .gap(px(9.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(4.))
                    .child(
                        label(device_name(&state.device), 13., p.primary)
                            .font_weight(FontWeight::SEMIBOLD)
                            .flex_1()
                            .line_clamp(2),
                    )
                    .child(label(
                        title,
                        11.,
                        if attention {
                            rgb(0xff9f0a).into()
                        } else {
                            p.secondary
                        },
                    )),
            )
            .child(label(message, 12., p.secondary))
            .child(self.compact_info_row(
                l("房间", "Room"),
                if state.device.room.is_empty() {
                    l("未分配", "Unassigned")
                } else {
                    state.device.room.clone()
                },
                p,
            ))
            .child(self.compact_info_row("IP".into(), state.device.host.to_string(), p))
            .when_some(state.error.clone(), |d, error| {
                d.child(self.compact_disclosure(&error_key, l("原始错误", "Original Error"), cx))
                    .when(expanded, |d| {
                        d.child(label(error, 11., p.secondary).font_family("Menlo"))
                    })
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        self.compact_button(
                            SharedString::from(format!("retry:{did}")),
                            l("重试", "Retry"),
                            p,
                            false,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.busy == 0 {
                                this.compact_retry(vec![did.clone()], cx);
                            }
                        })),
                    )
                    .when(show_open, |d| {
                        d.child(
                            div()
                                .id("open-offline")
                                .cursor_pointer()
                                .child(label(l("查看设备", "View Device"), 12., p.blue))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.go(Page::Detail(open.clone()), cx)
                                })),
                        )
                    }),
            )
    }
    fn compact_info_row(&self, name: String, value: String, p: Palette) -> Div {
        div()
            .flex()
            .items_start()
            .justify_between()
            .gap(px(12.))
            .child(label(name, 11., p.secondary).flex_shrink_0())
            .child(label(value, 11., p.primary).text_right().flex_1())
    }
    fn compact_bridge_status(&self) -> String {
        let info = self.bridge.info();
        if !info.running {
            return l("已关闭", "Off");
        }
        if info.paired {
            l(
                &format!("已配对 · {} 个配件", info.accessories),
                &format!("Paired · {} accessories", info.accessories),
            )
        } else {
            l(
                "尚未配对 · 请打开管理家庭设备",
                "Not paired · Open Manage Home Devices",
            )
        }
    }
    fn compact_bridge_toggle(&self, cx: &mut Context<Self>) -> Div {
        let p = Palette::new(cx);
        let info = self.bridge.info();
        let enabled = self.busy == 0 && !self.engine.is_demo();
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(12.))
            .py(px(11.))
            .child(self.compact_icon("house", 24., p.primary))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(label(
                        l("启用家庭桥接", "Enable Home Bridge"),
                        13.,
                        p.primary,
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .child(div().size(px(6.)).rounded_full().bg(if info.running {
                                rgb(0x30d158).into()
                            } else {
                                p.secondary
                            }))
                            .child(label(self.compact_bridge_status(), 11., p.secondary)),
                    ),
            )
            .child(
                self.compact_toggle("bridge-on", info.running, enabled, false, p)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !enabled {
                            return;
                        }
                        let on = !info.running;
                        let bridge = this.bridge.clone();
                        this.run(cx, move |engine| {
                            if on {
                                bridge
                                    .start()
                                    .map_err(|e| mijialan_core::Error::Validation(e.to_string()))?;
                            } else {
                                bridge.stop();
                            }
                            engine.store.update(|c| c.bridge_enabled = on)?;
                            Ok(if on {
                                l("家庭桥接已启动", "Home bridge started")
                            } else {
                                l("家庭桥接已停止", "Home bridge stopped")
                            })
                        });
                    })),
            )
    }
    fn compact_nav_row(
        &self,
        id: &str,
        title: String,
        icon: &str,
        value: Option<String>,
        route: Page,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let p = Palette::new(cx);
        navigation_row(SharedString::from(id.to_string()), p)
            .child(self.compact_icon(icon, 24., p.primary))
            .child(label(title, 13., p.primary).flex_1())
            .when_some(value, |d, v| d.child(label(v, 12., p.secondary)))
            .child(self.compact_icon("chevron.right", 10., p.tertiary))
            .on_click(cx.listener(move |this, _, _, cx| this.go(route.clone(), cx)))
    }
    fn compact_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(cx);
        let config = self.engine.store.config();
        let selected = config.devices.iter().filter(|d| d.bridged).count();
        let awake = config.keep_awake;
        let mut body = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .px(px(16.))
            .pt(px(6.))
            .pb(px(14.));
        body = body.child(
            section(l("Apple「家庭」", "Apple Home"), p)
                .child(
                    group(p)
                        .child(self.compact_bridge_toggle(cx))
                        .child(divider(p).mx(px(12.)))
                        .child(self.compact_nav_row(
                            "manage-home",
                            l("管理家庭设备", "Manage Home Devices"),
                            "gearshape",
                            Some(l(
                                &format!("{selected} 台"),
                                &format!("{selected} selected"),
                            )),
                            Page::Bridge,
                            cx,
                        )),
                )
                .child(
                    group(p).mt(px(2.)).child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .px(px(12.))
                            .py(px(11.))
                            .child(self.compact_icon("moon", 24., p.primary))
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(label(
                                        l("保持电脑唤醒", "Keep Computer Awake"),
                                        13.,
                                        p.primary,
                                    ))
                                    .child(label(
                                        l("家庭桥接运行时生效", "While the Home bridge is running"),
                                        11.,
                                        p.secondary,
                                    )),
                            )
                            .child(
                                self.compact_toggle("keep-awake", awake, true, false, p)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.run(cx, move |engine| {
                                            engine.store.update(|c| c.keep_awake = !awake)?;
                                            Ok(l(
                                                "闲置休眠设置已保存",
                                                "Idle sleep preference saved",
                                            ))
                                        });
                                    })),
                            ),
                    ),
                ),
        );
        body = body.child(divider(p)).child(
            section(l("设备接入", "Device Setup"), p).child(
                group(p)
                    .child(self.compact_nav_row(
                        "mijia-devices",
                        l("米家设备", "Mijia Devices"),
                        "externaldrive",
                        Some(l(
                            &format!("已导入 {} 台", config.devices.len()),
                            &format!("{} imported", config.devices.len()),
                        )),
                        Page::DevicesManagement,
                        cx,
                    ))
                    .child(divider(p).mx(px(12.)))
                    .child(self.compact_region_picker(cx))
                    .child(
                        self.compact_button(
                            "qr-import",
                            if config.devices.is_empty() {
                                l("扫码导入设备", "Scan to Import Devices")
                            } else {
                                l("重新扫码导入", "Re-import with QR Code")
                            },
                            p,
                            true,
                        )
                        .h(px(25.))
                        .mx(px(10.))
                        .mb(px(9.))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.region = this
                                .session
                                .borrow()
                                .compact
                                .region
                                .clone()
                                .unwrap_or_else(|| "cn".into());
                            this.login(cx);
                            this.go(Page::Login, cx);
                        })),
                    )
                    .child(divider(p).mx(px(12.)))
                    .child(self.compact_nav_row(
                        "manual-device",
                        l("手动添加设备", "Add Device Manually"),
                        "plus.circle",
                        None,
                        Page::Add,
                        cx,
                    ))
                    .child(
                        label(
                            l(
                                "扫码读取设备信息，后续控制只走局域网。",
                                "Scan to import. All later control stays on the LAN.",
                            ),
                            11.,
                            p.secondary,
                        )
                        .px(px(12.))
                        .pb(px(11.)),
                    ),
            ),
        );
        let appearance = config.appearance.clone();
        body = body.child(divider(p)).child(
            section(l("外观", "Appearance"), p).child(
                group(p).child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(12.))
                        .py(px(11.))
                        .child(self.compact_icon("paintpalette", 24., p.primary))
                        .child(label(l("外观", "Appearance"), 13., p.primary).flex_1())
                        .child(self.compact_segments(
                            "appearance",
                            vec![
                                (l("自动", "Auto"), "system".into()),
                                (l("浅色", "Light"), "light".into()),
                                (l("深色", "Dark"), "dark".into()),
                            ],
                            if appearance.is_empty() {
                                "system".into()
                            } else {
                                appearance
                            },
                            166.,
                            cx,
                            |this, value, window, cx| {
                                let mode = match value.as_str() {
                                    "light" => ThemeMode::Light,
                                    "dark" => ThemeMode::Dark,
                                    _ => window.appearance().into(),
                                };
                                Theme::change(mode, Some(window), cx);
                                this.run(cx, move |engine| {
                                    engine.store.update(|c| c.appearance = value)?;
                                    Ok(l("外观设置已保存", "Appearance saved"))
                                });
                            },
                        )),
                ),
            ),
        );
        body.child(divider(p))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(self.compact_icon("lock", 11., p.secondary))
                            .child(label(
                                l("仅局域网控制", "LAN Control Only"),
                                11.,
                                p.secondary,
                            )),
                    )
                    .child(
                        self.compact_button("quit", l("退出 MijiaLAN", "Quit MijiaLAN"), p, false)
                            .h(px(20.))
                            .on_click(|_, _, cx| cx.quit()),
                    ),
            )
            .into_any_element()
    }
    fn compact_segments(
        &self,
        id: &str,
        options: Vec<(String, String)>,
        selected: String,
        width: f32,
        cx: &mut Context<Self>,
        select: impl Fn(&mut Self, String, &mut Window, &mut Context<Self>) + Clone + 'static,
    ) -> Div {
        let p = Palette::new(cx);
        let mut row = div()
            .w(px(width))
            .h(px(20.))
            .rounded(px(4.))
            .bg(p.icon.opacity(0.7))
            .flex()
            .overflow_hidden();
        let count = options.len();
        for (index, (title, value)) in options.into_iter().enumerate() {
            let on = value == selected;
            let select = select.clone();
            row = row.child(
                div()
                    .id(SharedString::from(format!("{id}:{value}")))
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(11.))
                    .cursor_pointer()
                    // GPUI's overflow clip is rectangular; each end segment
                    // must round its own background to match the outer rail.
                    .when(index == 0, |segment| segment.rounded_l(px(4.)))
                    .when(index + 1 == count, |segment| segment.rounded_r(px(4.)))
                    .bg(if on { p.blue } else { transparent_black() })
                    .text_color(if on { rgb(0xffffff).into() } else { p.primary })
                    .child(title)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        select(this, value.clone(), window, cx)
                    })),
            );
        }
        row
    }
    fn compact_region_picker(&self, cx: &mut Context<Self>) -> Div {
        let p = Palette::new(cx);
        let region = self
            .session
            .borrow()
            .compact
            .region
            .clone()
            .unwrap_or_else(|| "cn".into());
        let name = region_name(&region);
        let bounds = Rc::new(Cell::new(Bounds::<Pixels>::default()));
        let measured = bounds.clone();
        let keyboard_bounds = bounds.clone();
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(12.))
            .py(px(9.))
            .child(self.compact_icon("server.rack", 24., p.primary))
            .child(label(l("服务器", "Server"), 13., p.primary).flex_1())
            .child(
                div()
                    .relative()
                    .w(px(118.))
                    .h(px(20.))
                    .child(
                        self.compact_menu_button("server-picker", name, p, false)
                            .custom(
                                ButtonCustomVariant::new(cx)
                                    .color(p.icon.opacity(0.7))
                                    .foreground(p.primary)
                                    .border(p.line)
                                    .hover(p.icon.opacity(0.85))
                                    .active(p.icon),
                            )
                            .w_full()
                            .h_full()
                            .on_key_down(cx.listener(
                                move |this, event: &KeyDownEvent, window, cx| {
                                    if ["enter", "space", "down"]
                                        .contains(&event.keystroke.key.as_str())
                                        && !event.keystroke.modifiers.modified()
                                    {
                                        window.prevent_default();
                                        cx.stop_propagation();
                                        this.compact_open_region_menu(
                                            keyboard_bounds.get(),
                                            window,
                                            cx,
                                        );
                                    }
                                },
                            ))
                            .on_key_up(|event, window, cx| {
                                if ["enter", "space", "down"]
                                    .contains(&event.keystroke.key.as_str())
                                {
                                    // Native tracking owns activation; don't synthesize
                                    // another GPUI click when the opening key is released.
                                    window.prevent_default();
                                    cx.stop_propagation();
                                }
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.compact_open_region_menu(bounds.get(), window, cx);
                            })),
                    )
                    .child(
                        canvas(move |bounds, _, _| measured.set(bounds), |_, _, _, _| {})
                            .absolute()
                            .inset_0(),
                    ),
            )
    }
    fn compact_open_region_menu(
        &self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let region = self
            .session
            .borrow()
            .compact
            .region
            .clone()
            .unwrap_or_else(|| "cn".into());
        let regions: Vec<String> = cloud::regions().iter().map(|s| (*s).into()).collect();
        let selected = regions.iter().position(|r| *r == region).unwrap_or(0);
        let titles = regions.iter().map(|r| region_name(r)).collect();
        let weak = cx.entity().downgrade();
        native_panel::show_popup_menu(
            window,
            bounds,
            titles,
            selected,
            Palette::new(cx).dark,
            cx,
            move |index, cx| {
                if let Some(region) = regions.get(index) {
                    let _ = weak.update(cx, |this, cx| {
                        this.region = region.clone();
                        this.session.borrow_mut().compact.region = Some(region.clone());
                        cx.notify();
                    });
                }
            },
        );
    }
}
fn region_name(region: &str) -> String {
    match region {
        "cn" => l("中国大陆", "Mainland China"),
        "de" => l("欧洲", "Europe"),
        "us" => l("美国", "United States"),
        "sg" => l("新加坡", "Singapore"),
        "ru" => l("俄罗斯", "Russia"),
        "i2" => l("印度", "India"),
        _ => region.into(),
    }
}
