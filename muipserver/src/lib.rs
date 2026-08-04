mod account_api;
mod mail_api;
mod routes;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::net::IpAddr;
use tokio::net::TcpListener;
use tracing::info;

#[derive(Debug, Clone)]
pub struct MuipOptions {
    pub host: String,
    pub port: u16,
    pub allow_unspecified_container_bind: bool,
    pub token: String,
    pub gm_addr: String,
    pub db: SqlitePool,
}

impl MuipOptions {
    pub fn from_config(db: SqlitePool) -> Self {
        Self {
            host: common::muip_host().to_string(),
            port: common::muip_port(),
            allow_unspecified_container_bind: common::muip_allow_unspecified_container_bind(),
            token: common::muip_token().to_string(),
            gm_addr: common::muip_gm_addr(),
            db,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GmRequest {
    Status,
    ListPlayers,
    Dungeons,
    Heroes { player_uid: i64 },
    Materials { query: MaterialQuery },
    DisconnectPlayer { player_uid: i64 },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GmResponse {
    pub retcode: i32,
    pub message: String,
    #[serde(default)]
    pub online: usize,
    #[serde(default)]
    pub players: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl GmResponse {
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            retcode: 0,
            message: message.into(),
            ..Default::default()
        }
    }

    pub fn ok_data(message: impl Into<String>, data: impl Serialize) -> Self {
        Self {
            retcode: 0,
            message: message.into(),
            data: Some(serde_json::to_value(data).unwrap_or(serde_json::Value::Null)),
            ..Default::default()
        }
    }

    pub fn err(retcode: i32, message: impl Into<String>) -> Self {
        Self {
            retcode,
            message: message.into(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MaterialQuery {
    pub r#type: Option<i32>,
    pub q: Option<String>,
    pub limit: Option<usize>,
    pub player_uid: Option<i64>,
    pub unowned_only: Option<bool>,
}

pub async fn run(options: MuipOptions) -> anyhow::Result<()> {
    validate_bind_host(&options.host, options.allow_unspecified_container_bind)?;
    let addr = format!("{}:{}", options.host, options.port);
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("failed to bind MUIP server on {addr}"))?;

    info!("MUIP HTTP server listening on {}", listener.local_addr()?);
    axum::serve(listener, routes::router(options)).await?;
    Ok(())
}

fn validate_bind_host(host: &str, allow_unspecified_container_bind: bool) -> anyhow::Result<()> {
    let host: IpAddr = host.parse()?;
    if host.is_loopback() || (host.is_unspecified() && allow_unspecified_container_bind) {
        return Ok(());
    }
    anyhow::bail!("MUIP host must be loopback or an explicitly enabled unspecified container bind")
}

#[cfg(test)]
mod tests {
    use super::{GmRequest, validate_bind_host};

    #[test]
    fn disconnect_request_has_a_fixed_typed_shape() {
        let request: GmRequest =
            serde_json::from_str(r#"{"type":"disconnect_player","player_uid":31}"#).unwrap();
        assert!(matches!(
            request,
            GmRequest::DisconnectPlayer { player_uid: 31 }
        ));
    }

    #[test]
    fn container_bind_requires_an_explicit_flag_and_never_accepts_public_ips() {
        assert!(validate_bind_host("127.0.0.1", false).is_ok());
        assert!(validate_bind_host("0.0.0.0", false).is_err());
        assert!(validate_bind_host("0.0.0.0", true).is_ok());
        assert!(validate_bind_host("192.0.2.10", true).is_err());
    }
}
