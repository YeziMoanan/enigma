use crate::AppState;
use anyhow::{Context, Result, bail};
use axum::http::HeaderMap;
#[cfg(not(test))]
use rand::Rng;
#[cfg(not(test))]
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};

#[cfg(not(test))]
const GAME_ID: &str = "reverse1999";
#[cfg(not(test))]
const SERVER_ID: &str = "reverse1999-main";
#[cfg(not(test))]
const DEFAULT_API: &str = "http://127.0.0.1:31063/api/v1";

#[cfg(not(test))]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DecisionRequest<'a> {
    game_id: &'static str,
    server_id: &'static str,
    identity: Identity<'a>,
    game_account: &'a str,
    ip: String,
    request_id: String,
}

#[cfg(not(test))]
#[derive(Debug, Serialize)]
struct Identity<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    value: &'a str,
}

#[cfg(not(test))]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Decision {
    allowed: bool,
    action: String,
    reason_code: String,
    policy_version: String,
}

pub fn normalize_qq(value: &str) -> Result<String> {
    let value = value.trim();
    if !(5..=12).contains(&value.len())
        || value.starts_with('0')
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        bail!("account must be a 5-12 digit QQ number");
    }
    Ok(value.to_string())
}

pub fn qq_identity_from_stored_account(value: &str) -> Result<String> {
    if let Ok(qq) = normalize_qq(value) {
        return Ok(qq);
    }
    let value = value.trim().to_ascii_lowercase();
    let Some(qq) = value.strip_suffix("@qq.com") else {
        bail!("stored account has no QQ identity");
    };
    normalize_qq(qq)
}

pub fn trusted_client_ip(headers: &HeaderMap, peer: SocketAddr) -> Result<IpAddr> {
    if peer.ip().is_loopback() {
        if let Some(value) = headers.get("x-real-ip") {
            let value = value.to_str().context("invalid X-Real-IP header")?;
            return value.trim().parse().context("invalid X-Real-IP address");
        }
    }
    Ok(peer.ip())
}

pub async fn authorize(
    state: &AppState,
    qq: &str,
    ip: IpAddr,
    existing_account: bool,
) -> Result<()> {
    #[cfg(test)]
    {
        let _ = (state, qq, ip, existing_account);
        return Ok(());
    }

    #[cfg(not(test))]
    {
        if existing_account {
            check(state, "access/account-bind", qq, ip).await?;
            check(state, "access/login-check", qq, ip).await
        } else {
            check(state, "access/register-check", qq, ip).await
        }
    }
}

#[cfg(not(test))]
async fn check(state: &AppState, endpoint: &str, qq: &str, ip: IpAddr) -> Result<()> {
    let base =
        std::env::var("CENTRAL_ACCESS_CONTROL_URL").unwrap_or_else(|_| DEFAULT_API.to_string());
    let request = DecisionRequest {
        game_id: GAME_ID,
        server_id: SERVER_ID,
        identity: Identity {
            kind: "qq",
            value: qq,
        },
        game_account: qq,
        ip: ip.to_string(),
        request_id: request_id(),
    };
    let response = state
        .sdk
        .http_client
        .post(format!("{}/{}", base.trim_end_matches('/'), endpoint))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(serde_json::to_vec(&request).context("encode central access request")?)
        .send()
        .await
        .context("central access-control request failed")?;
    let status = response.status();
    let response = response
        .error_for_status()
        .with_context(|| format!("central access-control returned HTTP {status}"))?;
    let body = response
        .bytes()
        .await
        .context("read central access-control response body")?;
    let decision: Decision =
        serde_json::from_slice(&body).context("invalid central access-control response")?;
    if !decision.allowed || decision.action != "ALLOW" {
        bail!(
            "central access denied: reason={}, action={}, policy={}",
            decision.reason_code,
            decision.action,
            decision.policy_version
        );
    }
    Ok(())
}

#[cfg(not(test))]
fn request_id() -> String {
    let mut rng = rand::rng();
    let mut bytes = [0u8; 16];
    rng.fill(&mut bytes);
    let suffix = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("reverse1999-{suffix}")
}

#[cfg(test)]
mod tests {
    use super::{normalize_qq, qq_identity_from_stored_account, trusted_client_ip};
    use axum::http::{HeaderMap, HeaderValue};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[test]
    fn accepts_only_plain_qq_numbers() {
        assert_eq!(normalize_qq(" 12345678 ").unwrap(), "12345678");
        for invalid in [
            "1234",
            "012345",
            "1234567890123",
            "123456@qq.com",
            "player01",
        ] {
            assert!(normalize_qq(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn derives_qq_only_from_legacy_stored_qq_email() {
        assert_eq!(
            qq_identity_from_stored_account(" 12345678@QQ.COM ").unwrap(),
            "12345678"
        );
        assert!(qq_identity_from_stored_account("player@example.com").is_err());
    }

    #[test]
    fn trusts_real_ip_only_from_loopback_proxy() {
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("203.0.113.8"));
        let loopback = SocketAddr::from(([127, 0, 0, 1], 1234));
        assert_eq!(
            trusted_client_ip(&headers, loopback).unwrap(),
            IpAddr::V4(Ipv4Addr::new(203, 0, 113, 8))
        );

        let remote = SocketAddr::from(([198, 51, 100, 9], 1234));
        assert_eq!(trusted_client_ip(&headers, remote).unwrap(), remote.ip());
    }
}
