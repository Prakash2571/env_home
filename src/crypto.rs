use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

const NONCE_BYTES: usize = 12;
const FORMAT_PREFIX: &str = "v1";

#[derive(Clone)]
pub struct Cipher {
    cipher: Aes256Gcm,
}

impl Cipher {
    pub fn from_hex(value: &str) -> Result<Self> {
        let trimmed = value.trim();
        if trimmed.len() != 64 || !trimmed.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("CBL_ENCRYPTION_KEY must be 64 hex characters (32 bytes)");
        }
        let bytes = hex::decode(trimmed).map_err(|_| anyhow!("CBL_ENCRYPTION_KEY is not hex"))?;
        let key = Key::<Aes256Gcm>::from_slice(&bytes);
        Ok(Self {
            cipher: Aes256Gcm::new(key),
        })
    }

    pub fn encrypt(&self, plaintext: &str, context: &str) -> Result<String> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let sealed = self
            .cipher
            .encrypt(
                &nonce,
                aes_gcm::aead::Payload {
                    msg: plaintext.as_bytes(),
                    aad: context.as_bytes(),
                },
            )
            .map_err(|_| anyhow!("encryption failed"))?;
        Ok(format!(
            "{FORMAT_PREFIX}.{}.{}",
            URL_SAFE_NO_PAD.encode(nonce),
            URL_SAFE_NO_PAD.encode(sealed)
        ))
    }

    pub fn decrypt(&self, sealed: &str, context: &str) -> Result<String> {
        let mut parts = sealed.split('.');
        let (Some(FORMAT_PREFIX), Some(nonce), Some(body), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            bail!("the stored value is not in the expected format");
        };
        let nonce = URL_SAFE_NO_PAD
            .decode(nonce)
            .map_err(|_| anyhow!("the stored nonce is corrupt"))?;
        let body = URL_SAFE_NO_PAD
            .decode(body)
            .map_err(|_| anyhow!("the stored value is corrupt"))?;
        if nonce.len() != NONCE_BYTES {
            bail!("the stored nonce has the wrong length");
        }
        let opened = self
            .cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                aes_gcm::aead::Payload {
                    msg: &body,
                    aad: context.as_bytes(),
                },
            )
            .map_err(|_| {
                anyhow!("the stored value failed authentication; wrong key or tampered row")
            })?;
        String::from_utf8(opened).map_err(|_| anyhow!("the stored value is not UTF-8"))
    }
}

pub fn sha256_hex(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

pub fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = Sha256::digest(left.as_bytes());
    let right = Sha256::digest(right.as_bytes());
    left.iter()
        .zip(right.iter())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

#[cfg(test)]
#[path = "../tests/unit/crypto.rs"]
mod tests;
