use super::helpers::get_user_by_id;
use crate::AppState;
use crate::access_control::{authorize, normalize_qq, trusted_client_ip};
use crate::models::request::AccountTokenRefreshReq;
use crate::models::response::{AccountTokenRefreshRsp, AccountTokenRefreshRspData};
use axum::{
    extract::{ConnectInfo, State},
    http::HeaderMap,
    response::Json,
};
use common::time::ServerTime;
use database::db::user::account::refresh_user_login_token;
use std::net::SocketAddr;

const TOKEN_LIFETIME_SECONDS: i64 = 7 * 24 * 60 * 60;

pub async fn post(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    axum::Json(req): axum::Json<AccountTokenRefreshReq>,
) -> Json<AccountTokenRefreshRsp> {
    let user = match get_user_by_id(&state, req.user_id).await {
        Ok(user) if user.refresh_token == req.refresh_token => user,
        Ok(_) | Err(_) => return Json(error_response()),
    };
    let qq = match normalize_qq(&user.email) {
        Ok(qq) => qq,
        Err(error) => {
            tracing::warn!(user_id = req.user_id, %error, "Token refresh rejected for non-QQ account");
            return Json(error_response());
        }
    };
    let ip = match trusted_client_ip(&headers, peer) {
        Ok(ip) => ip,
        Err(error) => {
            tracing::warn!(user_id = req.user_id, %error, "Token refresh rejected because client IP is invalid");
            return Json(error_response());
        }
    };
    if let Err(error) = authorize(&state, &qq, ip, true).await {
        tracing::warn!(user_id = req.user_id, %error, "Central access rejected token refresh");
        return Json(error_response());
    }

    let now = ServerTime::now_ms();
    let new_token = generate_token();
    let expires_at = now + TOKEN_LIFETIME_SECONDS * 1000;
    match refresh_user_login_token(
        &state.db,
        req.user_id,
        &req.refresh_token,
        &new_token,
        expires_at,
        now,
    )
    .await
    {
        Ok(true) => Json(AccountTokenRefreshRsp {
            code: 200,
            msg: "success".to_string(),
            data: AccountTokenRefreshRspData {
                token: new_token,
                expires_in: TOKEN_LIFETIME_SECONDS,
            },
        }),
        Ok(false) => Json(error_response()),
        Err(error) => {
            tracing::error!(user_id = req.user_id, %error, "Failed to refresh login token");
            Json(error_response())
        }
    }
}

fn error_response() -> AccountTokenRefreshRsp {
    AccountTokenRefreshRsp {
        code: 401,
        msg: "Invalid refresh token or user not found".to_string(),
        data: AccountTokenRefreshRspData::default(),
    }
}

fn generate_token() -> String {
    use rand::Rng;

    let mut rng = rand::rng();
    let mut bytes = [0u8; 16];
    rng.fill(&mut bytes);
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        + "200"
}

#[cfg(test)]
mod tests {
    use super::error_response;

    #[test]
    fn error_response_never_exposes_token_data() {
        let response = error_response();
        assert_eq!(response.code, 401);
        assert!(response.data.token.is_empty());
        assert_eq!(response.data.expires_in, 0);
    }
}
