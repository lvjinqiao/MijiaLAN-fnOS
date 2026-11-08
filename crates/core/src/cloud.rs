//! Xiaomi QR login and encrypted cloud requests. Daemon sessions persist only in the encrypted vault.
use crate::{Device, Engine, Error, Result, Token};
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use rand::RngCore;
use reqwest::{Url, blocking::Client, redirect::Policy};
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use sha2::Sha256;
use std::{
    net::Ipv4Addr,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone)]
pub struct Ticket {
    pub login_url: String,
    pub poll_url: String,
    pub timeout: Duration,
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct Session {
    user_id: String,
    ssecurity: String,
    service_token: String,
    region: String,
}

pub fn regions() -> &'static [&'static str] {
    &["cn", "de", "us", "sg", "ru", "i2"]
}
fn http() -> Result<Client> {
    Client::builder()
        .timeout(Duration::from_secs(16))
        .redirect(Policy::none())
        .build()
        .map_err(|_| Error::Response("无法初始化登录网络服务".into()))
}
fn account_url(text: &str) -> Result<Url> {
    let url = Url::parse(text).map_err(|_| Error::Response("无效的小米登录地址".into()))?;
    let host = url.host_str().unwrap_or_default();
    if url.scheme() != "https"
        || !(host == "xiaomi.com"
            || host.ends_with(".xiaomi.com")
            || host == "mi.com"
            || host.ends_with(".mi.com"))
    {
        return Err(Error::Response("登录服务返回了不受信任的地址".into()));
    }
    Ok(url)
}
pub fn parse_account_json(text: &str) -> Result<Value> {
    serde_json::from_str(
        text.trim()
            .strip_prefix("&&&START&&&")
            .unwrap_or(text.trim()),
    )
    .map_err(|_| Error::Response("小米账号服务返回了无效数据".into()))
}
pub fn start() -> Result<Ticket> {
    let response = http()?
        .get("https://account.xiaomi.com/longPolling/loginUrl")
        .query(&[
            ("_qrsize", "480".to_string()),
            ("qs", "%3Fsid%3Dxiaomiio%26_json%3Dtrue".into()),
            ("callback", "https://sts.api.io.mi.com/sts".into()),
            ("_hasLogo", "false".into()),
            ("sid", "xiaomiio".into()),
            ("serviceParam", String::new()),
            ("_locale", "zh_CN".into()),
            (
                "_dc",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    .to_string(),
            ),
        ])
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| Error::Response(format!("无法获取小米登录二维码：{}", e.without_url())))?;
    let body = response
        .text()
        .map_err(|_| Error::Response("无法读取二维码响应".into()))?;
    let value = parse_account_json(&body)?;
    let login_url = value["loginUrl"]
        .as_str()
        .ok_or_else(|| Error::Response("二维码响应缺少登录地址".into()))?
        .to_string();
    let poll_url = value["lp"]
        .as_str()
        .ok_or_else(|| Error::Response("二维码响应缺少轮询地址".into()))?
        .to_string();
    account_url(&poll_url)?;
    Ok(Ticket {
        login_url,
        poll_url,
        timeout: Duration::from_secs(value["timeout"].as_u64().unwrap_or(300).clamp(1, 600)),
    })
}
pub fn wait(ticket: &Ticket, region: &str, cancelled: &AtomicBool) -> Result<Session> {
    if !regions().contains(&region) {
        return Err(Error::Validation("未知服务器地区".into()));
    }
    let http = http()?;
    let end = Instant::now() + ticket.timeout;
    while !cancelled.load(Ordering::Relaxed) && Instant::now() < end {
        let response = match http.get(account_url(&ticket.poll_url)?).send() {
            Err(e) if e.is_timeout() => continue,
            Err(_) => {
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
            Ok(response) => response,
        };
        if !response.status().is_success() {
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }
        let body = response
            .text()
            .map_err(|_| Error::Response("无法读取扫码结果".into()))?;
        let value = parse_account_json(&body)?;
        let (Some(user), Some(security), Some(location)) = (
            value.get("userId"),
            value["ssecurity"].as_str(),
            value["location"].as_str(),
        ) else {
            continue;
        };
        let mut url = account_url(location)?;
        for _ in 0..5 {
            if cancelled.load(Ordering::Relaxed) {
                return Err(Error::Validation("扫码导入已取消".into()));
            }
            let response = http
                .get(url.clone())
                .send()
                .map_err(|_| Error::Response("无法交换登录凭据".into()))?;
            for cookie in response.headers().get_all(reqwest::header::SET_COOKIE) {
                if let Ok(cookie) = cookie.to_str()
                    && let Some(token) = cookie
                        .split(';')
                        .next()
                        .and_then(|c| c.trim().strip_prefix("serviceToken="))
                    && !token.is_empty()
                {
                    return Ok(Session {
                        user_id: user
                            .as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| user.to_string()),
                        ssecurity: security.into(),
                        service_token: token.into(),
                        region: region.into(),
                    });
                }
            }
            if !response.status().is_redirection() {
                break;
            }
            let next = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| Error::Response("登录跳转缺少地址".into()))?;
            url = account_url(
                url.join(next)
                    .map_err(|_| Error::Response("登录跳转地址无效".into()))?
                    .as_str(),
            )?;
        }
        return Err(Error::Response(
            "登录成功，但未取得服务凭据，请重新扫码".into(),
        ));
    }
    Err(Error::Validation(
        if cancelled.load(Ordering::Relaxed) {
            "扫码导入已取消"
        } else {
            "二维码已过期，请重新获取"
        }
        .into(),
    ))
}

