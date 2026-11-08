use anyhow::{Result, anyhow, bail};
use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit,
    aead::{Aead, Payload},
};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hap_crypto::HapPairSetupSrpServer;
use hkdf::Hkdf;
use rand::rngs::OsRng;
use sha2::Sha512;
use std::collections::BTreeMap;
use x25519_dalek::{PublicKey, StaticSecret};

pub fn tlv(fields: &[(u8, &[u8])]) -> Vec<u8> {
    let mut data = Vec::new();
    for (kind, value) in fields {
        if value.is_empty() {
            data.extend_from_slice(&[*kind, 0]);
        }
        for chunk in value.chunks(255) {
            data.extend_from_slice(&[*kind, chunk.len() as u8]);
            data.extend_from_slice(chunk);
        }
    }
    data
}
pub fn parse(data: &[u8]) -> Result<BTreeMap<u8, Vec<u8>>> {
    let mut fields: BTreeMap<u8, Vec<u8>> = BTreeMap::new();
    let mut position = 0;
    while position < data.len() {
        if position + 2 > data.len() {
            bail!("truncated TLV");
        }
        let kind = data[position];
        let length = data[position + 1] as usize;
        position += 2;
        if position + length > data.len() {
            bail!("truncated TLV value");
        }
        fields
            .entry(kind)
            .or_default()
            .extend_from_slice(&data[position..position + length]);
        position += length;
    }
    Ok(fields)
}
pub fn field(fields: &BTreeMap<u8, Vec<u8>>, kind: u8) -> Result<&[u8]> {
    fields
        .get(&kind)
        .map(Vec::as_slice)
        .ok_or_else(|| anyhow!("missing pairing field"))
}
pub fn derive(key: &[u8], salt: &str, info: &str) -> Result<[u8; 32]> {
    let mut result = [0u8; 32];
    Hkdf::<Sha512>::new(Some(salt.as_bytes()), key)
        .expand(info.as_bytes(), &mut result)
        .map_err(|_| anyhow!("HKDF failed"))?;
    Ok(result)
}
pub fn seal(key: &[u8; 32], nonce: [u8; 12], plain: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    ChaCha20Poly1305::new(key.into())
        .encrypt((&nonce).into(), Payload { msg: plain, aad })
        .map_err(|_| anyhow!("encryption failed"))
}
pub fn open(key: &[u8; 32], nonce: [u8; 12], encrypted: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    ChaCha20Poly1305::new(key.into())
        .decrypt(
            (&nonce).into(),
            Payload {
                msg: encrypted,
                aad,
            },
        )
        .map_err(|_| anyhow!("authentication failed"))
}
fn nonce(label: &[u8; 8]) -> [u8; 12] {
    let mut nonce = [0; 12];
    nonce[4..].copy_from_slice(label);
    nonce
}
fn check_signature(public: &[u8], signature: &[u8], message: &[u8]) -> Result<()> {
    let public: [u8; 32] = public
        .try_into()
        .map_err(|_| anyhow!("invalid public key"))?;
    VerifyingKey::from_bytes(&public)?
        .verify_strict(message, &Signature::from_slice(signature)?)?;
    Ok(())
}

pub struct PairSetup {
    srp: HapPairSetupSrpServer,
    key: Option<Vec<u8>>,
    phase: u8,
}
impl PairSetup {
    pub fn start(code: &str) -> Result<(Self, Vec<u8>)> {
        let (srp, salt) = HapPairSetupSrpServer::new(code)?;
        let response = tlv(&[(6, &[2]), (2, &salt), (3, &srp.b_pub_bytes())]);
        Ok((
            Self {
                srp,
                key: None,
                phase: 2,
            },
            response,
        ))
    }
    pub fn prove(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        if self.phase != 2 {
            bail!("unexpected setup state");
        }
        let fields = parse(data)?;
        let public = field(&fields, 3)?;
        let proof = field(&fields, 4)?;
        let response = self.srp.verify_m1_prove_m2(public, proof)?;
        self.key = Some(self.srp.session_key(public)?);
        self.phase = 4;
        Ok(tlv(&[(6, &[4]), (4, &response)]))
    }
    pub fn finish(
        &mut self,
        data: &[u8],
        accessory_id: &str,
        signing: &SigningKey,
    ) -> Result<(String, [u8; 32], Vec<u8>)> {
        if self.phase != 4 {
            bail!("unexpected setup state");
        }
        let key = self
            .key
            .as_ref()
            .ok_or_else(|| anyhow!("setup not authenticated"))?;
        let encryption = derive(key, "Pair-Setup-Encrypt-Salt", "Pair-Setup-Encrypt-Info")?;
        let fields = parse(data)?;
        let plain = open(&encryption, nonce(b"PS-Msg05"), field(&fields, 5)?, &[])?;
        let fields = parse(&plain)?;
        let id = std::str::from_utf8(field(&fields, 1)?)?.to_string();
        if id.is_empty() || id.len() > 128 {
            bail!("invalid controller id");
        }
        let public: [u8; 32] = field(&fields, 3)?
            .try_into()
            .map_err(|_| anyhow!("invalid controller key"))?;
        let mut message = derive(
            key,
            "Pair-Setup-Controller-Sign-Salt",
            "Pair-Setup-Controller-Sign-Info",
        )?
        .to_vec();
        message.extend_from_slice(id.as_bytes());
        message.extend_from_slice(&public);
        check_signature(&public, field(&fields, 10)?, &message)?;
        let public_accessory = signing.verifying_key().to_bytes();
        let mut message = derive(
            key,
            "Pair-Setup-Accessory-Sign-Salt",
            "Pair-Setup-Accessory-Sign-Info",
        )?
        .to_vec();
        message.extend_from_slice(accessory_id.as_bytes());
        message.extend_from_slice(&public_accessory);
        let signature = signing.sign(&message).to_bytes();
        let sub = tlv(&[
            (1, accessory_id.as_bytes()),
            (3, &public_accessory),
            (10, &signature),
        ]);
        let encrypted = seal(&encryption, nonce(b"PS-Msg06"), &sub, &[])?;
        self.phase = 6;
        Ok((id, public, tlv(&[(6, &[6]), (5, &encrypted)])))
    }
}
impl Drop for PairSetup {
    fn drop(&mut self) {
        if let Some(key) = &mut self.key {
            use zeroize::Zeroize;
            key.zeroize();
        }
    }
}

