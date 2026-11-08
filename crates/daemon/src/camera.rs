//! Native go2rtc camera processes: one identity/configuration per RTSP camera.
use anyhow::{Context, Result, bail};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha512};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::{
    collections::BTreeMap,
    fs,
    net::{TcpListener, UdpSocket},
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Camera {
    pub id: String,
    pub name: String,
    pub url: String,
    pub enabled: bool,
}
pub struct Cameras {
    checked: std::collections::BTreeSet<String>,
    live: Vec<std::sync::Arc<std::sync::Mutex<Child>>>,
    root: PathBuf,
    binary: PathBuf,
    ffmpeg: PathBuf,
    children: BTreeMap<String, Child>,
    errors: BTreeMap<String, String>,
}
impl Cameras {
    pub fn new() -> Result<Self> {
        let root = PathBuf::from(std::env::var("MIJIALAN_DATA_DIR")?).join("cameras");
        fs::create_dir_all(&root)?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        let binary = std::env::var_os("MIJIALAN_CAMERA_BIN")
            .map(PathBuf::from)
            .unwrap_or(std::env::current_exe()?.with_file_name("go2rtc"));
        let ffmpeg = std::env::var_os("MIJIALAN_FFMPEG_BIN")
            .map(PathBuf::from)
            .unwrap_or(std::env::current_exe()?.with_file_name("ffmpeg"));
        let mut this = Self {
            checked: Default::default(),
            live: Vec::new(),
            root,
            binary,
            ffmpeg,
            children: BTreeMap::new(),
            errors: BTreeMap::new(),
        };
        for c in this.load()? {
            if c.enabled {
                if let Err(e) = this.start(&c.id) {
                    this.errors.insert(c.id, e.to_string());
                }
            }
        }
        Ok(this)
    }
    fn load(&self) -> Result<Vec<Camera>> {
        let p = self.root.join("devices.json");
        if !p.exists() {
            return Ok(vec![]);
        }
        Ok(serde_json::from_slice(&fs::read(p)?)?)
    }
    fn file(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.yaml"))
    }
    fn config(&self, id: &str) -> Result<Value> {
        Ok(serde_yaml::from_slice(&fs::read(self.file(id))?)?)
    }
    pub fn list(&mut self) -> Result<Value> {
        let mut list = vec![];
        for c in self.load()? {
            let running = match self.children.get_mut(&c.id) {
                Some(child) => child.try_wait()?.is_none(),
                None => false,
            };
            let conf = self.config(&c.id)?;
            let paired = conf["homekit"][&c.id]["pairings"]
                .as_array()
                .is_some_and(|p| !p.is_empty());
            list.push(json!({"id":c.id,"name":c.name,"enabled":c.enabled,"running":running,"paired":paired,"error":self.errors.get(&c.id),"has_url":true}));
        }
        Ok(
            json!({"available":self.binary.is_file(),"snapshot_available":self.ffmpeg.is_file(),"devices":list}),
        )
    }
    pub fn save(&mut self, body: Value) -> Result<Value> {
        let id = body["id"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("camera-{:016x}", rand::thread_rng().r#gen::<u64>()));
        validate_id(&id)?;
        let mut all = self.load()?;
        let old = all.iter().find(|c| c.id == id);
        let name = body["name"].as_str().unwrap_or("").trim().to_string();
        if name.is_empty() || name.chars().count() > 64 {
            bail!("摄像头名称需为 1–64 个字符")
        }
        let url = body["url"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| old.map(|c| c.url.clone()))
            .context("请填写 RTSP 地址")?;
        validate_url(&url)?;
        let enabled = body["enabled"].as_bool().unwrap_or(false);
        if enabled && !self.binary.is_file() {
            bail!("原生视频组件尚未安装，无法开启配对")
        }
        if enabled {
            self.check_video(&url)?;
            self.checked.insert(id.clone());
        }
        let mut config = if self.file(&id).exists() {
            self.config(&id)?
        } else {
            let listener = TcpListener::bind("0.0.0.0:0")?;
            let port = listener.local_addr()?.port();
            let udp = UdpSocket::bind("0.0.0.0:0")?;
            let media_port = udp.local_addr()?.port();
            let seed = format!(
                "{:032x}{:032x}",
                rand::thread_rng().r#gen::<u128>(),
                rand::thread_rng().r#gen::<u128>()
            );
            let pin = format!("{:08}", rand::thread_rng().gen_range(20000000..80000000u32));
            json!({"api":{"listen":format!(":{port}"),"allow_paths":["/pair-setup","/pair-verify"]},"rtsp":{"listen":""},"webrtc":{"listen":""},"srtp":{"listen":format!(":{media_port}")},"log":{"level":"error"},"streams":{},"homekit":{id.clone():{"pin":pin,"device_id":(0..6).map(|_|format!("{:02X}",rand::thread_rng().r#gen::<u8>())).collect::<Vec<_>>().join(":"),"device_private":seed}}})
        };
        config["ffmpeg"] = json!({"bin":self.ffmpeg.to_string_lossy()});
        config["streams"][&id] = json!(url);
        config["homekit"][&id]["name"] = json!(name);
        self.stop_one(&id);
        // Reload pairings after stopping the process, so editing cannot erase a concurrent pairing.
        if self.file(&id).exists() {
            config["homekit"][&id]["pairings"] =
                self.config(&id)?["homekit"][&id]["pairings"].clone();
        }
        write_private(&self.file(&id), serde_yaml::to_string(&config)?.as_bytes())?;
        all.retain(|c| c.id != id);
        all.push(Camera {
            id: id.clone(),
            name,
            url,
            enabled,
        });
        write_private(&self.root.join("devices.json"), &serde_json::to_vec(&all)?)?;
        if enabled {
            if let Err(e) = self.start(&id) {
                self.errors.insert(id.clone(), e.to_string());
                return Err(e);
            }
        }
        Ok(json!({"ok":true,"id":id}))
    }
    fn start(&mut self, id: &str) -> Result<()> {
        let child = Command::new(&self.binary)
            .arg("-config")
            .arg(self.file(id))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("视频组件启动失败")?;
        self.children.insert(id.into(), child);
        std::thread::sleep(Duration::from_millis(150));
        if self.children.get_mut(id).unwrap().try_wait()?.is_some() {
            bail!("视频组件已退出，请检查端口及运行环境")
        }
        self.errors.remove(id);
        Ok(())
    }
    pub fn pairing(&mut self, id: &str) -> Result<Value> {
        validate_id(id)?;
        let c = self
            .load()?
            .into_iter()
            .find(|c| c.id == id)
            .context("摄像头不存在")?;
        if !c.enabled {
            bail!("请先启用此摄像头的 HomeKit 配对")
        }
        if !self
            .children
            .get_mut(id)
            .is_some_and(|p| p.try_wait().ok() == Some(None))
        {
            bail!("视频组件未运行")
        }
        if !self.checked.contains(id) {
            self.check_video(&c.url)?;
            self.checked.insert(id.into());
        }
        let conf = self.config(id)?;
        let pin = conf["homekit"][id]["pin"].as_str().context("配对码无效")?;
        let hash = Sha512::digest(id.as_bytes());
        let setup = format!("{:02X}{:02X}", hash[44], hash[46]);
        let mut number = (17u64 << 31) | (2 << 27) | pin.parse::<u64>()?;
        let mut digits = String::new();
        while number > 0 {
            digits.insert(
                0,
                b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"[(number % 36) as usize] as char,
            );
            number /= 36;
        }
        let uri = format!("X-HM://{:0>9}{setup}", digits);
        Ok(
            json!({"name":c.name,"code":format!("{}-{}-{}",&pin[..3],&pin[3..5],&pin[5..]),"qr_svg":crate::qr(&uri)?,"paired":conf["homekit"][id]["pairings"].as_array().is_some_and(|p|!p.is_empty())}),
        )
    }
    pub fn reset(&mut self, id: &str) -> Result<Value> {
        validate_id(id)?;
        let c = self
            .load()?
            .into_iter()
            .find(|c| c.id == id)
            .context("摄像头不存在")?;
        self.stop_one(id);
        let mut conf = self.config(id)?;
        conf["homekit"][id]["pairings"] = json!([]);
        write_private(&self.file(id), serde_yaml::to_string(&conf)?.as_bytes())?;
        if c.enabled {
            self.start(id)?;
        }
        Ok(json!({"ok":true}))
    }
    fn check_video(&self, url: &str) -> Result<()> {
        if !self.ffmpeg.is_file() {
            bail!("缺少视频检测组件，无法确认 HomeKit 兼容性")
        }
        let path = self.root.join(format!(
            "probe-{:016x}.log",
            rand::thread_rng().r#gen::<u64>()
        ));
        let log = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&path)?;
        let mut child = Command::new(&self.ffmpeg)
            .args([
                "-hide_banner",
                "-nostdin",
                "-nostats",
                "-rtsp_transport",
                "tcp",
                "-timeout",
                "5000000",
                "-i",
                url,
                "-map",
                "0:v:0",
                "-t",
                "0",
                "-f",
                "null",
                "-",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()?;
        let started = std::time::Instant::now();
        let result = (|| -> Result<()> {
            loop {
                if let Some(status) = child.try_wait()? {
                    let text = fs::read_to_string(&path)?;
                    if !status.success() {
                        bail!("无法读取视频流，请检查 RTSP 地址、密码和网络；未开始 HomeKit 配对")
                    }
                    if !text.contains("Video: h264") {
                        bail!(
                            "视频不是 H.264，当前不能直接配对到 HomeKit。请在摄像头中选择 H.264 视频流"
                        )
                    }
                    return Ok(());
                }
                if started.elapsed() > Duration::from_secs(10) {
                    bail!("视频检测超时，请检查摄像头网络；未开始 HomeKit 配对")
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        })();
        let _ = child.kill();
        let _ = child.wait();
        let _ = fs::remove_file(path);
        result
    }
    pub fn live(&mut self, id: &str) -> Result<LivePreview> {
        validate_id(id)?;
        self.live
            .retain(|p| p.lock().unwrap().try_wait().ok() == Some(None));
        if self.live.len() >= 2 {
            bail!("请先关闭一个视频预览窗口")
        }
        let camera = self
            .load()?
            .into_iter()
            .find(|c| c.id == id)
            .context("摄像头不存在")?;
        if !self.ffmpeg.is_file() {
            bail!("缺少视频预览组件")
        }
        let mut child = Command::new(&self.ffmpeg)
            .args([
                "-hide_banner",
                "-nostdin",
                "-loglevel",
                "error",
                "-rtsp_transport",
                "tcp",
                "-timeout",
                "5000000",
                "-i",
                &camera.url,
                "-map",
                "0:v:0",
                "-an",
                "-vf",
                "scale=640:-2",
                "-r",
                "15",
                "-c:v",
                "libx264",
                "-profile:v",
                "baseline",
                "-level:v",
                "3.0",
                "-pix_fmt",
                "yuv420p",
                "-preset",
                "ultrafast",
                "-tune",
                "zerolatency",
                "-g",
                "15",
                "-movflags",
                "frag_keyframe+empty_moov+default_base_moof",
                "-f",
                "mp4",
                "pipe:1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("视频预览启动失败")?;
        let output = child.stdout.take().context("视频输出不可用")?;
        let process = std::sync::Arc::new(std::sync::Mutex::new(child));
        self.live.push(process.clone());
        Ok(LivePreview { output, process })
    }
    pub fn snapshot(&self, id: &str) -> Result<Value> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        validate_id(id)?;
        let c = self
            .load()?
            .into_iter()
            .find(|c| c.id == id)
            .context("摄像头不存在")?;
        if !self.ffmpeg.is_file() {
            bail!("未安装视频预览组件")
        }
        let path = self.root.join(format!(
            "preview-{:016x}.jpg",
            rand::thread_rng().r#gen::<u64>()
        ));
        let mut child = Command::new(&self.ffmpeg)
            .args([
                "-hide_banner",
                "-nostdin",
                "-loglevel",
                "error",
                "-rtsp_transport",
                "tcp",
                "-i",
                &c.url,
                "-map",
                "0:v:0",
                "-frames:v",
                "1",
                "-vf",
                "scale=640:-2",
                "-y",
            ])
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("预览组件启动失败")?;
        let start = std::time::Instant::now();
        let result = (|| -> Result<Value> {
            loop {
                if let Some(status) = child.try_wait()? {
                    if !status.success() {
                        bail!("无法读取画面，请检查地址、账号密码和摄像头网络")
                    }
                    let data = fs::read(&path)?;
                    if data.len() > 4 * 1024 * 1024 || !data.starts_with(&[255, 216]) {
                        bail!("没有取得有效画面")
                    }
                    return Ok(
                        json!({"image":"data:image/jpeg;base64,".to_string()+&STANDARD.encode(data)}),
                    );
                }
                if start.elapsed() > Duration::from_secs(15) {
                    bail!("读取画面超时，请检查 RTSP 地址和设备网络")
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        })();
        let _ = child.kill();
        let _ = child.wait();
        let _ = fs::remove_file(path);
        result
    }
    pub fn delete(&mut self, id: &str) -> Result<Value> {
        validate_id(id)?;
        self.stop_one(id);
        let mut all = self.load()?;
        all.retain(|c| c.id != id);
        write_private(&self.root.join("devices.json"), &serde_json::to_vec(&all)?)?;
        if self.file(id).exists() {
            fs::remove_file(self.file(id))?;
        }
        Ok(json!({"ok":true}))
    }
    fn stop_one(&mut self, id: &str) {
        if let Some(mut p) = self.children.remove(id) {
            let _ = p.kill();
            let _ = p.wait();
        }
        self.errors.remove(id);
    }
    pub fn stop(&mut self) {
        for child in self.live.drain(..) {
            let mut p = child.lock().unwrap();
            let _ = p.kill();
            let _ = p.wait();
        }
        for id in self.children.keys().cloned().collect::<Vec<_>>() {
            self.stop_one(&id)
        }
    }
}
impl Drop for Cameras {
    fn drop(&mut self) {
        self.stop()
    }
}
fn write_private(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    let mut f = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(tmp, path)?;
    Ok(())
}
fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 80
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        bail!("摄像头 ID 无效")
    }
    Ok(())
}
fn validate_url(url: &str) -> Result<()> {
    let u = reqwest::Url::parse(url).context("RTSP 地址无效")?;
    if !matches!(u.scheme(), "rtsp" | "rtsps")
        || u.host_str().is_none()
        || u.fragment().is_some()
        || url.len() > 4096
    {
        bail!("只接受完整的 rtsp:// 或 rtsps:// 地址")
    };
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rtsp_only() {
        assert!(validate_url("rtsp://user:pass@192.168.1.2/live").is_ok());
        for s in [
            "exec:touch /tmp/a",
            "ffmpeg:test",
            "http://example.com",
            "rtsp://host/live#video=h264",
        ] {
            assert!(validate_url(s).is_err());
        }
        assert!(validate_id("../camera").is_err());
    }
}

pub struct LivePreview {
    output: std::process::ChildStdout,
    process: std::sync::Arc<std::sync::Mutex<Child>>,
}
impl std::io::Read for LivePreview {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.output, buffer)
    }
}
impl Drop for LivePreview {
    fn drop(&mut self) {
        let mut p = self.process.lock().unwrap();
        let _ = p.kill();
        let _ = p.wait();
    }
}
