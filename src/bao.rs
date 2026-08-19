use reqwest::Client;
use serde::Deserialize;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};
use totp_lite::{totp_custom, Sha1};

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

/// TOTP verification (RFC 6238, SHA1, 30s step, 6 digits) with +/-1 step tolerance
/// for clock drift and typing time. Accepts base32 secrets with or without padding.
pub fn verify_totp(secret_b32: &str, code: &str) -> bool {
    let cleaned: String = secret_b32
        .trim()
        .trim_end_matches('=')
        .replace(' ', "")
        .to_uppercase();
    let Some(secret) = base32::decode(base32::Alphabet::RFC4648 { padding: false }, &cleaned) else {
        return false;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let code = code.trim();
    for window in [now.saturating_sub(30), now, now + 30] {
        if totp_custom::<Sha1>(30, 6, &secret, window) == code {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 6238 test key (ASCII "12345678901234567890"), base32-encoded
    const RFC_SECRET_B32: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    #[test]
    fn totp_matches_rfc6238_vector() {
        let secret = base32::decode(
            base32::Alphabet::RFC4648 { padding: false },
            RFC_SECRET_B32,
        )
        .unwrap();
        // RFC 6238 SHA1, T=59s, 6 digits: 287082
        assert_eq!(totp_custom::<Sha1>(30, 6, &secret, 59), "287082");
    }

    #[test]
    fn verify_accepts_current_code() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let secret = base32::decode(
            base32::Alphabet::RFC4648 { padding: false },
            RFC_SECRET_B32,
        )
        .unwrap();
        let code = totp_custom::<Sha1>(30, 6, &secret, now);
        assert!(verify_totp(RFC_SECRET_B32, &code));
    }

    #[test]
    fn verify_rejects_wrong_code() {
        assert!(!verify_totp(RFC_SECRET_B32, "000000"));
    }

    #[test]
    fn verify_rejects_garbage_secret() {
        assert!(!verify_totp("!!!not-base32!!!", "123456"));
    }
}
