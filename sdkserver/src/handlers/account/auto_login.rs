use super::helpers::*;
use crate::AppState;
use crate::models::request::AccountAutoLoginReq;
use crate::models::response::AccountLoginRsp;
use axum::{extract::State, response::Json};
use common::time::ServerTime;

pub async fn post(
    State(state): State<AppState>,
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
    use axum::{Json, extract::State};
    use reqwest::Client;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn blacklisted_auto_login_is_rejected_without_rotating_token() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        database::run_migrations(&db).await.unwrap();
        sqlx::query(
            "INSERT INTO users
                (id, username, email, token, refresh_token, token_expires_at, created_at, updated_at)
             VALUES (7, 'player_7', 'player01', 'original-token', 'original-refresh', 999999, 1, 1)",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO account_allowlist
                (account_key, display_account, batch_id, imported_at, imported_by, activated_user_id)
             VALUES ('player01', 'player01', 'test', 1, 'test', 7)",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO account_blacklist
                (account_key, user_id, source, reason, created_at, created_by)
             VALUES ('player01', 7, 'manual_ban', 'test', 2, 'test')",
        )
        .execute(&db)
        .await
        .unwrap();

        let request = serde_json::from_value(serde_json::json!({
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
            "token": "original-token",
            "userId": 7,
            "accountType": 10
        }))
        .unwrap();
        let state = AppState {
            sdk: SdkState {
                http_client: Client::new(),
            },
            db: db.clone(),
        };

        let response = post(State(state), Json(request)).await.0;
        assert_eq!(response.code, 401);
        let stored_token: String = sqlx::query_scalar("SELECT token FROM users WHERE id = 7")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(stored_token, "original-token");
    }

    #[tokio::test]
    async fn repeated_auto_login_is_idempotent() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        database::run_migrations(&db).await.unwrap();
        sqlx::query(
            "INSERT INTO users
                (id, username, email, token, refresh_token, token_expires_at, created_at, updated_at)
             VALUES (8, 'player_8', 'player08', 'stable-token', 'stable-refresh', 999999, 1, 1)",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO account_allowlist
                (account_key, display_account, batch_id, imported_at, imported_by, activated_user_id)
             VALUES ('player08', 'player08', 'test', 1, 'test', 8)",
        )
        .execute(&db)
        .await
        .unwrap();

        let request_json = serde_json::json!({
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
            "token": "stable-token",
            "userId": 8,
            "accountType": 10
        });
        let state = AppState {
            sdk: SdkState {
                http_client: Client::new(),
            },
            db: db.clone(),
        };

        for _ in 0..2 {
            let request = serde_json::from_value(request_json.clone()).unwrap();
            let response = post(State(state.clone()), Json(request)).await.0;
            assert_eq!(response.code, 200);
            assert_eq!(response.data.token, "stable-token");
            assert_eq!(response.data.refresh_token, "stable-refresh");
        }

        let stored_token: String = sqlx::query_scalar("SELECT token FROM users WHERE id = 8")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(stored_token, "stable-token");
    }

    #[tokio::test]
    async fn previous_password_login_token_recovers_to_current_token() {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        database::run_migrations(&db).await.unwrap();
        sqlx::query(
            "INSERT INTO users
                (id, username, email, token, refresh_token, token_expires_at, created_at, updated_at)
             VALUES (9, 'player_9', 'player09', 'previous-token', 'previous-refresh', ?1, 1, 1)",
        )
        .bind(i64::MAX / 2)
        .execute(&db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO account_allowlist
                (account_key, display_account, batch_id, imported_at, imported_by, activated_user_id)
             VALUES ('player09', 'player09', 'test', 1, 'test', 9)",
        )
        .execute(&db)
        .await
        .unwrap();
        let current = database::db::user::account::TokenInfo {
            token: "current-token".to_string(),
            refresh_token: "current-refresh".to_string(),
            expires_at: i64::MAX / 2,
        };
        database::db::user::account::update_user_login(&db, 9, &current, 2)
            .await
            .unwrap();

        let request = serde_json::from_value(serde_json::json!({
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
            "token": "previous-token",
            "userId": 9,
            "accountType": 10
        }))
        .unwrap();
        let state = AppState {
            sdk: SdkState {
                http_client: Client::new(),
            },
            db,
        };

        let response = post(State(state), Json(request)).await.0;
        assert_eq!(response.code, 200);
        assert_eq!(response.data.token, "current-token");
        assert_eq!(response.data.refresh_token, "current-refresh");
    }
}
