use crate::AppState;
use crate::access_control::{authorize, normalize_qq, trusted_client_ip};
use crate::models::request::AccountLoginMailReq;
use crate::models::response::{AccountLoginRsp, AccountLoginRspData, AccountType, RealNameInfo};
use axum::{
    extract::{ConnectInfo, State},
    http::HeaderMap,
    response::Json,
};
use common::time::ServerTime;
use database::db::user::{
    access,
    account::{TokenInfo, get_user_by_email, handle_user_login, verify_user_password},
};
use std::net::SocketAddr;

pub async fn post(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    axum::Json(req): axum::Json<AccountLoginMailReq>,
) -> Json<AccountLoginRsp> {
    let now = ServerTime::now_ms();
    let qq = match normalize_qq(&req.account) {
        Ok(qq) => qq,
        Err(error) => {
            tracing::warn!("Rejected non-QQ account: {error}");
            return Json(create_error_response());
        }
    };
    let ip = match trusted_client_ip(&headers, peer) {
        Ok(ip) => ip,
        Err(error) => {
            tracing::warn!("Login rejected because client IP is invalid: {error}");
            return Json(create_error_response());
        }
    };

    tracing::info!(
        "Login attempt - QQ: {}, Device: {}, OS: {}",
        qq,
        req.device_info.device_name,
        req.device_info.os_version
    );

    // Generate tokens
    let token = generate_token();
    let refresh_token = generate_token();
    let expires_in = 7 * 24 * 60 * 60; // 7 days in seconds
    let token_expires_at = now + (expires_in * 1000);

    let token_info = TokenInfo {
        token: token.clone(),
        refresh_token: refresh_token.clone(),
        expires_at: token_expires_at,
    };

    let existing = match get_user_by_email(&state.db, &qq).await {
        Ok(user) => user,
        Err(error) => {
            tracing::error!("Failed to read QQ account before login: {error}");
            return Json(create_error_response());
        }
    };
    if let Some(user) = &existing {
        if access::require_login_access(&state.db, user.id, &qq)
            .await
            .is_err()
            || !matches!(
                verify_user_password(&state.db, &qq, &req.pwd).await,
                Ok(true)
            )
        {
            return Json(create_error_response());
        }
    } else if !(6..=64).contains(&req.pwd.chars().count()) {
        return Json(create_error_response());
    }
    if let Err(error) = authorize(&state, &qq, ip, existing.is_some()).await {
        tracing::warn!("Central access rejected QQ {qq}: {error}");
        return Json(create_error_response());
    }

    // Handle login with password verification after central authorization.
    let user = match handle_user_login(&state.db, &qq, &req.pwd, token_info, now).await {
        Ok(user) => user,
        Err(e) => {
            tracing::warn!("Login failed for QQ {}: {}", qq, e);
            return Json(create_error_response());
        }
    };

    tracing::info!(
        "Login successful - User ID: {}, QQ: {}, First join: {}",
        user.id,
        qq,
        user.first_join
    );

    let rsp = AccountLoginRsp {
        code: 200,
        msg: "success".to_string(),
        data: AccountLoginRspData {
            token,
            expires_in,
            refresh_token,
            user_id: user.id as u64, // Use actual user ID from database
            account_type: AccountType::Email,
            registration_account_type: 1,
            account: user.email.clone(),
            real_name_info: RealNameInfo {
                need_real_name: user.need_real_name,
                real_name_status: user.real_name_status,
                age: user.age as u8,
                adult: user.is_adult,
            },
            need_activate: user.need_activate,
            cipher_mark: user.cipher_mark,
            first_join: user.first_join,
            account_tags: user.account_tags,
        },
    };

    Json(rsp)
}

fn generate_token() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let mut bytes = [0u8; 16];
    rng.fill(&mut bytes);
    bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
        + "200"
}

fn create_error_response() -> AccountLoginRsp {
    AccountLoginRsp {
        code: 401,
        msg: "Invalid QQ number, password, or access policy".to_string(),
        data: AccountLoginRspData {
            token: String::new(),
            expires_in: 0,
            refresh_token: String::new(),
            user_id: 0,
            account_type: AccountType::Email,
            registration_account_type: 0,
            account: String::new(),
            real_name_info: RealNameInfo {
                need_real_name: false,
                real_name_status: false,
                age: 0,
                adult: false,
            },
            need_activate: false,
            cipher_mark: false,
            first_join: false,
            account_tags: String::new(),
        },
    }
}
