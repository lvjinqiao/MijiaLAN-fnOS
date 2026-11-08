use anyhow::{Result, bail};
use mijialan_core::Engine;
use mijialan_homekit::HomeKit;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

pub struct DeviceBridges {
    engine: Arc<Engine>,
    bridges: Mutex<BTreeMap<String, HomeKit>>,
}
impl DeviceBridges {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            bridges: Mutex::new(BTreeMap::new()),
        }
    }
    pub fn enable(&self, did: &str) -> Result<()> {
        if self.engine.state(did).is_none() {
            bail!("设备不存在");
        }
        let mut bridges = self.bridges.lock().unwrap();
        let bridge = bridges
            .entry(did.into())
            .or_insert_with(|| HomeKit::for_device(self.engine.clone(), did));
        bridge.start()?;
        if bridge.info().accessories == 0 {
            bridge.stop();
            bail!("此设备暂无可正确映射的 HomeKit 功能，请在管理界面控制");
        }
        Ok(())
    }
    pub fn disable(&self, did: &str) {
        self.bridges.lock().unwrap().remove(did);
    }
    pub fn start(&self) -> Result<()> {
        for state in self.engine.states() {
            if state.device.bridged {
                self.enable(&state.device.did)?;
            }
        }
        Ok(())
    }
    pub fn stop(&self) {
        self.bridges.lock().unwrap().clear();
    }
    pub fn reset_pairing(&self, did: &str) -> Result<()> {
        let bridges = self.bridges.lock().unwrap();
        let Some(bridge) = bridges.get(did) else {
            bail!("请先开启此设备的 HomeKit 配对");
        };
        bridge.reset_pairing()
    }
    pub fn info(&self) -> BTreeMap<String, Value> {
        self.bridges.lock().unwrap().iter().map(|(did, bridge)| {
            let i = bridge.info();
            (did.clone(), json!({"running":i.running,"paired":i.paired,"code":i.code,"uri":i.uri,"port":i.port,"accessories":i.accessories,"error":i.error,"qr_svg":crate::qr(&i.uri).unwrap_or_default()}))
        }).collect()
    }
}
