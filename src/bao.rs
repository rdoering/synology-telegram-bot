use reqwest::Client;
use serde::Deserialize;
use std::fmt;

#[derive(Debug)]
pub enum BaoError {
    Reqwest(reqwest::Error),
    Api(String),
}

impl fmt::Display for BaoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BaoError::Reqwest(err) => write!(f, "HTTP error: {}", err),
            BaoError::Api(msg) => write!(f, "OpenBao API error: {}", msg),
        }
    }
}

impl std::error::Error for BaoError {}

impl From<reqwest::Error> for BaoError {
    fn from(err: reqwest::Error) -> Self {
        BaoError::Reqwest(err)
    }
}

#[derive(Debug, Deserialize)]
pub struct SealStatusInfo {
    pub initialized: bool,
    pub sealed: bool,
    #[serde(default)]
    pub progress: u32,
    #[serde(default)]
    pub t: u32,
}

#[derive(Debug, Deserialize)]
struct ApiErrorResponse {
    errors: Vec<String>,
}

/// Minimal OpenBao client for seal management (unauthenticated endpoints).
pub struct BaoClient {
    addr: String,
    http: Client,
}

impl BaoClient {
    pub fn new(addr: &str) -> Self {
        BaoClient {
            addr: addr.trim_end_matches('/').to_string(),
            http: Client::new(),
        }
    }

    pub async fn seal_status(&self) -> Result<SealStatusInfo, BaoError> {
        let url = format!("{}/v1/sys/seal-status", self.addr);
        let resp = self.http.get(&url).send().await?;
        if !resp.status().is_success() {
            return Err(BaoError::Api(format!("seal-status returned {}", resp.status())));
        }
        Ok(resp.json::<SealStatusInfo>().await?)
    }

    pub async fn unseal(&self, key: &str) -> Result<SealStatusInfo, BaoError> {
        let url = format!("{}/v1/sys/unseal", self.addr);
        let resp = self.http
            .put(&url)
            .json(&serde_json::json!({ "key": key }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            let msg = serde_json::from_str::<ApiErrorResponse>(&body)
                .map(|e| e.errors.join("; "))
                .unwrap_or(body);
            return Err(BaoError::Api(format!("unseal returned {}: {}", status, msg)));
        }
        Ok(resp.json::<SealStatusInfo>().await?)
    }
}

/// Ephemeral X25519 keypair for one unseal challenge (lives only in RAM).
pub struct EphemeralKey {
    pub identity: age::x25519::Identity,
    pub recipient: String,
}

pub fn generate_ephemeral_key() -> EphemeralKey {
    let identity = age::x25519::Identity::generate();
    let recipient = identity.to_public().to_string();
    EphemeralKey { identity, recipient }
}

/// Random URL-safe session reference (16 bytes hex).
pub fn random_session_id() -> String {
    let mut bytes = [0u8; 16];
    rand::Rng::fill(&mut rand::thread_rng(), &mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Decrypt an age-armored ciphertext with an ephemeral identity.
pub fn decrypt_ciphertext(
    ciphertext: &str,
    identity: &age::x25519::Identity,
) -> Result<String, BaoError> {
    use std::io::Read;

    let armored = age::armor::ArmoredReader::new(ciphertext.as_bytes());
    let decryptor = age::Decryptor::new(armored)
        .map_err(|e| BaoError::Api(format!("invalid age ciphertext: {}", e)))?;
    let mut reader = decryptor
        .decrypt(std::iter::once(identity as &dyn age::Identity))
        .map_err(|e| BaoError::Api(format!("decryption failed: {}", e)))?;
    let mut buf = String::new();
    reader
        .read_to_string(&mut buf)
        .map_err(|e| BaoError::Api(format!("read failed: {}", e)))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn age_roundtrip() {
        let key = generate_ephemeral_key();
        let recipient: age::x25519::Recipient = key.recipient.parse().expect("valid recipient");
        let encryptor = age::Encryptor::with_recipients(
            std::iter::once(&recipient as &dyn age::Recipient),
        ).expect("encryptor");
        let mut ciphertext = vec![];
        {
            use std::io::Write;
            let armor = age::armor::ArmoredWriter::wrap_output(
                &mut ciphertext,
                age::armor::Format::AsciiArmor,
            )
            .unwrap();
            let mut writer = encryptor.wrap_output(armor).unwrap();
            writer.write_all(b"roundtrip-secret").unwrap();
            writer.finish().unwrap().finish().unwrap();
        }
        let decrypted = decrypt_ciphertext(
            &String::from_utf8(ciphertext).unwrap(),
            &key.identity,
        )
        .unwrap();
        assert_eq!(decrypted, "roundtrip-secret");
    }

    /// Interop: ciphertext produced by the official age JS implementation
    /// (same library the web app bundles) must be decryptable here.
    #[test]
    fn decrypts_js_produced_ciphertext() {
        let fixture = include_str!("../tests/fixtures/js-ciphertext.json");
        let v: serde_json::Value = serde_json::from_str(fixture).unwrap();
        let identity: age::x25519::Identity = v["identity"]
            .as_str()
            .unwrap()
            .parse()
            .expect("valid identity in fixture");
        let decrypted = decrypt_ciphertext(v["ciphertext"].as_str().unwrap(), &identity)
            .expect("js ciphertext must decrypt");
        assert_eq!(decrypted, "test-secret-123");
    }

    #[test]
    fn session_id_is_32_hex_chars() {
        let id = random_session_id();
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
