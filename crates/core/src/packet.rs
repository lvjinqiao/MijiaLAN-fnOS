use aes::Aes128;
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use md5::{Digest, Md5};
use std::fmt;
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Token 必须是 32 位十六进制字符")]
    InvalidToken,
    #[error("无效或不完整的 miIO 数据包")]
    Packet,
    #[error("设备响应校验失败，请检查 Token")]
    Checksum,
    #[error("设备未响应，请检查 IP、网络权限和设备电源")]
    Timeout,
    #[error("握手成功，但加密请求未响应；请检查 Token 或设备的局域网支持")]
    NoEncryptedReply,
    #[error("网络错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("设备返回错误 {code}：{message}")]
    Device { code: i64, message: String },
    #[error("响应错误：{0}")]
    Response(String),
    #[error("{0}")]
    Storage(String),
    #[error("规格错误：{0}")]
    Spec(String),
    #[error("无效操作：{0}")]
    Validation(String),
}

#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Token([u8; 16]);

impl Token {
    pub fn parse(text: &str) -> Result<Self> {
        let clean: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        let bytes = hex::decode(clean).map_err(|_| Error::InvalidToken)?;
        Ok(Self(bytes.try_into().map_err(|_| Error::InvalidToken)?))
    }

    pub(crate) fn hex(&self) -> String {
        hex::encode(self.0)
    }
    pub(crate) fn bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token([REDACTED])")
    }
}

pub const PORT: u16 = 54321;
pub const HEADER_LEN: usize = 32;
pub const HELLO: [u8; 32] = {
    let mut data = [0xff; 32];
    data[0] = 0x21;
    data[1] = 0x31;
    data[2] = 0;
    data[3] = 32;
    data
};

#[derive(Debug, Clone, Copy)]
pub struct Header {
    pub length: usize,
    pub device_id: u32,
    pub stamp: u32,
}

pub fn header(data: &[u8]) -> Result<Header> {
    if data.len() < HEADER_LEN || data[..2] != [0x21, 0x31] {
        return Err(Error::Packet);
    }
    let length = u16::from_be_bytes([data[2], data[3]]) as usize;
    if length < HEADER_LEN || length != data.len() {
        return Err(Error::Packet);
    }
    Ok(Header {
        length,
        device_id: u32::from_be_bytes(data[8..12].try_into().unwrap()),
        stamp: u32::from_be_bytes(data[12..16].try_into().unwrap()),
    })
}

fn keys(token: &Token) -> ([u8; 16], [u8; 16]) {
    let key: [u8; 16] = Md5::digest(token.bytes()).into();
    let iv = Md5::new()
        .chain_update(key)
        .chain_update(token.bytes())
        .finalize()
        .into();
    (key, iv)
}

pub fn encode(payload: &[u8], token: &Token, device_id: u32, stamp: u32) -> Result<Vec<u8>> {
    let (mut key, mut iv) = keys(token);
    let encrypted = cbc::Encryptor::<Aes128>::new(&key.into(), &iv.into())
        .encrypt_padded_vec_mut::<Pkcs7>(payload);
    key.zeroize();
    iv.zeroize();
    let length = HEADER_LEN + encrypted.len();
    if length > u16::MAX as usize {
        return Err(Error::Packet);
    }
    let mut data = vec![0x21, 0x31];
    data.extend_from_slice(&(length as u16).to_be_bytes());
    data.extend_from_slice(&[0; 4]);
    data.extend_from_slice(&device_id.to_be_bytes());
    data.extend_from_slice(&stamp.to_be_bytes());
    let checksum = Md5::new()
        .chain_update(&data)
        .chain_update(token.bytes())
        .chain_update(&encrypted)
        .finalize();
    data.extend_from_slice(&checksum);
    data.extend_from_slice(&encrypted);
    Ok(data)
}

pub fn decode(data: &[u8], token: &Token) -> Result<(Header, Vec<u8>)> {
    let header = header(data)?;
    let encrypted = &data[HEADER_LEN..];
    if encrypted.is_empty() || !encrypted.len().is_multiple_of(16) {
        return Err(Error::Packet);
    }
    let checksum = Md5::new()
        .chain_update(&data[..16])
        .chain_update(token.bytes())
        .chain_update(encrypted)
        .finalize();
    if !bool::from(checksum.as_slice().ct_eq(&data[16..32])) {
        return Err(Error::Checksum);
    }
    let (mut key, mut iv) = keys(token);
    let result = cbc::Decryptor::<Aes128>::new(&key.into(), &iv.into())
        .decrypt_padded_vec_mut::<Pkcs7>(encrypted)
        .map_err(|_| Error::Checksum);
    key.zeroize();
    iv.zeroize();
    Ok((header, result?))
}
