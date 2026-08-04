use super::*;

#[test]
fn parses_split_token_login() {
    let mut data = Vec::new();
    data.extend_from_slice(&7u16.to_be_bytes());
    data.extend_from_slice(b"1_12345");
    data.extend_from_slice(&3u16.to_be_bytes());
    data.extend_from_slice(b"tok");

    assert_eq!(
        parse_login_request(&data).unwrap(),
        LoginRequest {
            account_id: "1_12345".into(),
            token: "tok".into(),
        }
    );
}

#[test]
fn parses_inline_token_login() {
    let account = b"1_12345#tok";
    let mut data = Vec::new();
    data.extend_from_slice(&(account.len() as u16).to_be_bytes());
    data.extend_from_slice(account);

    assert_eq!(
        parse_login_request(&data).unwrap(),
        LoginRequest {
            account_id: "1_12345".into(),
            token: "tok".into(),
        }
    );
}

#[test]
fn login_reply_matches_live_wire_shape() {
    assert_eq!(
        login_reply_payload(0x17eb591e),
        [0, 0, 0, 0, 0, 0, 0x17, 0xeb, 0x59, 0x1e]
    );
}

#[tokio::test]
async fn login_rechecks_allowlist_and_blacklist_before_accepting_token() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    database::run_migrations(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO users
            (id, username, email, token, token_expires_at, created_at, updated_at)
         VALUES (7, 'player_7', 'player01', 'valid-token', 9223372036854775807, 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO account_allowlist
            (account_key, display_account, batch_id, imported_at, imported_by, activated_user_id)
         VALUES ('player01', 'player01', 'test', 1, 'test', 7)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let request = LoginRequest {
        account_id: "1_7".to_string(),
        token: "valid-token".to_string(),
    };
    assert_eq!(
        validate_login(&pool, request.clone()).await.unwrap(),
        LoginSession { user_id: 7 }
    );

    sqlx::query(
        "INSERT INTO account_blacklist
            (account_key, user_id, source, reason, created_at, created_by)
         VALUES ('player01', 7, 'manual_ban', 'test', 2, 'test')",
    )
    .execute(&pool)
    .await
    .unwrap();

    assert!(validate_login(&pool, request).await.is_err());
}
