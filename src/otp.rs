use anyhow::{Result, bail};
use hmac::{Hmac, Mac};
use sha1::Sha1;

pub const STEP_SECONDS: u64 = 30;
const DIGITS: u32 = 6;

pub fn decode_base32(value: &str) -> Result<Vec<u8>> {
    let mut bits = 0u32;
    let mut width = 0u32;
    let mut bytes = Vec::new();

    for character in value.chars().filter(|c| !c.is_whitespace() && *c != '=') {
        let index = match character.to_ascii_uppercase() {
            letter @ 'A'..='Z' => letter as u32 - 'A' as u32,
            digit @ '2'..='7' => digit as u32 - '2' as u32 + 26,
            other => bail!("the TOTP seed contains {other:?}, which is not base32"),
        };
        bits = (bits << 5) | index;
        width += 5;
        if width >= 8 {
            width -= 8;
            bytes.push((bits >> width) as u8);
        }
    }

    if bytes.is_empty() {
        bail!("the TOTP seed decoded to no bytes");
    }
    Ok(bytes)
}

pub const fn step_of(unix_seconds: u64) -> u64 {
    unix_seconds / STEP_SECONDS
}

pub fn code_for_step(secret: &[u8], step: u64) -> String {
    let mut mac = <Hmac<Sha1>>::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(&step.to_be_bytes());
    let digest = mac.finalize().into_bytes();

    let offset = (digest[digest.len() - 1] & 0x0f) as usize;
    let truncated = u32::from_be_bytes([
        digest[offset] & 0x7f,
        digest[offset + 1],
        digest[offset + 2],
        digest[offset + 3],
    ]);

    let modulus = 10u32.pow(DIGITS);
    format!("{:0width$}", truncated % modulus, width = DIGITS as usize)
}

pub fn code_at(secret: &[u8], unix_seconds: u64) -> String {
    code_for_step(secret, step_of(unix_seconds))
}

pub const fn seconds_remaining(unix_seconds: u64) -> u64 {
    STEP_SECONDS - (unix_seconds % STEP_SECONDS)
}

pub fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

pub fn is_six_digits(code: &str) -> bool {
    code.len() == DIGITS as usize && code.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
#[path = "../tests/unit/otp.rs"]
mod tests;
