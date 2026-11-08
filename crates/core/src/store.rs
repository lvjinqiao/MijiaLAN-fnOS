use crate::{Error, Result, Token};
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, aead::Aead};
use directories::ProjectDirs;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    net::Ipv4Addr,
    path::{Path, PathBuf},
    sync::Mutex,
};
use zeroize::Zeroizing;

#[cfg(not(target_os = "linux"))]
const SERVICE: &str = "xyz.ipocket.MijiaLAN.Rust";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub did: String,
    pub name: String,
    pub model: String,
    pub host: Ipv4Addr,
    #[serde(default)]
    pub room: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "yes")]
    pub bridged: bool,
}
fn default_port() -> u16 {
    crate::packet::PORT
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub devices: Vec<Device>,
    #[serde(default)]
    pub bridge_enabled: bool,
    #[serde(default = "yes")]
    pub keep_awake: bool,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub appearance: String,
    #[serde(default)]
    pub homekit_names: BTreeMap<String, String>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            devices: vec![],
            bridge_enabled: false,
            keep_awake: true,
            language: String::new(),
            appearance: String::new(),
            homekit_names: BTreeMap::new(),
        }
    }
}

/// The OS vault stores only a 32-byte master key; authenticated ciphertext lives in the app directory.
/// Keeping the vault entry small also respects Windows Credential Manager's per-item size limit.
pub struct Vault {
    path: Option<PathBuf>,
    values: Mutex<Option<BTreeMap<String, String>>>,
}