pub fn rc4(key: &[u8], input: &[u8]) -> Result<Vec<u8>> {
    if key.is_empty() {
        return Err(Error::Response("加密密钥为空".into()));
    }
    let mut s = [0u8; 256];
    for (index, value) in s.iter_mut().enumerate() {
        *value = index as u8;
    }
    let mut j = 0usize;
    for i in 0..256 {
        j = (j + s[i] as usize + key[i % key.len()] as usize) & 255;
        s.swap(i, j);
    }
    let (mut i, mut j) = (0usize, 0usize);
    let mut output = Vec::with_capacity(input.len());
    for index in 0..1024 + input.len() {
        i = (i + 1) & 255;
        j = (j + s[i] as usize) & 255;
        s.swap(i, j);
        let byte = s[(s[i] as usize + s[j] as usize) & 255];
        if index >= 1024 {
            output.push(input[index - 1024] ^ byte);
        }
    }
    s.zeroize();
    Ok(output)
}
fn base64(text: &str) -> Result<Vec<u8>> {
    B64.decode(text.trim())
        .map_err(|_| Error::Response("云服务加密数据无效".into()))
}
fn signature(path: &str, fields: &[(String, String)], signed: &str) -> String {
    let mut parts = vec!["POST".into(), path.into()];
    parts.extend(fields.iter().map(|(k, v)| format!("{k}={v}")));
    parts.push(signed.into());
    B64.encode(Sha1::digest(parts.join("&").as_bytes()))
}

