use super::helpers::*;
use crate::AppState;
use crate::access_control::{authorize, qq_identity_from_stored_account, trusted_client_ip};
use crate::models::request::AccountAutoLoginReq;
use crate::models::response::AccountLoginRsp;
use axum::{
    extract::{ConnectInfo, State},
    http::HeaderMap,
    response::Json,
};
use common::time::ServerTime;
use std::net::SocketAddr;

pub async fn post(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    axum::Json(req): axum::Json<AccountAutoLoginReq>,
) -> Json<AccountLoginRsp> {
    tracing::info!("Auto-login attempt - User ID: {}", req.user_id);

    // Validate token and get user
    let user = match get_user_with_token_validation(&state, req.user_id as i64, &req.token).await {
        Ok(user) => user,
        Err(e) => {
            tracing::warn!("Auto-login failed: {}", e);
            return Json(create_auth_error_response());
        }
    };

    let qq = match qq_identity_from_stored_account(&user.email) {
        Ok(qq) => qq,
        Err(error) => {
            tracing::warn!("Auto-login rejected for non-QQ account: {error}");
            return Json(create_auth_error_response());
        }
    };
    let ip = match trusted_client_ip(&headers, peer) {
        Ok(ip) => ip,
        Err(error) => {
            tracing::warn!("Auto-login rejected because client IP is invalid: {error}");
            return Json(create_auth_error_response());
        }
    };
    if let Err(error) = authorize(&state, &qq, ip, true).await {
        tracing::warn!("Central access rejected auto-login for QQ {qq}: {error}");
        return Json(create_auth_error_response());
    }

    let now = ServerTime::now_ms();
    if let Err(e) =
        database::db::user::account::touch_user_login(&state.db, user.user_id, now).await
    {
        tracing::error!("Failed to update last login time: {}", e);
    }

    tracing::info!("Auto-login successful for user {}", req.user_id);
    Json(build_login_response(
        &user,
        user.token.clone(),
        user.refresh_token.clone(),
    ))
}

#[cfg(test)]
mod tests {
    use super::post;
    use crate::{AppState, SdkState};
    use axum::{
        Json,
        extract::{ConnectInfo, State},
        http::HeaderMap,
    };
    use reqwest::Client;
    use sqlx::sqlite::SqlitePoolOptions;
    use std::net::SocketAddr;

    fn request(user_id: u64, token: &str) -> crate::models::request::AccountAutoLoginReq {
        serde_json::from_value(serde_json::json!({
            "deviceInfo": {
                "networkName": "test", "deviceId": "test", "cnadid": "", "oaId": "",
                "androidId": "", "imsi": "", "imei": "", "uuid": "test",
                "deviceName": "test", "deviceManufacturer": "test", "osType": 1,
                "osVersion": "test", "apiLevel": "test", "language": "zh-CN",
                "displayWidth": "1", "displayHeight": "1", "hardware": "test",
                "buildName": "test", "distinctId": "test", "anonymousId": "test"
            },
            "appPackageInfo": {
                "appPackageName": "test", "appVersion": 1, "appVersionName": "test",
                "gameId": 60001, "gameCode": "test", "gameName": "test",
                "channelId": "200", "subChannelId": "200", "appInstallTime": "1",
                "appUpdateTime": "1", "appSignature": "test", "sdkVersion": "test",
                "channelVersion": "test", "adFid": "", "gclid": "", "dataAppId": ""
            },
            "reactivate": false,
            "token": token,
            "userId": user_id,
            "accountType": 10
        }))
        .unwrap()
    }

    async fn state() -> AppState {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        database::run_migrations(&db).await.unwrap();
        AppState {
            sdk: SdkState {
                http_client: Client::new(),
            },
            db,
        }
    }

    async fn auto_login(
        state: AppState,
        request: crate::models::request::AccountAutoLoginReq,
    ) -> crate::models::response::AccountLoginRsp {
        post(
            State(state),
            ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 32019))),
            HeaderMap::new(),
            Json(request),
        )
        .await
        .0
    }

    #[tokio::test]
    async fn blacklisted_auto_login_is_rejected_without_rotating_token() {
        let state = state().await;
        sqlx::query(
            "INSERT INTO users
                (id, username, email, token, refresh_token, created_at, updated_at)
             VALUES (7, 'player_7', '1234507', 'original-token', 'original-refresh', 1, 1);
             INSERT INTO account_blacklist
                (account_key, user_id, source, reason, created_at, created_by)
             VALUES ('1234507', 7, 'manual_ban', 'test', 2, 'test');",
        )
        .execute(&state.db)
        .await
        .unwrap();

        let response = auto_login(state.clone(), request(7, "original-token")).await;
        assert_eq!(response.code, 401);
        let stored_token: String = sqlx::query_scalar("SELECT token FROM users WHERE id = 7")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(stored_token, "original-token");
    }

    #[tokio::test]
    async fn repeated_auto_login_preserves_current_tokens_without_legacy_allowlist() {
        let state = state().await;
        sqlx::query(
            "INSERT INTO users
                (id, username, email, token, refresh_token, created_at, updated_at)
             VALUES (8, 'player_8', '1234508', 'stable-token', 'stable-refresh', 1, 1)",
        )
        .execute(&state.db)
        .await
        .unwrap();

        for _ in 0..2 {
            let response = auto_login(state.clone(), request(8, "stable-token")).await;
            assert_eq!(response.code, 200);
            assert_eq!(response.data.token, "stable-token");
            assert_eq!(response.data.refresh_token, "stable-refresh");
        }
        let allowlist_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM account_allowlist")
            .fetch_one(&state.db)
            .await
            .unwrap();
        assert_eq!(allowlist_count, 0);
    }
}