impl Vault {
    pub fn memory() -> Self {
        Self {
            path: None,
            values: Mutex::new(Some(BTreeMap::new())),
        }
    }
    fn system(root: &Path) -> Self {
        Self {
            path: Some(root.join("credentials.enc")),
            values: Mutex::new(None),
        }
    }
    #[cfg(not(target_os = "linux"))]
    fn entry() -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, "master-key-v1")
            .map_err(|e| Error::Storage(format!("无法访问系统凭据存储：{e}")))
    }
    fn master_key(create: bool) -> Result<Zeroizing<[u8; 32]>> {
        if let Some(root) = std::env::var_os("MIJIALAN_DATA_DIR") {
            let path = PathBuf::from(root).join("master.key");
            match fs::read(&path) {
                Ok(bytes) => {
                    return Ok(Zeroizing::new(
                        bytes
                            .try_into()
                            .map_err(|_| Error::Storage("服务主密钥长度无效".into()))?,
                    ));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && create => {
                    let mut key = Zeroizing::new([0u8; 32]);
                    rand::thread_rng().fill_bytes(&mut *key);
                    let mut options = fs::OpenOptions::new();
                    options.write(true).create_new(true);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::OpenOptionsExt;
                        options.mode(0o600);
                    }
                    match options.open(&path) {
                        Ok(mut file) => {
                            file.write_all(&*key)?;
                            file.sync_all()?;
                            return Ok(key);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                            return Self::master_key(false);
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
        #[cfg(target_os = "linux")]
        return Err(Error::Storage(
            "Linux 服务需要设置 MIJIALAN_DATA_DIR".into(),
        ));
        #[cfg(not(target_os = "linux"))]
        {
            let entry = Self::entry()?;
            match entry.get_password() {
                Ok(text) => {
                    let text = Zeroizing::new(text);
                    let bytes = hex::decode(text.as_str())
                        .map_err(|_| Error::Storage("凭据主密钥无效，未覆盖原数据".into()))?;
                    Ok(Zeroizing::new(bytes.try_into().map_err(|_| {
                        Error::Storage("凭据主密钥长度无效".into())
                    })?))
                }
                Err(keyring::Error::NoEntry) if create => {
                    let mut key = Zeroizing::new([0; 32]);
                    rand::thread_rng().fill_bytes(&mut *key);
                    let encoded = Zeroizing::new(hex::encode(*key));
                    entry
                        .set_password(&encoded)
                        .map_err(|e| Error::Storage(format!("无法保存凭据主密钥：{e}")))?;
                    Ok(key)
                }
                Err(error) => Err(Error::Storage(format!("系统凭据存储不可用：{error}"))),
            }
        }
    }
    fn load(&self, values: &mut Option<BTreeMap<String, String>>) -> Result<()> {
        if values.is_some() {
            return Ok(());
        }
        let map = match fs::read(
            self.path
                .as_ref()
                .ok_or_else(|| Error::Storage("缺少凭据路径".into()))?,
        ) {
            Ok(data) => {
                if data.len() < 32 || &data[..4] != b"MLR1" {
                    return Err(Error::Storage("凭据文件损坏，未覆盖原数据".into()));
                }
                let key = Self::master_key(false)?;
                let plain = Zeroizing::new(
                    ChaCha20Poly1305::new((&*key).into())
                        .decrypt((&data[4..16]).into(), &data[16..])
                        .map_err(|_| Error::Storage("凭据文件认证失败，未覆盖原数据".into()))?,
                );
                serde_json::from_slice(&plain)
                    .map_err(|_| Error::Storage("系统凭据数据损坏，未覆盖原数据".into()))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(e.into()),
        };
        *values = Some(map);
        Ok(())
    }
    pub fn get(&self, key: &str) -> Result<Option<String>> {
        let mut values = self.values.lock().unwrap();
        self.load(&mut values)?;
        Ok(values.as_ref().unwrap().get(key).cloned())
    }
    pub fn set(&self, key: &str, value: Option<String>) -> Result<()> {
        let mut values = self.values.lock().unwrap();
        self.load(&mut values)?;
        let mut next = values.as_ref().unwrap().clone();
        if let Some(value) = value {
            next.insert(key.into(), value);
        } else {
            next.remove(key);
        }
        if let Some(path) = &self.path {
            let key = Self::master_key(true)?;
            let encoded = Zeroizing::new(
                serde_json::to_vec(&next).map_err(|_| Error::Storage("无法编码凭据".into()))?,
            );
            let mut nonce = [0; 12];
            rand::thread_rng().fill_bytes(&mut nonce);
            let cipher = ChaCha20Poly1305::new((&*key).into())
                .encrypt((&nonce).into(), encoded.as_slice())
                .map_err(|_| Error::Storage("无法加密凭据".into()))?;
            let mut data = b"MLR1".to_vec();
            data.extend_from_slice(&nonce);
            data.extend_from_slice(&cipher);
            atomic_write(path, &data)?;
        }
        *values = Some(next);
        Ok(())
    }
}

pub struct Store {
    pub vault: Vault,
    root: Option<PathBuf>,
    config: Mutex<Config>,
    memory_specs: Mutex<BTreeMap<String, String>>,
}

impl Store {
    pub fn open() -> Result<Self> {
        let dirs = ProjectDirs::from("xyz", "iPocket", "MijiaLAN Rust")
            .ok_or_else(|| Error::Storage("无法定位应用数据目录".into()))?;
        let root = std::env::var_os("MIJIALAN_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| dirs.config_dir().to_path_buf());
        fs::create_dir_all(&root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        }
        let path = root.join("devices.json");
        let config = match fs::read(&path) {
            Ok(data) => serde_json::from_slice(&data)
                .map_err(|_| Error::Storage("设备清单损坏，未覆盖原数据".into()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            vault: Vault::system(&root),
            root: Some(root),
            config: Mutex::new(config),
            memory_specs: Mutex::new(BTreeMap::new()),
        })
    }
    pub fn memory() -> Self {
        Self {
            vault: Vault::memory(),
            root: None,
            config: Mutex::new(Config::default()),
            memory_specs: Mutex::new(BTreeMap::new()),
        }
    }
    pub fn config(&self) -> Config {
        self.config.lock().unwrap().clone()
    }
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }
    pub fn token(&self, did: &str) -> Result<Token> {
        let text = Zeroizing::new(
            self.vault
                .get(&format!("token:{did}"))?
                .ok_or_else(|| Error::Storage("设备没有凭据，请重新导入".into()))?,
        );
        Token::parse(&text)
    }
    pub fn put(&self, device: Device, token: &Token) -> Result<()> {
        let mut config = self.config.lock().unwrap();
        let mut next = config.clone();
        if let Some(old) = next.devices.iter_mut().find(|d| d.did == device.did) {
            *old = device.clone();
        } else {
            next.devices.push(device.clone());
        }
        let key = format!("token:{}", device.did);
        let previous = self.vault.get(&key)?;
        self.vault.set(&key, Some(token.hex()))?;
        if let Err(e) = self.persist(&next) {
            let _ = self.vault.set(&key, previous);
            return Err(e);
        }
        *config = next;
        Ok(())
    }
    pub fn update(&self, change: impl FnOnce(&mut Config)) -> Result<()> {
        let mut config = self.config.lock().unwrap();
        let mut next = config.clone();
        change(&mut next);
        self.persist(&next)?;
        *config = next;
        Ok(())
    }
    pub fn remove(&self, did: &str) -> Result<()> {
        self.update(|c| c.devices.retain(|d| d.did != did))?;
        self.vault.set(&format!("token:{did}"), None)
    }
    fn persist(&self, config: &Config) -> Result<()> {
        let Some(root) = &self.root else {
            return Ok(());
        };
        let data = serde_json::to_vec_pretty(config)
            .map_err(|_| Error::Storage("无法编码设备清单".into()))?;
        atomic_write(&root.join("devices.json"), &data)
    }
    pub fn cached_spec(&self, model: &str) -> Option<String> {
        let name = safe_model(model)?;
        match &self.root {
            Some(root) => fs::read_to_string(root.join("specs").join(name)).ok(),
            None => self.memory_specs.lock().unwrap().get(&name).cloned(),
        }
    }
    pub fn cache_spec(&self, model: &str, text: &str) -> Result<()> {
        let name = safe_model(model).ok_or_else(|| Error::Spec("无效型号".into()))?;
        let Some(root) = &self.root else {
            self.memory_specs.lock().unwrap().insert(name, text.into());
            return Ok(());
        };
        let folder = root.join("specs");
        fs::create_dir_all(&folder)?;
        atomic_write(&folder.join(name), text.as_bytes())
    }
}

fn safe_model(model: &str) -> Option<String> {
    (!model.is_empty()
        && model.len() < 180
        && !model.starts_with('.')
        && model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-'))
    .then(|| format!("{model}.json"))
}

fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    temporary.write_all(data)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|e| Error::Storage(format!("无法保存文件：{}", e.error)))?;
    Ok(())
}