impl Session {
    pub fn account_key(&self) -> String {
        format!("{}:{}", self.region, self.user_id)
    }
    pub fn request(&self, api: &str, data: Value) -> Result<Value> {
        let mut nonce = vec![0u8; 8];
        rand::thread_rng().fill_bytes(&mut nonce);
        nonce.extend_from_slice(
            &((SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                / 60) as u32)
                .to_be_bytes(),
        );
        let mut sign = base64(&self.ssecurity)?;
        sign.extend_from_slice(&nonce);
        let signed_key = Sha256::digest(&sign);
        sign.zeroize();
        let signed = B64.encode(signed_key);
        let path = format!("/{api}");
        let data = data.to_string();
        let hash = signature(&path, &[("data".into(), data.clone())], &signed);
        let encrypted = vec![
            (
                "data".into(),
                B64.encode(rc4(&signed_key, &data.into_bytes())?),
            ),
            (
                "rc4_hash__".into(),
                B64.encode(rc4(&signed_key, hash.as_bytes())?),
            ),
        ];
        let signature = signature(&path, &encrypted, &signed);
        let mut fields = encrypted;
        fields.extend([
            ("signature".into(), signature),
            ("ssecurity".into(), self.ssecurity.clone()),
            ("_nonce".into(), B64.encode(nonce)),
        ]);
        let base = if self.region == "cn" {
            "https://api.io.mi.com/app".to_string()
        } else {
            format!("https://{}.api.io.mi.com/app", self.region)
        };
        let cookie = format!(
            "userId={}; serviceToken={}; yetAnotherServiceToken={}; locale=zh_CN; timezone=GMT+08:00; is_daylight=0; dst_offset=0; channel=MI_APP_STORE",
            self.user_id, self.service_token, self.service_token
        );
        let response = http()?
            .post(format!("{base}{path}"))
            .header(
                "User-Agent",
                "MijiaLAN-Rust APP/com.xiaomi.mihome APPV/10.5.201",
            )
            .header("x-xiaomi-protocal-flag-cli", "PROTOCAL-HTTP2")
            .header("MIOT-ENCRYPT-ALGORITHM", "ENCRYPT-RC4")
            .header("Accept-Encoding", "identity")
            .header("Cookie", cookie)
            .form(&fields)
            .send()
            .map_err(|_| Error::Response("无法获取小米设备列表".into()))?;
        if !response.status().is_success() {
            return Err(Error::Response(format!(
                "小米云返回 HTTP {}",
                response.status().as_u16()
            )));
        }
        let text = response
            .text()
            .map_err(|_| Error::Response("无法读取云响应".into()))?;
        let value: Value = if let Ok(value) = serde_json::from_str(&text) {
            value
        } else {
            serde_json::from_slice(&rc4(&signed_key, &base64(&text)?)?)
                .map_err(|_| Error::Response("无法解密云响应".into()))?
        };
        if value["code"].as_i64().unwrap_or(0) != 0 {
            return Err(Error::Response(format!(
                "小米云返回错误 {}，请重新扫码",
                value["code"]
            )));
        }
        Ok(value["result"].clone())
    }
    pub fn import(&self, engine: &Engine, cancelled: &AtomicBool) -> Result<ImportReport> {
        let mut report = ImportReport::default();
        let mut order = vec![self.region.as_str()];
        order.extend(regions().iter().copied().filter(|r| *r != self.region));
        for region in order {
            if cancelled.load(Ordering::Relaxed) {
                return Err(Error::Validation("导入已取消".into()));
            }
            let mut scoped = self.clone();
            scoped.region = region.into();
            let result = scoped.request("home/device_list",json!({"getVirtualModel":false,"getHuamiDevices":0,"get_split_device":false,"support_smart_home":true}))?;
            let Some(list) = result["list"].as_array().filter(|l| !l.is_empty()) else {
                continue;
            };
            let rooms = scoped.request("v2/homeroom/gethome_merged",json!({"fg":true,"fetch_share":true,"fetch_share_dev":true,"limit":300,"app_ver":7})).unwrap_or(Value::Null);
            for item in list {
                if cancelled.load(Ordering::Relaxed) {
                    return Err(Error::Validation("导入已取消".into()));
                }
                let name = item["name"].as_str().unwrap_or("未命名设备");
                let did = item["did"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| item["did"].to_string());
                let token = item["token"].as_str().and_then(|t| Token::parse(t).ok());
                let host = item["localip"]
                    .as_str()
                    .and_then(|h| h.parse::<Ipv4Addr>().ok())
                    .filter(|h| !h.is_unspecified() && !h.is_multicast() && !h.is_broadcast());
                let (Some(token), Some(host), Some(model)) = (token, host, item["model"].as_str())
                else {
                    report.skipped.push(name.into());
                    continue;
                };
                let mut room = String::new();
                for home in rooms["homelist"].as_array().into_iter().flatten() {
                    for r in home["roomlist"].as_array().into_iter().flatten() {
                        if r["dids"]
                            .as_array()
                            .is_some_and(|dids| dids.iter().any(|d| d.as_str() == Some(&did)))
                        {
                            room = r["name"].as_str().unwrap_or_default().into();
                        }
                    }
                }
                engine.import(
                    Device {
                        did,
                        name: name.into(),
                        model: model.into(),
                        host,
                        room,
                        port: crate::packet::PORT,
                        bridged: true,
                    },
                    &token,
                )?;
                report.imported += 1;
            }
            report.region = region.into();
            return Ok(report);
        }
        Ok(report)
    }
}

#[derive(Debug, Default)]
pub struct ImportReport {
    pub imported: usize,
    pub skipped: Vec<String>,
    pub region: String,
}
