use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;

const VERSION: u8 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const PBKDF2_ITERS: u32 = 210_000;
const HEADER_LEN: usize = 1 + SALT_LEN + NONCE_LEN;

fn derive_key(passphrase: &str, salt: &[u8]) -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), salt, PBKDF2_ITERS, &mut key);
    key
}

fn fill_random(buf: &mut [u8]) -> Result<(), String> {
    getrandom::fill(buf).map_err(|e| format!("随机数生成失败: {e}"))
}

fn nonce_from(bytes: &[u8]) -> Result<Nonce<aes_gcm::aead::consts::U12>, String> {
    Nonce::try_from(bytes).map_err(|_| "nonce 长度无效".to_string())
}

/// Encrypt plaintext with passphrase; returns base64 payload.
#[tauri::command]
pub fn crypto_encrypt(plaintext: String, passphrase: String) -> Result<String, String> {
    if passphrase.is_empty() {
        return Err("口令不能为空".to_string());
    }

    let mut salt = [0u8; SALT_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    fill_random(&mut salt)?;
    fill_random(&mut nonce_bytes)?;

    let key = derive_key(&passphrase, &salt);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("密钥无效: {e}"))?;
    let nonce = nonce_from(&nonce_bytes)?;
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|_| "加密失败".to_string())?;

    let mut out = Vec::with_capacity(HEADER_LEN + ciphertext.len());
    out.push(VERSION);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(B64.encode(out))
}

/// Decrypt base64 payload with passphrase; returns plaintext.
#[tauri::command]
pub fn crypto_decrypt(ciphertext: String, passphrase: String) -> Result<String, String> {
    if passphrase.is_empty() {
        return Err("口令不能为空".to_string());
    }

    let raw = B64
        .decode(ciphertext.trim())
        .map_err(|_| "密文不是合法 base64".to_string())?;
    if raw.len() < HEADER_LEN + 16 {
        return Err("密文过短或已损坏".to_string());
    }
    if raw[0] != VERSION {
        return Err(format!("不支持的密文版本: {}", raw[0]));
    }

    let salt = &raw[1..1 + SALT_LEN];
    let nonce_bytes = &raw[1 + SALT_LEN..HEADER_LEN];
    let body = &raw[HEADER_LEN..];

    let key = derive_key(&passphrase, salt);
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("密钥无效: {e}"))?;
    let nonce = nonce_from(nonce_bytes)?;
    let plain = cipher
        .decrypt(&nonce, body)
        .map_err(|_| "口令错误或密文已损坏".to_string())?;
    String::from_utf8(plain).map_err(|_| "解密结果不是合法 UTF-8".to_string())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let ct = crypto_encrypt("你好 secret".into(), "pw".into()).unwrap();
        let pt = crypto_decrypt(ct.clone(), "pw".into()).unwrap();
        assert_eq!(pt, "你好 secret");
        assert!(crypto_decrypt(ct, "bad".into()).is_err());
    }
}
