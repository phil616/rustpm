use std::{
    net::IpAddr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::http::{header, HeaderMap};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};

pub fn extract_password_from_headers(headers: &HeaderMap) -> String {
    if let Some(v) = headers.get("x-rustpm-password").and_then(|v| v.to_str().ok()) {
        return v.to_string();
    }
    if let Some(v) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(rest) = v.strip_prefix("Bearer ") {
            return rest.to_string();
        }
    }
    String::new()
}

pub fn extract_session_id(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in raw.split(';') {
        let kv = part.trim();
        let Some((k, v)) = kv.split_once('=') else {
            continue;
        };
        if k.trim() == cookie_name {
            let value = v.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

pub fn new_session_id() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex_encode(&bytes)
}

pub fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_secs()
}

pub fn user_agent_hash(headers: &HeaderMap) -> String {
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    let digest = Sha256::digest(ua.as_bytes());
    hex_encode(digest.as_slice())
}

pub fn client_ip_prefix(ip: Option<IpAddr>) -> Option<String> {
    let ip = ip?;
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            Some(format!("{}.{}.{}.*", o[0], o[1], o[2]))
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            Some(format!("{:x}:{:x}:{:x}:{:x}::", s[0], s[1], s[2], s[3]))
        }
    }
}

pub fn infer_secure_cookie(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("https"))
        .unwrap_or(false)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(nibble_to_hex(b >> 4));
        out.push(nibble_to_hex(b & 0x0f));
    }
    out
}

fn nibble_to_hex(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        _ => (b'a' + (n - 10)) as char,
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_session_id, user_agent_hash};
    use axum::http::{header, HeaderMap, HeaderValue};

    #[test]
    fn extract_session_id_parses_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("a=1; rustpm_session=abc123; b=2"),
        );
        let value = extract_session_id(&headers, "rustpm_session");
        assert_eq!(value.as_deref(), Some("abc123"));
    }

    #[test]
    fn user_agent_hash_is_stable_for_same_input() {
        let mut headers = HeaderMap::new();
        headers.insert(header::USER_AGENT, HeaderValue::from_static("UA-TEST"));
        let a = user_agent_hash(&headers);
        let b = user_agent_hash(&headers);
        assert_eq!(a, b);
    }
}
