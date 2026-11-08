use crate::{AddDevice, Assets, Dismiss, Refresh, Settings};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, Theme, ThemeMode,
    button::{Button, ButtonVariants},
    input::{Input, InputState},
    slider::{Slider, SliderEvent, SliderState, SliderValue},
    switch::Switch,
};
use mijialan_core::localization::text as t;
use mijialan_core::{DeviceState, Engine, Property, Token, cloud, discovery, engine::numeric};
use mijialan_homekit::HomeKit;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeMap, net::Ipv4Addr, rc::Rc, sync::Arc, time::Duration};
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod compact;

#[derive(Clone, PartialEq, Debug)]
enum Page {
    Devices,
    Detail(String),
    Add,
    Discover,
    Login,
    Bridge,
    Settings,
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    DevicesManagement,
}
pub struct UiSession {
    page: Page,
    room: Option<String>,
    drafts: BTreeMap<String, Value>,
    scrolls: BTreeMap<String, ScrollHandle>,
    pending: Option<Page>,
    login: Arc<mijialan_core::import_job::ImportJob>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    compact: compact::PanelSession,
}
impl Default for UiSession {
    fn default() -> Self {
        Self {
            page: Page::Devices,
            room: None,
            drafts: BTreeMap::new(),
            scrolls: BTreeMap::new(),
            pending: None,
            login: Arc::new(mijialan_core::import_job::ImportJob::default()),
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            compact: compact::PanelSession::default(),
        }
    }
}
impl UiSession {
    pub fn panel_height(&self) -> f32 {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.compact
                .height(self.pending.as_ref().unwrap_or(&self.page))
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            495.
        }
    }
    pub fn settings(&mut self) {
        self.pending = Some(Page::Settings);
    }
}
pub struct Dashboard {
    engine: Arc<Engine>,
    bridge: Arc<HomeKit>,
    assets: Assets,
    page: Page,
    room: Option<String>,
    name: Entity<InputState>,
    host: Entity<InputState>,
    token: Entity<InputState>,
    status: String,
    error: bool,
    busy: usize,
    found: Vec<discovery::DiscoveredDevice>,
    region: String,
    qr: Option<String>,
    qr_counter: u64,
    sliders: BTreeMap<String, Entity<SliderState>>,
    drafts: BTreeMap<String, Value>,
    subscriptions: Vec<Subscription>,
    focus: FocusHandle,
    session: Rc<RefCell<UiSession>>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    compact: compact::PanelState,
}
#[cfg_attr(
    any(target_os = "macos", target_os = "windows"),
    allow(
        dead_code,
        reason = "Also contains the Windows and Linux desktop presentation"
    )
)]
impl Dashboard {
    pub fn new(
        engine: Arc<Engine>,
        bridge: Arc<HomeKit>,
        assets: Assets,
        session: Rc<RefCell<UiSession>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window);
        #[allow(
            unused_mut,
            reason = "Only macOS installs a native popup activation observer"
        )]
        let mut subscriptions = Vec::new();
        #[cfg(target_os = "macos")]
        subscriptions.push(cx.observe_window_activation(window, |_, window, cx| {
            crate::trace_panel(if window.is_window_active() {
                "active"
            } else {
                "inactive"
            });
            if !window.is_window_active() && !crate::macos::is_tracking_menu() {
                let tray = cx
                    .has_global::<crate::TrayState>()
                    .then(|| &cx.global::<crate::TrayState>().0);
                if !crate::macos::pointer_over_tray(tray) {
                    window.remove_window();
                }
            }
        }));
        #[cfg(target_os = "windows")]
        subscriptions.push(cx.observe_window_activation(window, |_, window, cx| {
            if !window.is_window_active() && !crate::windows_panel::is_tracking_menu() {
                let tray = cx
                    .has_global::<crate::TrayState>()
                    .then(|| &cx.global::<crate::TrayState>().0);
                if !crate::windows_panel::pointer_over_tray(tray) {
                    crate::trace_panel("inactive_hide");
                    window.remove_window();
                }
            }
        }));
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        let saved = session.borrow();
        let page = saved.page.clone();
        let room = saved.room.clone();
        let drafts = saved.drafts.clone();
        drop(saved);
        #[allow(
            unused_mut,
            reason = "macOS restores compact form state after construction"
        )]
        let mut view = Self {
            engine,
            bridge,
            assets,
            page,
            room,
            name: cx.new(|cx| InputState::new(window, cx).placeholder("例如：书桌台灯")),
            host: cx.new(|cx| InputState::new(window, cx).placeholder("192.168.31.20")),
            token: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("32 位设备 Token")
                    .masked(true)
            }),
            status: "设备控制通过局域网完成".into(),
            error: false,
            busy: 0,
            found: vec![],
            region: "cn".into(),
            qr: None,
            qr_counter: 0,
            sliders: BTreeMap::new(),
            drafts,
            subscriptions,
            focus,
            session,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            compact: compact::PanelState::default(),
        };
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        view.compact_restore_manual(window, cx);
        view
    }
    fn go(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.page == Page::Login && page != Page::Login {
            self.qr = None;
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            self.session.borrow().login.cancel();
        }
        self.page = page;
        self.error = false;
        cx.notify();
    }
    fn run(
        &mut self,
        cx: &mut Context<Self>,
        job: impl FnOnce(Arc<Engine>) -> mijialan_core::Result<String> + Send + 'static,
    ) {
        self.busy += 1;
        self.error = false;
        self.status = "正在处理…".into();
        cx.notify();
        let engine = self.engine.clone();
        let task = cx.background_executor().spawn(async move { job(engine) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = this.busy.saturating_sub(1);
                match result {
                    Ok(message) => {
                        this.status = message;
                        this.error = false;
                    }
                    Err(error) => {
                        this.status = error.to_string();
                        this.error = true;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.run(cx, |engine| {
            engine.refresh_all();
            Ok("设备状态已刷新".into())
        });
    }
    fn write(&mut self, did: String, key: String, value: Value, cx: &mut Context<Self>) {
        self.drafts.remove(&format!("{did}:{key}"));
        self.run(cx, move |engine| {
            engine.write(&did, &key, value)?;
            let _ = engine.refresh(&did);
            Ok("设备已确认指令".into())
        });
    }
    fn add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().to_string();
        let host = self.host.read(cx).value().to_string();
        let token = self.token.read(cx).value().to_string();
        let parsed = (|| -> mijialan_core::Result<(Ipv4Addr, Token)> {
            let host = host
                .trim()
                .parse()
                .map_err(|_| mijialan_core::Error::Validation("请输入有效的 IPv4 地址".into()))?;
            Ok((host, Token::parse(&token)?))
        })();
        match parsed {
            Ok((host, token)) => {
                self.token
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.run(cx, move |engine| {
                    let device = engine.add(&name, host, token, 54321)?;
                    Ok(format!("已添加 {}", device.name))
                });
            }
            Err(error) => {
                self.status = error.to_string();
                self.error = true;
                cx.notify();
            }
        }
    }
    fn discover(&mut self, cx: &mut Context<Self>) {
        if self.engine.is_demo() {
            self.status = "演示模式不访问真实局域网".into();
            cx.notify();
            return;
        }
        self.busy += 1;
        self.status = "正在广播发现设备…".into();
        self.error = false;
        let engine = self.engine.clone();
        let task = cx.background_executor().spawn(async move {
            let devices = discovery::discover(Duration::from_secs(3))?;
            engine.update_discovered_hosts(&devices)?;
            Ok::<_, mijialan_core::Error>(devices)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = this.busy.saturating_sub(1);
                match result {
                    Ok(devices) => {
                        this.status = format!("发现 {} 台设备，已更新已知设备的 IP", devices.len());
                        this.found = devices;
                    }
                    Err(error) => {
                        this.status = error.to_string();
                        this.error = true;
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn login(&mut self, cx: &mut Context<Self>) {
        self.session
            .borrow()
            .login
            .start(self.engine.clone(), self.region.clone());
        self.status = "正在获取二维码…".into();
        self.error = false;
        cx.notify();
    }
    fn heading(&self, title: &str, subtitle: &str, cx: &App) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .mb_6()
            .when(cfg!(target_os = "macos"), |d| d.mb_3())
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t(title)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(subtitle)),
            )
    }
    fn sidebar(&self, cx: &mut Context<Self>) -> Div {
        let mut items = Vec::new();
        for (id, label, page) in [
            ("devices", "全部设备", Page::Devices),
            ("discover", "发现设备", Page::Discover),
            ("add", "添加设备", Page::Add),
            ("login", "扫码导入", Page::Login),
            ("bridge", "家庭桥接", Page::Bridge),
            ("settings", "设置", Page::Settings),
        ] {
            let selected = std::mem::discriminant(&self.page) == std::mem::discriminant(&page);
            items.push(
                div()
                    .id(id)
                    .w_full()
                    .px_4()
                    .py_3()
                    .rounded_lg()
                    .cursor_pointer()
                    .when(selected, |d| d.bg(cx.theme().accent))
                    .hover(|d| d.bg(cx.theme().accent))
                    .child(t(label))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.room = None;
                        this.go(page.clone(), cx);
                    }))
                    .into_any_element(),
            );
        }
        let mut rooms: Vec<_> = self
            .engine
            .states()
            .iter()
            .map(|s| s.device.room.clone())
            .filter(|r| !r.is_empty())
            .collect();
        rooms.sort();
        rooms.dedup();
        for room in rooms {
            let label = room.clone();
            items.push(
                div()
                    .id(SharedString::from(format!("room:{room}")))
                    .px_4()
                    .py_2()
                    .rounded_lg()
                    .cursor_pointer()
                    .hover(|d| d.bg(cx.theme().accent))
                    .child(room)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.room = Some(label.clone());
                        this.go(Page::Devices, cx);
                    }))
                    .into_any_element(),
            );
        }
        div()
            .w(px(208.))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .p_4()
            .gap_1()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .px_4()
                    .pt_4()
                    .pb_6()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .child(t("MijiaLAN")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t("Rust · 局域网设备控制")),
                    ),
            )
            .children(items)
            .child(div().flex_1())
            .child(
                div()
                    .px_4()
                    .py_3()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.engine.is_demo() {
                        "演示设备 · 不发送网络指令"
                    } else {
                        "本机连接 · 凭据安全保存"
                    }),
            )
    }
    fn devices(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let states: Vec<_> = self
            .engine
            .states()
            .into_iter()
            .filter(|s| self.room.as_ref().is_none_or(|r| *r == s.device.room))
            .collect();
        let online = states.iter().filter(|s| s.online).count();
        let title = self.room.clone().unwrap_or("我的设备".into());
        let mut grid = div().flex().flex_wrap().gap_4();
        for state in &states {
            let did = state.device.did.clone();
            let power = state
                .spec
                .properties
                .iter()
                .find(|p| p.name == "on" && p.writable);
            let is_on = power
                .and_then(|p| state.values.get(&p.key()))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let accent: Hsla = if is_on && state.online {
                rgb(0x22a693).into()
            } else {
                cx.theme().muted_foreground
            };
            let icon = match state.spec.kind.as_str() {
                "light" => "◉",
                "fan" => "✣",
                "air-purifier" => "≋",
                _ => "⏻",
            };
            let mut top = div()
                .flex()
                .items_center()
                .justify_between()
                .child(div().text_3xl().text_color(accent).child(icon));
            if let Some(p) = power {
                let key = p.key();
                let id = did.clone();
                top = top.child(
                    Switch::new(SharedString::from(format!("power:{did}")))
                        .checked(is_on)
                        .disabled(!state.online || self.busy > 0)
                        .on_click(cx.listener(move |this, on: &bool, _, cx| {
                            cx.stop_propagation();
                            this.write(id.clone(), key.clone(), json!(on), cx);
                        })),
                );
            }
            let reading_order: &[&str] = match state.spec.kind.as_str() {
                "light" => &["brightness", "color-temperature"],
                "fan" => &["speed-level", "fan-level"],
                "air-purifier" => &["pm2.5-density", "filter-life-level"],
                _ => &[
                    "temperature",
                    "relative-humidity",
                    "speed-level",
                    "brightness",
                ],
            };
            let reading = reading_order.iter().find_map(|name| {
                state
                    .spec
                    .properties
                    .iter()
                    .find(|p| p.name == *name && p.readable)
                    .and_then(|p| {
                        state
                            .values
                            .get(&p.key())
                            .map(|v| format!("{} {}", p.label(), p.display(v)))
                    })
            });
            let subtitle = if state.online {
                reading.unwrap_or("设备已连接".into())
            } else {
                state.error.clone().unwrap_or("等待连接".into())
            };
            grid = grid.child(
                div()
                    .id(SharedString::from(format!("card:{did}")))
                    .w(px(if cfg!(target_os = "macos") {
                        184.
                    } else {
                        240.
                    }))
                    .min_h(px(if cfg!(target_os = "macos") {
                        150.
                    } else {
                        178.
                    }))
                    .p_5()
                    .when(cfg!(target_os = "macos"), |d| d.p_4().gap_2())
                    .rounded_xl()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .cursor_pointer()
                    .hover(|d| d.border_color(rgb(0x22a693)))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .when(cfg!(target_os = "macos"), |d| d.gap_2())
                    .child(top)
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(state.device.name.clone()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .truncate()
                            .when(cfg!(target_os = "macos"), |d| d.text_xs())
                            .text_color(cx.theme().muted_foreground)
                            .child(subtitle),
                    )
                    .child(
                        div()
                            .text_xs()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{} · {}",
                                if state.device.room.is_empty() {
                                    "全屋"
                                } else {
                                    &state.device.room
                                },
                                state.device.host
                            )),
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.go(Page::Detail(did.clone()), cx)),
                    ),
            );
        }
        let mut rooms = std::collections::BTreeSet::new();
        for state in self.engine.states() {
            if !state.device.room.is_empty() {
                rooms.insert(state.device.room);
            }
        }
        let mut room_buttons = div().flex().flex_wrap().gap_2().mb_3();
        if !rooms.is_empty() {
            room_buttons = room_buttons.child(
                Button::new("room-all")
                    .label(t("全屋"))
                    .when(self.room.is_none(), |b| b.primary())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.room = None;
                        cx.notify();
                    })),
            );
            for room in rooms {
                let selected = self.room.as_ref() == Some(&room);
                let id = format!("room-chip:{room}");
                room_buttons = room_buttons.child(
                    Button::new(SharedString::from(id))
                        .label(room.clone())
                        .when(selected, |b| b.primary())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.room = Some(room.clone());
                            cx.notify();
                        })),
                );
            }
        }
        div()
            .flex()
            .flex_col()
            .child(self.heading(
                &title,
                &format!("{} 台设备 · {} 台在线", states.len(), online),
                cx,
            ))
            .when(cfg!(target_os = "macos"), |d| d.child(room_buttons))
            .when(states.is_empty(), |d| {
                d.child(
                    div()
                        .py_12()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(div().text_lg().child(t("把家里的设备连接起来")))
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child(t("用米家 App 扫码导入，或填写已有的设备 IP 和 Token。")),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_3()
                                .child(
                                    Button::new("empty-login")
                                        .primary()
                                        .label(t("扫码导入设备"))
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.go(Page::Login, cx)),
                                        ),
                                )
                                .child(Button::new("empty-add").label(t("手动添加")).on_click(
                                    cx.listener(|this, _, _, cx| this.go(Page::Add, cx)),
                                )),
                        ),
                )
            })
            .child(grid)
            .into_any_element()
    }
    fn property(
        &mut self,
        state: &DeviceState,
        p: &Property,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = p.key();
        let full = format!("{}:{key}", state.device.did);
        let value = self
            .drafts
            .get(&full)
            .or_else(|| state.values.get(&key))
            .cloned();
        let mut row = div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().child(p.label().to_owned()))
                    .child(div().text_color(cx.theme().muted_foreground).child(
                        value.as_ref().map(|v| p.display(v)).unwrap_or_else(|| {
                            t(state
                                .errors
                                .get(&key)
                                .map(String::as_str)
                                .unwrap_or("暂无读数"))
                        }),
                    )),
            );
        let writable = p.writable && state.online && self.busy == 0;
        if p.writable && p.format == "bool" {
            let did = state.device.did.clone();
            let key = key.clone();
            row = row.child(
                Switch::new(SharedString::from(format!("switch:{full}")))
                    .checked(value.as_ref().and_then(Value::as_bool).unwrap_or(false))
                    .disabled(!writable)
                    .on_click(cx.listener(move |this, on: &bool, _, cx| {
                        this.write(did.clone(), key.clone(), json!(on), cx)
                    })),
            );
        } else if p.writable && !p.choices.is_empty() {
            let mut choices = div().flex().flex_wrap().gap_2();
            for (index, choice) in p.choices.iter().enumerate() {
                let draft = full.clone();
                let v = choice.value.clone();
                let selected = value.as_ref() == Some(&v);
                choices = choices.child(
                    Button::new(SharedString::from(format!("{full}:choice:{index}")))
                        .label(if mijialan_core::localization::english() {
                            choice.description_en.clone()
                        } else {
                            choice.description.clone()
                        })
                        .when(selected, |b| b.primary())
                        .disabled(!writable)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.drafts.insert(draft.clone(), v.clone());
                            cx.notify();
                        })),
                );
            }
            row = row.child(choices);
        } else if p.writable
            && let Some([min, max, step]) = p.range.filter(|r| r[0] < r[1])
        {
            if !self.sliders.contains_key(&full) {
                let initial = value.as_ref().and_then(Value::as_f64).unwrap_or(min);
                let slider =
                    cx.new(|_| slider_model(min as f32, max as f32, step as f32, initial as f32));
                let draft = full.clone();
                let format = p.format.clone();
                self.subscriptions.push(cx.subscribe(
                    &slider,
                    move |this, _, event: &SliderEvent, cx| {
                        let SliderEvent::Change(SliderValue::Single(value)) = event else {
                            return;
                        };
                        this.drafts
                            .insert(draft.clone(), numeric(*value as f64, &format));
                        cx.notify();
                    },
                ));
                self.sliders.insert(full.clone(), slider);
                window.request_animation_frame();
            }
            let slider = self.sliders.get(&full).unwrap().clone();
            if !self.drafts.contains_key(&full)
                && let Some(value) = value.as_ref().and_then(Value::as_f64)
                && slider.read(cx).value() != SliderValue::Single(value as f32)
            {
                slider.update(cx, |slider, cx| slider.set_value(value as f32, window, cx));
            }
            row = row.child(Slider::new(&slider).disabled(!writable));
        }
        if p.format != "bool" && self.drafts.contains_key(&full) {
            let did = state.device.did.clone();
            let key = key.clone();
            let v = value.clone().unwrap_or(Value::Null);
            row = row.child(
                Button::new(SharedString::from(format!("apply:{full}")))
                    .primary()
                    .label(t("应用参数"))
                    .disabled(!writable)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.write(did.clone(), key.clone(), v.clone(), cx)
                    })),
            );
        }
        row.into_any_element()
    }
    fn detail(&mut self, did: &str, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.engine.state(did) else {
            return div().child(t("设备已移除")).into_any_element();
        };
        let mut content = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                Button::new("back")
                    .label(t("‹ 返回设备"))
                    .on_click(cx.listener(|this, _, _, cx| this.go(Page::Devices, cx))),
            )
            .child(self.heading(
                &state.device.name,
                &format!("{} · {}", state.device.model, state.device.host),
                cx,
            ));
        if let Some(error) = &state.error {
            content = content.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_4()
                    .rounded_lg()
                    .text_sm()
                    .bg(cx.theme().accent)
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(t("设备离线")))
                    .child(t(error)),
            );
        } else if state.spec.properties.is_empty() {
            content = content.child(div().child(t("正在获取设备规格…")));
        }
        for p in &state.spec.properties {
            if p.readable || p.writable {
                content = content.child(self.property(&state, p, window, cx));
            }
        }
        for action in &state.spec.actions {
            let did = did.to_string();
            let siid = action.siid;
            let aiid = action.aiid;
            let inputs: Option<Vec<_>> = action
                .inputs
                .iter()
                .map(|p| {
                    self.drafts
                        .get(&format!("{did}:{}", p.key()))
                        .or_else(|| state.values.get(&p.key()))
                        .cloned()
                })
                .collect();
            let available = inputs.is_some() && state.online && self.busy == 0;
            content = content.child(
                Button::new(SharedString::from(format!("action:{siid}.{aiid}")))
                    .label(action.label().to_owned())
                    .disabled(!available)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let did = did.clone();
                        let inputs = inputs.clone().unwrap_or_default();
                        this.run(cx, move |engine| {
                            engine.action(&did, siid, aiid, inputs)?;
                            Ok("设备已确认动作".into())
                        });
                    })),
            );
        }
        let id = did.to_string();
        content = content.child(
            div()
                .mt_6()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(t(
                            "移除设备也会从 HomeKit 桥中移除，家庭中的相关场景可能受影响。",
                        )),
                )
                .child(
                    Button::new("remove-device")
                        .label(t("移除设备"))
                        .disabled(self.busy > 0)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let id = id.clone();
                            this.run(cx, move |engine| {
                                engine.remove(&id)?;
                                Ok("设备已移除".into())
                            });
                            this.go(Page::Devices, cx);
                        })),
                ),
        );
        content.into_any_element()
    }
    fn add_page(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .max_w(px(560.))
            .flex()
            .flex_col()
            .gap_5()
            .child(self.heading("添加设备", "连接后验证设备身份，再将凭据保存到本机。", cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(t("设备名称"))
                    .child(Input::new(&self.name)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(t("设备 IP 地址"))
                    .child(Input::new(&self.host)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(t("设备 Token"))
                    .child(Input::new(&self.token)),
            )
            .child(
                Button::new("connect-add")
                    .primary()
                    .label(t("验证并添加设备"))
                    .disabled(self.busy > 0 || self.engine.is_demo())
                    .on_click(cx.listener(|this, _, window, cx| this.add(window, cx))),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(
                        "Token 可通过扫码导入获取。现代固件的局域网广播通常不会返回 Token。",
                    )),
            )
            .into_any_element()
    }
    fn discover_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut content = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(self.heading(
                "发现设备",
                "广播查找 miIO 设备，并按设备 ID 更新已知 IP。",
                cx,
            ))
            .child(
                Button::new("discover-start")
                    .primary()
                    .label(t("搜索局域网"))
                    .disabled(self.busy > 0)
                    .on_click(cx.listener(|this, _, _, cx| this.discover(cx))),
            );
        for device in &self.found {
            let host = device.host;
            content = content.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .p_4()
                    .rounded_lg()
                    .bg(cx.theme().accent)
                    .child(format!("{}    设备 ID {}", device.host, device.device_id))
                    .child(
                        Button::new(SharedString::from(format!("found:{}", device.device_id)))
                            .label(t("添加"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.host.update(cx, |input, cx| {
                                    input.set_value(host.to_string(), window, cx)
                                });
                                this.go(Page::Add, cx);
                            })),
                    ),
            );
        }
        content.into_any_element()
    }
    fn login_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut regions = div().flex().flex_wrap().gap_2();
        for region in cloud::regions() {
            let selected = self.region == *region;
            let r = region.to_string();
            regions = regions.child(
                Button::new(SharedString::from(format!("region:{r}")))
                    .label(if r == "cn" {
                        t("中国大陆")
                    } else {
                        r.to_uppercase()
                    })
                    .when(selected, |b| b.primary())
                    .disabled(self.session.borrow().login.busy())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.region = r.clone();
                        cx.notify();
                    })),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_5()
            .child(self.heading(
                "扫码导入设备",
                "使用米家 App 确认登录，导入设备凭据后即丢弃云端登录会话。",
                cx,
            ))
            .child(regions)
            .when_some(self.qr.clone(), |d, path| {
                d.child(
                    div()
                        .w(px(264.))
                        .h(px(264.))
                        .p_3()
                        .rounded_xl()
                        .bg(rgb(0xffffff))
                        .child(svg().path(path).size_full().text_color(rgb(0x000000))),
                )
            })
            .child(
                Button::new("login-start")
                    .primary()
                    .label(t(if self.qr.is_some() {
                        "等待手机确认…"
                    } else {
                        "获取登录二维码"
                    }))
                    .disabled(self.session.borrow().login.busy())
                    .on_click(cx.listener(|this, _, _, cx| this.login(cx))),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t("扫码和导入需要访问小米服务；日常设备控制只使用局域网。")),
            )
            .when(self.session.borrow().login.busy(), |d| {
                d.child(
                    Button::new("cancel-login")
                        .label(t("取消导入"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.session.borrow().login.cancel();
                            this.qr = None;
                            this.status = "导入已取消".into();
                            cx.notify();
                        })),
                )
            })
            .into_any_element()
    }
    fn bridge_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let info = self.bridge.info();
        let enabled = info.running;
        let mut content = div()
            .flex()
            .flex_col()
            .gap_5()
            .child(self.heading(
                "Apple 家庭桥接",
                "让家庭 App、Siri 和家居中枢通过这台电脑控制设备。",
                cx,
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .p_4()
                    .rounded_lg()
                    .bg(cx.theme().accent)
                    .child(t("启用 HomeKit 桥"))
                    .child(
                        Switch::new("bridge-enable")
                            .checked(enabled)
                            .disabled(self.busy > 0 || self.engine.is_demo())
                            .on_click(cx.listener(|this, on: &bool, _, cx| {
                                let on = *on;
                                let bridge = this.bridge.clone();
                                this.run(cx, move |engine| {
                                    if on {
                                        bridge.start().map_err(|e| {
                                            mijialan_core::Error::Validation(e.to_string())
                                        })?;
                                    } else {
                                        bridge.stop();
                                    }
                                    engine.store.update(|c| c.bridge_enabled = on)?;
                                    Ok(if on {
                                        "HomeKit 桥已启动"
                                    } else {
                                        "HomeKit 桥已停止"
                                    }
                                    .into())
                                });
                            })),
                    ),
            );
        content = content.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .p_4()
                .child(t("桥接时阻止闲置休眠"))
                .child(
                    Switch::new("keep-awake")
                        .checked(self.engine.store.config().keep_awake)
                        .on_click(cx.listener(|this, on: &bool, _, cx| {
                            let on = *on;
                            this.run(cx, move |engine| {
                                engine.store.update(|c| c.keep_awake = on)?;
                                Ok("闲置休眠设置已保存".into())
                            });
                        })),
                ),
        );
        if let Some(error) = info.error {
            content = content.child(div().text_color(cx.theme().danger).child(error));
        }
        if enabled {
            self.qr_counter += 1;
            let path = self.assets.qr(&info.uri, self.qr_counter);
            content = content.child(div().child(format!(
                "{} · 已桥接 {} 台设备 · 端口 {}",
                if info.paired {
                    "已配对"
                } else {
                    "等待配对"
                },
                info.accessories,
                info.port
            )));
            if !info.paired {
                content =
                    content
                        .when_some(path, |d, path| {
                            d.child(
                                div()
                                    .w(px(240.))
                                    .h(px(240.))
                                    .p_2()
                                    .bg(rgb(0xffffff))
                                    .rounded_xl()
                                    .child(svg().path(path).size_full().text_color(rgb(0x000000))),
                            )
                        })
                        .child(
                            div()
                                .text_2xl()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(info.code),
                        )
                        .child(div().text_sm().child(
                            "在 iPhone「家庭」中添加配件并扫描二维码，确认添加未认证配件。",
                        ));
            }
        }
        for state in self.engine.states() {
            let did = state.device.did.clone();
            let included = self
                .engine
                .store
                .config()
                .devices
                .iter()
                .find(|d| d.did == did)
                .is_some_and(|d| d.bridged);
            content = content.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .p_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(state.device.name)
                    .child(
                        Switch::new(SharedString::from(format!("bridge-device:{did}")))
                            .checked(included)
                            .disabled(self.busy > 0)
                            .on_click(cx.listener(move |this, on: &bool, _, cx| {
                                let id = did.clone();
                                let on = *on;
                                this.run(cx, move |engine| {
                                    engine.set_bridged(&id, on)?;
                                    let _ = engine.refresh(&id);
                                    Ok("桥接选择已保存".into())
                                });
                            })),
                    ),
            );
        }
        content
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(
                        "桥运行时需要电脑保持开机和网络连接。关闭窗口后继续在托盘后台运行。",
                    )),
            )
            .into_any_element()
    }
    fn settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut languages = div().flex().gap_2();
        for (code, label) in [("", "跟随系统"), ("zh-CN", "简体中文"), ("en", "English")] {
            let selected = self.engine.store.config().language == code;
            languages = languages.child(
                Button::new(SharedString::from(format!("lang:{code}")))
                    .label(t(label))
                    .when(selected, |b| b.primary())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let code = code.to_string();
                        mijialan_core::localization::configure(&code);
                        gpui_component::set_locale(if mijialan_core::localization::english() {
                            "en"
                        } else {
                            "zh-CN"
                        });
                        this.run(cx, move |engine| {
                            engine.store.update(|c| c.language = code)?;
                            Ok("语言设置已保存".into())
                        });
                    })),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_5()
            .child(self.heading("设置", "MijiaLAN Rust 0.1.0", cx))
            .child(languages)
            .child(
                Button::new("theme")
                    .label(t("切换浅色 / 深色外观"))
                    .on_click(cx.listener(|_, _, window, cx| {
                        let mode = if cx.theme().mode.is_dark() {
                            ThemeMode::Light
                        } else {
                            ThemeMode::Dark
                        };
                        Theme::change(mode, Some(window), cx);
                    })),
            )
            .child(div().text_sm().child(t(
                "凭据保存在本项目独立的系统凭据存储中。扫码登录会话不持久化。",
            )))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        self.engine
                            .store
                            .root()
                            .map(|p| format!("应用数据：{}", p.display()))
                            .unwrap_or("演示模式：只使用内存数据".into()),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .child(t("添加设备 ⌘N / Ctrl+N；刷新 ⌘R / F5；设置 ⌘, / Ctrl+,。")),
            )
            .into_any_element()
    }
    fn panel(
        &mut self,
        content: AnyElement,
        scroll: ScrollHandle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id("menubar-panel")
            .key_context("MijiaLAN")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Refresh, _, cx| this.refresh(cx)))
            .on_action(cx.listener(|this, _: &AddDevice, _, cx| this.go(Page::Add, cx)))
            .on_action(cx.listener(|this, _: &Settings, _, cx| this.go(Page::Settings, cx)))
            .on_action(cx.listener(|_, _: &Dismiss, window, _| {
                crate::trace_panel("dismiss");
                window.remove_window();
            }))
            .size_full()
            .flex()
            .flex_col()
            .rounded_2xl()
            .overflow_hidden()
            .bg(cx.theme().background.opacity(0.94))
            .text_color(cx.theme().foreground)
            .font_family(".AppleSystemUIFont")
            .text_sm()
            .child(
                div()
                    .h(px(58.))
                    .flex_shrink_0()
                    .px_4()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("panel-home")
                            .label("MijiaLAN")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.room = None;
                                this.go(Page::Devices, cx);
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("panel-refresh")
                                    .label(t("刷新"))
                                    .disabled(self.busy > 0)
                                    .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                            )
                            .child(Button::new("panel-settings").label(t("设置")).on_click(
                                cx.listener(|this, _, _, cx| this.go(Page::Settings, cx)),
                            )),
                    ),
            )
            .child(
                div()
                    .id("panel-scroll")
                    .track_scroll(&scroll)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .child(content),
            )
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .gap_2()
                    .p_3()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .children(
                        [
                            ("discover", "发现", Page::Discover),
                            ("add", "添加", Page::Add),
                            ("login", "扫码", Page::Login),
                            ("bridge", "家庭", Page::Bridge),
                        ]
                        .into_iter()
                        .map(|(id, label, page)| {
                            Button::new(id).label(t(label)).flex_1().on_click(
                                cx.listener(move |this, _, _, cx| this.go(page.clone(), cx)),
                            )
                        }),
                    ),
            )
            .child(
                div()
                    .min_h(px(38.))
                    .flex_shrink_0()
                    .px_4()
                    .py_2()
                    .text_xs()
                    .text_color(if self.error {
                        cx.theme().danger
                    } else {
                        cx.theme().muted_foreground
                    })
                    .child(t(if self.engine.is_demo() {
                        "演示设备 · 不发送网络指令"
                    } else {
                        &self.status
                    })),
            )
            .into_any_element()
    }
}
impl Render for Dashboard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pending = self.session.borrow_mut().pending.take();
        if let Some(page) = pending {
            self.page = page;
        }
        if let Some(result) = self.session.borrow().login.take_finished() {
            self.qr = None;
            match result {
                Ok(message) => {
                    self.status = message;
                    self.error = false;
                    if self.page == Page::Login {
                        self.page = Page::Devices;
                    }
                }
                Err(error) => {
                    self.status = error;
                    self.error = true;
                }
            }
        }
        match self.session.borrow().login.snapshot() {
            mijialan_core::import_job::Progress::Waiting(ticket) => {
                self.qr_counter += 1;
                self.qr = self.assets.qr(&ticket.login_url, self.qr_counter);
                if self.page == Page::Login {
                    self.status = "用米家 App 扫描二维码，并在手机上确认".into();
                }
            }
            mijialan_core::import_job::Progress::Fetching => {
                if self.page == Page::Login {
                    self.status = "正在获取二维码…".into();
                }
            }
            mijialan_core::import_job::Progress::Importing if self.page == Page::Login => {
                self.status = "正在导入设备…".into();
            }
            _ => {}
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            self.compact_render(window, cx)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let page = self.page.clone();
            let content = match page {
                Page::Devices => self.devices(cx),
                Page::Detail(did) => self.detail(&did, window, cx),
                Page::Add => self.add_page(cx),
                Page::Discover => self.discover_page(cx),
                Page::Login => self.login_page(cx),
                Page::Bridge => self.bridge_page(cx),
                Page::Settings => self.settings(cx),
                Page::DevicesManagement => self.devices(cx),
            };
            let scroll = self
                .session
                .borrow_mut()
                .scrolls
                .entry(format!("{:?}", self.page))
                .or_default()
                .clone();
            if cfg!(target_os = "macos") {
                return self.panel(content, scroll, cx);
            }
            div()
                .key_context("MijiaLAN")
                .track_focus(&self.focus)
                .on_action(cx.listener(|this, _: &Refresh, _, cx| this.refresh(cx)))
                .on_action(cx.listener(|this, _: &AddDevice, _, cx| this.go(Page::Add, cx)))
                .on_action(cx.listener(|this, _: &Settings, _, cx| this.go(Page::Settings, cx)))
                .size_full()
                .flex()
                .bg(cx.theme().background)
                .text_color(cx.theme().foreground)
                .font_family(if cfg!(target_os = "macos") {
                    ".AppleSystemUIFont"
                } else {
                    "sans-serif"
                })
                .text_sm()
                .child(self.sidebar(cx))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .h(px(60.))
                                .flex_shrink_0()
                                .px_8()
                                .flex()
                                .items_center()
                                .justify_between()
                                .border_b_1()
                                .border_color(cx.theme().border)
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(if self.engine.is_demo() {
                                            "演示模式"
                                        } else {
                                            "局域网直连"
                                        }),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .gap_2()
                                        .child(
                                            Button::new("refresh")
                                                .label(t("刷新"))
                                                .disabled(self.busy > 0)
                                                .on_click(
                                                    cx.listener(|this, _, _, cx| this.refresh(cx)),
                                                ),
                                        )
                                        .child(
                                            Button::new("top-add")
                                                .primary()
                                                .label(t("＋ 添加设备"))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.go(Page::Add, cx)
                                                })),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .id("content-scroll")
                                .track_scroll(&scroll)
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .p_8()
                                .child(content),
                        )
                        .child(
                            div()
                                .min_h(px(42.))
                                .flex_shrink_0()
                                .px_8()
                                .py_3()
                                .border_t_1()
                                .border_color(cx.theme().border)
                                .text_xs()
                                .text_color(if self.error {
                                    cx.theme().danger
                                } else {
                                    cx.theme().muted_foreground
                                })
                                .child(format!(
                                    "{}{}",
                                    if self.busy > 0 { "◌  " } else { "" },
                                    t(&self.status)
                                )),
                        ),
                )
                .into_any_element()
        }
    }
}
impl Drop for Dashboard {
    fn drop(&mut self) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        self.compact_cleanup_edits();
        crate::trace_panel("released");
        let mut saved = self.session.borrow_mut();
        saved.page = self.page.clone();
        saved.room = self.room.clone();
        saved.drafts = self.drafts.clone();
    }
}

fn slider_model(min: f32, max: f32, step: f32, value: f32) -> SliderState {
    // The widget clamps during each builder call. Widen its default 0..100 range before narrowing.
    SliderState::new()
        .max(max.max(100.))
        .min(min)
        .max(max)
        .step(step)
        .default_value(value.clamp(min, max))
}
#[cfg(test)]
mod tests {
    use super::slider_model;
    use gpui_component::slider::SliderValue;
    use mijialan_core::Engine;
    #[test]
    fn real_device_ranges_can_initialize_sliders() {
        for (min, max, step, value) in [
            (2700., 6500., 1., 4200.),
            (-50., -10., 0.5, -20.),
            (1., 100., 1., 50.),
        ] {
            let slider = slider_model(min, max, step, value);
            assert_eq!(slider.value(), SliderValue::Single(value));
        }
        for state in Engine::demo().states() {
            for property in state.spec.properties.iter().filter(|p| p.writable) {
                if let Some([min, max, step]) = property.range.filter(|r| r[0] < r[1]) {
                    let _ = slider_model(min as f32, max as f32, step as f32, min as f32);
                }
            }
        }
    }
}
