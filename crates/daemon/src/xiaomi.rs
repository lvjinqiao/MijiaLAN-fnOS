use mijialan_core::{
    Device, Engine, Error, MiioClient, Result, Specification, Token, cloud::Session,
    engine::DeviceTransport, legacy::Profile,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    net::Ipv4Addr,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct CloudDevice {
    pub device: Device,
    pub spec: Specification,
    pub token: Option<String>,
    #[serde(default)]
    pub home_id: String,
    #[serde(default)]
    pub home_name: String,
}
pub struct CloudTransport {
    pub record: CloudDevice,
    pub session: Arc<RwLock<Option<Session>>>,
    route: Mutex<(bool, Instant)>,
}
impl CloudTransport {
    pub fn new(record: CloudDevice, session: Arc<RwLock<Option<Session>>>) -> Self {
        Self {
            record,
            session,
            route: Mutex::new((false, Instant::now() - Duration::from_secs(60))),
        }
    }
    fn client(&self) -> Result<MiioClient> {
        Ok(MiioClient::with_port(
            self.record.device.host,
            self.record.device.port,
            Token::parse(
                self.record
                    .token
                    .as_deref()
                    .ok_or_else(|| Error::Validation("没有本地 Token".into()))?,
            )?,
        ))
    }
    fn local_read(&self) -> Result<BTreeMap<String, Value>> {
        let mut client = self.client()?;
        if let Some(profile) = Profile::for_model(&self.record.device.model) {
            return profile.read(&mut client, &self.record.spec);
        }
        let props = self
            .record
            .spec
            .properties
            .iter()
            .filter(|p| p.readable)
            .map(|p| (p.siid, p.piid))
            .collect::<Vec<_>>();
        parse_properties(
            client.get_properties(&self.record.device.did, &props)?,
            &self.record.spec,
        )
    }
    fn session(&self) -> Result<Session> {
        self.session
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| Error::Validation("米家未登录，请扫码登录".into()))
    }
}
fn parse_properties(items: Vec<Value>, spec: &Specification) -> Result<BTreeMap<String, Value>> {
    let mut values = BTreeMap::new();
    for item in items {
        if item["code"].as_i64() == Some(0) {
            let key = format!("{}.{}", item["siid"], item["piid"]);
            if spec.properties.iter().any(|p| p.key() == key) && item.get("value").is_some() {
                values.insert(key, item["value"].clone());
            }
        }
    }
    if values.is_empty() {
        return Err(Error::Response(
            "未返回支持的属性；此型号可能需要旧 miIO 专用映射".into(),
        ));
    }
    Ok(values)
}
impl DeviceTransport for CloudTransport {
    fn poll_seconds(&self) -> u64 {
        30
    }
    fn action(&self, siid: u32, aiid: u32, inputs: Vec<Value>) -> Result<()> {
        self.session()?;
        let response = if self.route.lock().unwrap().0 {
            let mut client = self.client()?;
            if let Some(profile) = Profile::for_model(&self.record.device.model) {
                return profile.invoke(&mut client, siid, aiid);
            }
            client.call(
                "action",
                json!({"did":self.record.device.did,"siid":siid,"aiid":aiid,"in":inputs}),
            )?
        } else {
            self.session()?.request("miotspec/action",json!({"params":{"did":self.record.device.did,"siid":siid,"aiid":aiid,"in":inputs}}))?
        };
        if response["code"].as_i64() == Some(0) {
            Ok(())
        } else {
            Err(Error::Response("设备未确认动作成功".into()))
        }
    }
    fn read(&self) -> Result<BTreeMap<String, Value>> {
        self.session()?;
        let local_possible =
            !self.record.device.host.is_unspecified() && self.record.token.is_some();
        let (last_local, checked) = *self.route.lock().unwrap();
        if local_possible && (last_local || checked.elapsed() > Duration::from_secs(60)) {
            let result = self.local_read();
            *self.route.lock().unwrap() = (result.is_ok(), Instant::now());
            if let Ok(values) = result {
                return Ok(values);
            }
        }
        let params = self
            .record
            .spec
            .properties
            .iter()
            .filter(|p| p.readable)
            .map(|p| json!({"did":self.record.device.did,"siid":p.siid,"piid":p.piid}))
            .collect::<Vec<_>>();
        if params.is_empty() {
            return Err(Error::Spec("该型号暂无可读取的 MIoT 属性".into()));
        }
        let mut items = vec![];
        let session = self.session()?;
        for chunk in params.chunks(15) {
            let response = session.request("miotspec/prop/get", json!({"params":chunk}))?;
            items.extend(
                response
                    .as_array()
                    .cloned()
                    .ok_or_else(|| Error::Response("米家云属性响应格式无效".into()))?,
            );
        }
        parse_properties(items, &self.record.spec)
    }
    fn write(&self, key: &str, value: Value) -> Result<()> {
        self.session()?;
        let property = self
            .record
            .spec
            .properties
            .iter()
            .find(|p| p.key() == key)
            .ok_or_else(|| Error::Validation("属性不存在".into()))?;
        property.validate(&value)?;
        if self.route.lock().unwrap().0 {
            let mut client = self.client()?;
            // Do not repeat a possibly executed LAN write through cloud after a timeout.
            if let Some(profile) = Profile::for_model(&self.record.device.model) {
                return profile.write(&mut client, property, &value);
            }
            return client.set_property(
                &self.record.device.did,
                property.siid,
                property.piid,
                value,
            );
        }
        let response=self.session()?.request("miotspec/prop/set",json!({"params":[{"did":self.record.device.did,"siid":property.siid,"piid":property.piid,"value":value}]}))?;
        if response
            .as_array()
            .is_some_and(|a| a.len() == 1 && a[0]["code"].as_i64() == Some(0))
        {
            Ok(())
        } else {
            Err(Error::Response(
                "米家云未确认控制成功，请检查设备兼容性".into(),
            ))
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Home {
    pub id: String,
    pub name: String,
}
pub struct Snapshot {
    pub homes: Vec<Home>,
    pub devices: Vec<CloudDevice>,
}
fn id(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}
fn check_cancel(cancelled: &AtomicBool) -> Result<()> {
    if cancelled.load(Ordering::SeqCst) {
        Err(Error::Validation("同步已取消".into()))
    } else {
        Ok(())
    }
}
fn merge_device(devices: &mut BTreeMap<String, Value>, item: &Value) {
    if let Some(did) = id(&item["did"]) {
        let old = devices.entry(did).or_insert_with(|| json!({}));
        if let (Some(old), Some(item)) = (old.as_object_mut(), item.as_object()) {
            old.extend(item.clone());
        }
    }
}
fn room_assignments(document: &Value) -> BTreeMap<String, (String, String, String)> {
    let mut result = BTreeMap::new();
    for home in document["homelist"].as_array().into_iter().flatten() {
        let Some(hid) = id(&home["id"]) else { continue };
        let name = home["name"].as_str().unwrap_or("未命名家庭");
        for did in home["dids"].as_array().into_iter().flatten().filter_map(id) {
            result.insert(did, (hid.clone(), name.into(), String::new()));
        }
        for room in home["roomlist"].as_array().into_iter().flatten() {
            for did in room["dids"].as_array().into_iter().flatten().filter_map(id) {
                result.insert(
                    did,
                    (
                        hid.clone(),
                        name.into(),
                        room["name"].as_str().unwrap_or_default().into(),
                    ),
                );
            }
        }
    }
    result
}
pub fn fetch_devices(
    session: &Session,
    engine: &Engine,
    cancelled: &AtomicBool,
) -> Result<Snapshot> {
    fetch_with(engine, cancelled, |api, data| session.request(api, data))
}
fn fetch_with(
    engine: &Engine,
    cancelled: &AtomicBool,
    mut request: impl FnMut(&str, Value) -> Result<Value>,
) -> Result<Snapshot> {
    check_cancel(cancelled)?;
    let document = request(
        "v2/homeroom/gethome_merged",
        json!({"fg":true,"fetch_share":true,"fetch_share_dev":true,"fetch_cariot":true,"limit":300,"app_ver":7,"plat_form":0}),
    )?;
    let home_list = document["homelist"]
        .as_array()
        .ok_or_else(|| Error::Response("家庭信息缺少 homelist，保留已有家庭与设备".into()))?;
    let rooms = room_assignments(&document);
    let base = request(
        "home/device_list",
        json!({"getVirtualModel":true,"getHuamiDevices":1,"get_split_device":true,"support_smart_home":true}),
    )?;
    let list = base["list"]
        .as_array()
        .ok_or_else(|| Error::Response("设备清单缺少 list，保留原设备".into()))?;
    let mut all = BTreeMap::new();
    for item in list {
        merge_device(&mut all, item);
    }
    let mut homes = Vec::new();
    let mut membership = BTreeMap::new();
    for home in home_list {
        check_cancel(cancelled)?;
        let hid = id(&home["id"]).ok_or_else(|| Error::Response("家庭缺少 ID".into()))?;
        let name = home["name"].as_str().unwrap_or("未命名家庭").to_string();
        let mut cursor = String::new();
        let mut seen = std::collections::BTreeSet::new();
        loop {
            check_cancel(cancelled)?;
            let owner = id(&home["uid"])
                .and_then(|s| s.parse::<u64>().ok())
                .map(Value::from)
                .unwrap_or_else(|| home["uid"].clone());
            let home_id = hid
                .parse::<u64>()
                .map(Value::from)
                .unwrap_or_else(|_| json!(hid));
            let page = request(
                "v2/home/home_device_list",
                json!({"home_owner":owner,"home_id":home_id,"limit":300,"start_did":cursor,"get_split_device":true,"support_smart_home":true,"get_cariot_device":true,"get_third_device":true}),
            )?;
            let items = page["device_info"]
                .as_array()
                .ok_or_else(|| Error::Response(format!("家庭 {name} 的设备清单格式无效")))?;
            for item in items {
                if let Some(did) = id(&item["did"]) {
                    membership.insert(did, (hid.clone(), name.clone()));
                }
                merge_device(&mut all, item);
            }
            if !matches!(page["has_more"], Value::Bool(true))
                && page["has_more"].as_u64() != Some(1)
            {
                break;
            }
            let next = id(&page["max_did"])
                .ok_or_else(|| Error::Response("家庭设备分页缺少游标".into()))?;
            if !seen.insert(next.clone()) {
                return Err(Error::Response(
                    "家庭设备分页游标重复，未应用不完整的同步".into(),
                ));
            }
            cursor = next;
        }
        homes.push(Home { id: hid, name });
    }
    let previous = engine
        .store
        .vault
        .get("daemon:cloud-devices")?
        .map(|s| serde_json::from_str::<Vec<CloudDevice>>(&s))
        .transpose()
        .map_err(|_| Error::Storage("缓存设备格式无效".into()))?
        .unwrap_or_default();
    let mut devices = vec![];
    for (did, item) in all {
        check_cancel(cancelled)?;
        if did.starts_with("custom:") {
            continue;
        }
        let Some(model) = item["model"].as_str() else {
            continue;
        };
        let (home_id, home_name, room) = if let Some(location) = rooms.get(&did) {
            location.clone()
        } else if let Some((id, name)) = membership.get(&did) {
            (id.clone(), name.clone(), String::new())
        } else {
            (String::new(), String::new(), String::new())
        };
        let name = item["name"].as_str().unwrap_or(model).to_string();
        let host = item["localip"]
            .as_str()
            .and_then(|s| s.parse::<Ipv4Addr>().ok())
            .filter(|h| !h.is_multicast() && !h.is_broadcast())
            .unwrap_or(Ipv4Addr::UNSPECIFIED);
        let token = item["token"]
            .as_str()
            .filter(|s| Token::parse(s).is_ok())
            .map(String::from);
        let text = Specification::bundled(model)
            .map(String::from)
            .or_else(|| engine.store.cached_spec(model))
            .or_else(|| mijialan_core::spec::fetch(model).ok());
        let spec = if let Some(text) = text {
            let spec = Specification::for_model(model, &text).unwrap_or_default();
            engine.store.cache_spec(model, &text)?;
            spec
        } else {
            Specification::default()
        };
        let bridged = engine
            .state(&did)
            .map(|s| s.device.bridged)
            .unwrap_or_else(|| {
                previous
                    .iter()
                    .any(|r| r.device.did == did && r.device.bridged)
            });
        devices.push(CloudDevice {
            device: Device {
                did,
                name,
                model: model.into(),
                host,
                room,
                port: 54321,
                bridged,
            },
            spec,
            token,
            home_id,
            home_name,
        });
    }
    Ok(Snapshot { homes, devices })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn two_homes_shared_rooms_and_paginated_devices() {
        let engine = Engine::new(Arc::new(mijialan_core::Store::memory()), false);
        let cancel = AtomicBool::new(false);
        let mut pages = vec![];
        let result=fetch_with(&engine,&cancel,|api,data|Ok(match api{
 "v2/homeroom/gethome_merged"=>json!({"homelist":[{"id":1,"uid":9,"name":"家里","roomlist":[{"name":"客厅","dids":["a"]}]},{"id":"2","uid":"9","name":"父母家","roomlist":[{"name":"客厅","dids":["b"]}]}]}),
 "home/device_list"=>json!({"list":[{"did":"a","name":"灯 A","model":"yeelink.light.lamp4","token":"00112233445566778899aabbccddeeff"}]}),
 "v2/home/home_device_list"=>{pages.push(data.clone());if data["home_id"]==1 {json!({"device_info":[{"did":"a","name":"灯 A","model":"yeelink.light.lamp4"}],"has_more":false})}else if data["start_did"]=="" {json!({"device_info":[{"did":"b","name":"灯 B","model":"yeelink.light.lamp4"}],"has_more":true,"max_did":"b"})}else{json!({"device_info":[{"did":"c","name":"未分房间灯","model":"yeelink.light.lamp4"}],"has_more":false})}},_=>panic!("unexpected request")
 })).unwrap();
        assert_eq!(result.homes.len(), 2);
        assert_eq!(result.devices.len(), 3);
        assert_eq!(pages.len(), 3);
        assert_eq!(result.devices[0].home_name, "家里");
        assert!(result.devices[0].token.is_some());
        assert_eq!(result.devices[1].home_name, "父母家");
        assert_eq!(result.devices[1].device.room, "客厅");
        assert_eq!(result.devices[2].home_id, "2");
    }
    #[test]
    fn many_homes_with_duplicate_names_keep_distinct_ids() {
        let engine = Engine::new(Arc::new(mijialan_core::Store::memory()), false);
        let homes: Vec<Value> = (1..=7).map(|id| json!({"id":id,"uid":9,"name":"我的家","roomlist":[{"name":"客厅","dids":[format!("lamp-{id}")]}]})).collect();
        let mut visited = Vec::new();
        let result = fetch_with(&engine, &AtomicBool::new(false), |api, data| Ok(match api {
            "v2/homeroom/gethome_merged" => json!({"homelist":homes}),
            "home/device_list" => json!({"list":[]}),
            "v2/home/home_device_list" => {
                let id = data["home_id"].as_u64().unwrap();
                visited.push(id);
                json!({"device_info":[{"did":format!("lamp-{id}"),"name":"灯","model":"yeelink.light.lamp4"}],"has_more":false})
            },
            _ => panic!("unexpected API"),
        })).unwrap();
        assert_eq!(visited, (1..=7).collect::<Vec<_>>());
        assert_eq!(result.homes.len(), 7);
        assert_eq!(result.devices.len(), 7);
        for device in result.devices {
            assert_eq!(device.device.did, format!("lamp-{}", device.home_id));
            assert_eq!(device.home_name, "我的家");
            assert_eq!(device.device.room, "客厅");
        }
    }
    #[test]
    fn missing_home_data_and_cancel_are_errors() {
        let engine = Engine::new(Arc::new(mijialan_core::Store::memory()), false);
        assert!(fetch_with(&engine, &AtomicBool::new(false), |_, _| Ok(json!({}))).is_err());
        assert!(
            fetch_with(&engine, &AtomicBool::new(true), |_, _| panic!(
                "cancel must precede network"
            ))
            .is_err()
        );
    }
}