pub struct PairVerify {
    shared: [u8; 32],
    encryption: [u8; 32],
    accessory_public: [u8; 32],
    controller_public: [u8; 32],
}
impl PairVerify {
    pub fn start(data: &[u8], accessory_id: &str, signing: &SigningKey) -> Result<(Self, Vec<u8>)> {
        let fields = parse(data)?;
        let controller_public: [u8; 32] = field(&fields, 3)?
            .try_into()
            .map_err(|_| anyhow!("invalid ephemeral key"))?;
        let secret = StaticSecret::random_from_rng(OsRng);
        let accessory_public = PublicKey::from(&secret).to_bytes();
        let shared = secret.diffie_hellman(&PublicKey::from(controller_public));
        if !shared.was_contributory() {
            bail!("invalid shared key");
        }
        let shared = shared.to_bytes();
        let encryption = derive(
            &shared,
            "Pair-Verify-Encrypt-Salt",
            "Pair-Verify-Encrypt-Info",
        )?;
        let mut message = accessory_public.to_vec();
        message.extend_from_slice(accessory_id.as_bytes());
        message.extend_from_slice(&controller_public);
        let signature = signing.sign(&message).to_bytes();
        let sub = tlv(&[(1, accessory_id.as_bytes()), (10, &signature)]);
        let encrypted = seal(&encryption, nonce(b"PV-Msg02"), &sub, &[])?;
        Ok((
            Self {
                shared,
                encryption,
                accessory_public,
                controller_public,
            },
            tlv(&[(6, &[2]), (3, &accessory_public), (5, &encrypted)]),
        ))
    }
    pub fn controller(&self, data: &[u8]) -> Result<(String, Vec<u8>)> {
        let fields = parse(data)?;
        let plain = open(
            &self.encryption,
            nonce(b"PV-Msg03"),
            field(&fields, 5)?,
            &[],
        )?;
        let fields = parse(&plain)?;
        Ok((
            std::str::from_utf8(field(&fields, 1)?)?.to_string(),
            field(&fields, 10)?.to_vec(),
        ))
    }
    pub fn finish(&self, id: &str, public: &[u8; 32], signature: &[u8]) -> Result<Session> {
        let mut message = self.controller_public.to_vec();
        message.extend_from_slice(id.as_bytes());
        message.extend_from_slice(&self.accessory_public);
        check_signature(public, signature, &message)?;
        Ok(Session {
            read_key: derive(&self.shared, "Control-Salt", "Control-Write-Encryption-Key")?,
            write_key: derive(&self.shared, "Control-Salt", "Control-Read-Encryption-Key")?,
            read_count: 0,
            write_count: 0,
        })
    }
}
impl Drop for PairVerify {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.shared.zeroize();
        self.encryption.zeroize();
    }
}

pub struct Session {
    pub read_key: [u8; 32],
    pub write_key: [u8; 32],
    pub read_count: u64,
    pub write_count: u64,
}
impl Session {
    pub fn encode(&mut self, plain: &[u8]) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        for chunk in plain.chunks(1024) {
            let length = (chunk.len() as u16).to_le_bytes();
            let mut nonce = [0; 12];
            nonce[4..].copy_from_slice(&self.write_count.to_le_bytes());
            self.write_count = self
                .write_count
                .checked_add(1)
                .ok_or_else(|| anyhow!("nonce exhausted"))?;
            output.extend_from_slice(&length);
            output.extend_from_slice(&seal(&self.write_key, nonce, chunk, &length)?);
        }
        Ok(output)
    }
    pub fn decode_frame(&mut self, frame: &[u8]) -> Result<Vec<u8>> {
        if frame.len() < 18 {
            bail!("short encrypted frame");
        }
        let length = u16::from_le_bytes([frame[0], frame[1]]) as usize;
        if length > 1024 || length + 18 != frame.len() {
            bail!("invalid encrypted frame length");
        }
        let mut nonce = [0; 12];
        nonce[4..].copy_from_slice(&self.read_count.to_le_bytes());
        let plain = open(&self.read_key, nonce, &frame[2..], &frame[..2])?;
        self.read_count = self
            .read_count
            .checked_add(1)
            .ok_or_else(|| anyhow!("nonce exhausted"))?;
        Ok(plain)
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.read_key.zeroize();
        self.write_key.zeroize();
    }
}
