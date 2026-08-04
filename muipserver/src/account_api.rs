use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use database::db::user::access;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use std::{
    collections::HashSet,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

use crate::{GmRequest, GmResponse};

const MAX_IMPORT_ACCOUNTS: usize = 10_000;
const MAX_REASON_CHARS: usize = 200;

#[derive(Clone)]
pub(crate) struct ApiState {
    token: Arc<str>,
    gm_addr: Arc<str>,
    db: SqlitePool,
}

impl ApiState {
    pub(crate) fn new(token: String, gm_addr: String, db: SqlitePool) -> Self {
        Self {
            token: Arc::from(token),
            gm_addr: Arc::from(gm_addr),
            db,
        }
    }

    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    pub(crate) fn db(&self) -> &SqlitePool {
        &self.db
    }
}

#[derive(Debug)]
pub(crate) struct ApiError {
    status: StatusCode,
    message: &'static str,
}

impl ApiError {
    fn invalid(message: &'static str) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message,
        }
    }

    fn conflict(message: &'static str) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message,
        }
    }

    fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: "request not found",
        }
    }

    fn database(error: sqlx::Error) -> Self {
        tracing::error!("Reverse1999 admin database operation failed: {error}");
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: "operation unavailable",
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct AccountInput {
    account: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountPreview {
    account: String,
    user_id: Option<i64>,
    role_name: String,
    allowlisted: bool,
    blacklisted: bool,
    online: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ImportInput {
    accounts: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportPreview {
    valid: usize,
    invalid: usize,
    duplicates: usize,
    existing: usize,
    conflicts: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BanInput {
    reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OperationResponse {
    request_id: String,
    state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LedgerResult {
    request_id: String,
    state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user_id: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RequestStatus {
    request_id: String,
    state: String,
    operation: String,
}

#[derive(Debug)]
struct NormalizedBatch {
    accounts: Vec<(String, String)>,
    invalid: usize,
    duplicates: usize,
}

#[derive(Debug)]
struct RequestRow {
    operation: String,
    payload_sha256: String,
    status: String,
    result_json: String,
}

pub(crate) async fn preview_account(
    State(state): State<ApiState>,
    Json(input): Json<AccountInput>,
) -> Result<Json<AccountPreview>, ApiError> {
    let account = normalize(&input.account)?;
    let user = sqlx::query("SELECT id, username FROM users WHERE LOWER(TRIM(email)) = ? LIMIT 1")
        .bind(&account)
        .fetch_optional(&state.db)
        .await
        .map_err(ApiError::database)?;
    let user_id = user.as_ref().map(|row| row.get::<i64, _>("id"));
    let role_name = user
        .as_ref()
        .map(|row| row.get::<String, _>("username"))
        .unwrap_or_default();
    let allowlisted = exists(
        &state.db,
        "SELECT 1 FROM account_allowlist WHERE account_key = ? LIMIT 1",
        &account,
    )
    .await?;
    let blacklisted = exists(
        &state.db,
        "SELECT 1 FROM account_blacklist WHERE account_key = ? LIMIT 1",
        &account,
    )
    .await?;
    let online = match send_gm(&state.gm_addr, GmRequest::ListPlayers).await {
        Ok(response) => {
            user_id.is_some_and(|id| response.players.iter().any(|v| v == &id.to_string()))
        }
        Err(_) => false,
    };

    Ok(Json(AccountPreview {
        account,
        user_id,
        role_name,
        allowlisted,
        blacklisted,
        online,
    }))
}

pub(crate) async fn preview_import(
    State(state): State<ApiState>,
    Json(input): Json<ImportInput>,
) -> Result<Json<ImportPreview>, ApiError> {
    Ok(Json(
        preview_batch(&state.db, normalize_batch(input.accounts)?).await?,
    ))
}

pub(crate) async fn preview_replace(
    State(state): State<ApiState>,
    Json(input): Json<ImportInput>,
) -> Result<Json<ImportPreview>, ApiError> {
    Ok(Json(
        preview_batch(&state.db, normalize_batch(input.accounts)?).await?,
    ))
}

pub(crate) async fn apply_import(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(input): Json<ImportInput>,
) -> Result<Json<OperationResponse>, ApiError> {
    apply_batch(&state, &headers, input.accounts, false)
        .await
        .map(Json)
}

pub(crate) async fn apply_replace(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(input): Json<ImportInput>,
) -> Result<Json<OperationResponse>, ApiError> {
    apply_batch(&state, &headers, input.accounts, true)
        .await
        .map(Json)
}

pub(crate) async fn ban(
    State(state): State<ApiState>,
    Path(account): Path<String>,
    headers: HeaderMap,
    Json(input): Json<BanInput>,
) -> Result<Response, ApiError> {
    let request_id = idempotency_key(&headers)?;
    let account = normalize(&account)?;
    let reason = input.reason.trim();
    if reason.is_empty() || reason.chars().count() > MAX_REASON_CHARS {
        return Err(ApiError::invalid("invalid ban reason"));
    }
    let payload_hash = hash_payload(&serde_json::json!({
        "account": account,
        "reason": reason,
    }))?;
    let mut tx = state
        .db
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(ApiError::database)?;

    if let Some(existing) = request_row(&mut tx, &request_id).await? {
        ensure_same_request(&existing, "ban", &payload_hash)?;
        let result = ledger_result(&existing, &request_id);
        tx.rollback().await.map_err(ApiError::database)?;
        if existing.status == "applied" {
            return Ok((StatusCode::OK, Json(operation_response(&result))).into_response());
        }
        return finish_disconnect(&state, result).await;
    }

    let user_id =
        sqlx::query_scalar::<_, i64>("SELECT id FROM users WHERE LOWER(TRIM(email)) = ? LIMIT 1")
            .bind(&account)
            .fetch_optional(&mut *tx)
            .await
            .map_err(ApiError::database)?;
    let now = now_ms();
    sqlx::query(
        "INSERT INTO account_blacklist
            (account_key, user_id, source, reason, created_at, created_by)
         VALUES (?, ?, 'manual_ban', ?, ?, 'platform-admin')
         ON CONFLICT(account_key) DO UPDATE SET
            user_id = excluded.user_id,
            source = 'manual_ban',
            reason = excluded.reason,
            created_at = excluded.created_at,
            created_by = excluded.created_by",
    )
    .bind(&account)
    .bind(user_id)
    .bind(reason)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(ApiError::database)?;
    if let Some(user_id) = user_id {
        sqlx::query(
            "UPDATE users SET token = '', refresh_token = '', token_expires_at = 0, updated_at = ?
             WHERE id = ?",
        )
        .bind(now)
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::database)?;
    }
    let result = LedgerResult {
        request_id: request_id.clone(),
        state: "prepared".to_string(),
        user_id,
    };
    insert_request(&mut tx, &request_id, "ban", &payload_hash, &result).await?;
    tx.commit().await.map_err(ApiError::database)?;

    finish_disconnect(&state, result).await
}

pub(crate) async fn unban(
    State(state): State<ApiState>,
    Path(account): Path<String>,
    headers: HeaderMap,
) -> Result<Json<OperationResponse>, ApiError> {
    let request_id = idempotency_key(&headers)?;
    let account = normalize(&account)?;
    let payload_hash = hash_payload(&serde_json::json!({ "account": account }))?;
    let mut tx = state
        .db
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(ApiError::database)?;
    if let Some(existing) = request_row(&mut tx, &request_id).await? {
        ensure_same_request(&existing, "unban", &payload_hash)?;
        let result = ledger_result(&existing, &request_id);
        tx.rollback().await.map_err(ApiError::database)?;
        return Ok(Json(operation_response(&result)));
    }

    sqlx::query("DELETE FROM account_blacklist WHERE account_key = ?")
        .bind(&account)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::database)?;
    let result = LedgerResult {
        request_id: request_id.clone(),
        state: "applied".to_string(),
        user_id: None,
    };
    insert_request(&mut tx, &request_id, "unban", &payload_hash, &result).await?;
    mark_request(&mut tx, &request_id, "applied", &result).await?;
    tx.commit().await.map_err(ApiError::database)?;
    Ok(Json(operation_response(&result)))
}

pub(crate) async fn request_status(
    State(state): State<ApiState>,
    Path(request_id): Path<String>,
) -> Result<Json<RequestStatus>, ApiError> {
    let row =
        sqlx::query("SELECT operation, status FROM account_admin_requests WHERE request_id = ?")
            .bind(&request_id)
            .fetch_optional(&state.db)
            .await
            .map_err(ApiError::database)?
            .ok_or_else(ApiError::not_found)?;
    Ok(Json(RequestStatus {
        request_id,
        state: row.get("status"),
        operation: row.get("operation"),
    }))
}

async fn apply_batch(
    state: &ApiState,
    headers: &HeaderMap,
    accounts: Vec<String>,
    replace: bool,
) -> Result<OperationResponse, ApiError> {
    let request_id = idempotency_key(headers)?;
    let batch = normalize_batch(accounts)?;
    if batch.accounts.is_empty() || batch.invalid > 0 {
        return Err(ApiError::invalid("allowlist contains invalid accounts"));
    }
    let preview = preview_batch(
        &state.db,
        NormalizedBatch {
            accounts: batch.accounts.clone(),
            invalid: batch.invalid,
            duplicates: batch.duplicates,
        },
    )
    .await?;
    if preview.conflicts > 0 {
        return Err(ApiError::conflict("allowlist conflicts with blacklist"));
    }
    let keys = batch
        .accounts
        .iter()
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    let payload_hash = hash_payload(&serde_json::json!({
        "accounts": keys,
        "replace": replace,
    }))?;
    let operation = if replace {
        "allowlist_replace"
    } else {
        "allowlist_import"
    };
    let mut tx = state
        .db
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(ApiError::database)?;
    if let Some(existing) = request_row(&mut tx, &request_id).await? {
        ensure_same_request(&existing, operation, &payload_hash)?;
        let result = ledger_result(&existing, &request_id);
        tx.rollback().await.map_err(ApiError::database)?;
        return Ok(operation_response(&result));
    }
    ensure_no_blacklist_conflicts(
        &mut tx,
        &batch
            .accounts
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>(),
    )
    .await?;
    let prepared = LedgerResult {
        request_id: request_id.clone(),
        state: "prepared".to_string(),
        user_id: None,
    };
    insert_request(&mut tx, &request_id, operation, &payload_hash, &prepared).await?;

    if replace {
        let keep = batch
            .accounts
            .iter()
            .map(|(key, _)| key.as_str())
            .collect::<HashSet<_>>();
        let existing = sqlx::query_scalar::<_, String>("SELECT account_key FROM account_allowlist")
            .fetch_all(&mut *tx)
            .await
            .map_err(ApiError::database)?;
        for key in existing {
            if !keep.contains(key.as_str()) {
                sqlx::query("DELETE FROM account_allowlist WHERE account_key = ?")
                    .bind(key)
                    .execute(&mut *tx)
                    .await
                    .map_err(ApiError::database)?;
            }
        }
    }
    let now = now_ms();
    for (key, display) in batch.accounts {
        sqlx::query(
            "INSERT OR IGNORE INTO account_allowlist
                (account_key, display_account, batch_id, imported_at, imported_by)
             VALUES (?, ?, ?, ?, 'platform-admin')",
        )
        .bind(key)
        .bind(display)
        .bind(&request_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(ApiError::database)?;
    }
    let applied = LedgerResult {
        request_id: request_id.clone(),
        state: "applied".to_string(),
        user_id: None,
    };
    mark_request(&mut tx, &request_id, "applied", &applied).await?;
    tx.commit().await.map_err(ApiError::database)?;
    Ok(operation_response(&applied))
}

async fn finish_disconnect(
    state: &ApiState,
    mut result: LedgerResult,
) -> Result<Response, ApiError> {
    let applied = match result.user_id {
        None => true,
        Some(user_id) => send_gm(
            &state.gm_addr,
            GmRequest::DisconnectPlayer {
                player_uid: user_id,
            },
        )
        .await
        .is_ok_and(|response| response.retcode == 0),
    };
    result.state = if applied { "applied" } else { "unknown" }.to_string();
    let status = if applied {
        StatusCode::OK
    } else {
        StatusCode::ACCEPTED
    };
    if let Err(error) =
        update_request_state(&state.db, &result.request_id, &result.state, &result).await
    {
        tracing::error!("failed to update Reverse1999 request state: {error}");
    }
    Ok((status, Json(operation_response(&result))).into_response())
}

fn operation_response(result: &LedgerResult) -> OperationResponse {
    OperationResponse {
        request_id: result.request_id.clone(),
        state: result.state.clone(),
    }
}

fn normalize(value: &str) -> Result<String, ApiError> {
    access::normalize_account(value).map_err(|_| ApiError::invalid("invalid account format"))
}

fn normalize_batch(accounts: Vec<String>) -> Result<NormalizedBatch, ApiError> {
    if accounts.len() > MAX_IMPORT_ACCOUNTS {
        return Err(ApiError::invalid("allowlist batch is too large"));
    }
    let mut invalid = 0;
    let mut duplicates = 0;
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for display in accounts {
        let key = match access::normalize_account(&display) {
            Ok(key) => key,
            Err(_) => {
                invalid += 1;
                continue;
            }
        };
        if !seen.insert(key.clone()) {
            duplicates += 1;
            continue;
        }
        normalized.push((key, display.trim().to_string()));
    }
    normalized.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(NormalizedBatch {
        accounts: normalized,
        invalid,
        duplicates,
    })
}

async fn preview_batch(db: &SqlitePool, batch: NormalizedBatch) -> Result<ImportPreview, ApiError> {
    let existing = sqlx::query_scalar::<_, String>("SELECT account_key FROM account_allowlist")
        .fetch_all(db)
        .await
        .map_err(ApiError::database)?
        .into_iter()
        .collect::<HashSet<_>>();
    let blacklist = sqlx::query_scalar::<_, String>("SELECT account_key FROM account_blacklist")
        .fetch_all(db)
        .await
        .map_err(ApiError::database)?
        .into_iter()
        .collect::<HashSet<_>>();
    Ok(ImportPreview {
        valid: batch.accounts.len(),
        invalid: batch.invalid,
        duplicates: batch.duplicates,
        existing: batch
            .accounts
            .iter()
            .filter(|(key, _)| existing.contains(key))
            .count(),
        conflicts: batch
            .accounts
            .iter()
            .filter(|(key, _)| blacklist.contains(key))
            .count(),
    })
}

async fn ensure_no_blacklist_conflicts(
    tx: &mut Transaction<'_, Sqlite>,
    accounts: &[String],
) -> Result<(), ApiError> {
    for account in accounts {
        let blocked = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM account_blacklist WHERE account_key = ? LIMIT 1",
        )
        .bind(account)
        .fetch_optional(&mut **tx)
        .await
        .map_err(ApiError::database)?
        .is_some();
        if blocked {
            return Err(ApiError::conflict("allowlist conflicts with blacklist"));
        }
    }
    Ok(())
}

fn idempotency_key(headers: &HeaderMap) -> Result<String, ApiError> {
    let value = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.')
                })
        })
        .ok_or_else(|| ApiError::invalid("invalid idempotency key"))?;
    Ok(value.to_string())
}

fn hash_payload(value: &impl Serialize) -> Result<String, ApiError> {
    let bytes =
        serde_json::to_vec(value).map_err(|_| ApiError::invalid("invalid request payload"))?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

async fn exists(db: &SqlitePool, query: &str, key: &str) -> Result<bool, ApiError> {
    Ok(sqlx::query_scalar::<_, i64>(query)
        .bind(key)
        .fetch_optional(db)
        .await
        .map_err(ApiError::database)?
        .is_some())
}

async fn request_row(
    tx: &mut Transaction<'_, Sqlite>,
    request_id: &str,
) -> Result<Option<RequestRow>, ApiError> {
    sqlx::query(
        "SELECT operation, payload_sha256, status, result_json
         FROM account_admin_requests WHERE request_id = ?",
    )
    .bind(request_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(ApiError::database)
    .map(|row| {
        row.map(|row| RequestRow {
            operation: row.get("operation"),
            payload_sha256: row.get("payload_sha256"),
            status: row.get("status"),
            result_json: row.get("result_json"),
        })
    })
}

fn ensure_same_request(
    row: &RequestRow,
    operation: &str,
    payload_hash: &str,
) -> Result<(), ApiError> {
    if row.operation != operation || row.payload_sha256 != payload_hash {
        return Err(ApiError::conflict("idempotency key payload mismatch"));
    }
    Ok(())
}

fn ledger_result(row: &RequestRow, request_id: &str) -> LedgerResult {
    serde_json::from_str(&row.result_json).unwrap_or_else(|_| LedgerResult {
        request_id: request_id.to_string(),
        state: row.status.clone(),
        user_id: None,
    })
}

async fn insert_request(
    tx: &mut Transaction<'_, Sqlite>,
    request_id: &str,
    operation: &str,
    payload_hash: &str,
    result: &LedgerResult,
) -> Result<(), ApiError> {
    let now = now_ms();
    let result_json =
        serde_json::to_string(result).map_err(|_| ApiError::invalid("invalid request result"))?;
    sqlx::query(
        "INSERT INTO account_admin_requests
            (request_id, operation, payload_sha256, status, result_json, created_at, updated_at)
         VALUES (?, ?, ?, 'prepared', ?, ?, ?)",
    )
    .bind(request_id)
    .bind(operation)
    .bind(payload_hash)
    .bind(result_json)
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::database)?;
    Ok(())
}

async fn mark_request(
    tx: &mut Transaction<'_, Sqlite>,
    request_id: &str,
    status: &str,
    result: &LedgerResult,
) -> Result<(), ApiError> {
    let result_json =
        serde_json::to_string(result).map_err(|_| ApiError::invalid("invalid request result"))?;
    sqlx::query(
        "UPDATE account_admin_requests SET status = ?, result_json = ?, updated_at = ?
         WHERE request_id = ?",
    )
    .bind(status)
    .bind(result_json)
    .bind(now_ms())
    .bind(request_id)
    .execute(&mut **tx)
    .await
    .map_err(ApiError::database)?;
    Ok(())
}

async fn update_request_state(
    db: &SqlitePool,
    request_id: &str,
    status: &str,
    result: &LedgerResult,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE account_admin_requests SET status = ?, result_json = ?, updated_at = ?
         WHERE request_id = ?",
    )
    .bind(status)
    .bind(serde_json::to_string(result).unwrap_or_else(|_| "{}".to_string()))
    .bind(now_ms())
    .bind(request_id)
    .execute(db)
    .await?;
    Ok(())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

async fn send_gm(addr: &str, request: GmRequest) -> anyhow::Result<GmResponse> {
    let mut stream = TcpStream::connect(addr).await?;
    let mut payload = serde_json::to_vec(&request)?;
    payload.push(b'\n');
    stream.write_all(&payload).await?;
    stream.flush().await?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let trimmed = line.trim();
    if trimmed.is_empty() {
        anyhow::bail!("empty GM response");
    }
    Ok(serde_json::from_str(trimmed)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn state() -> ApiState {
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        database::run_migrations(&db).await.unwrap();
        ApiState::new("token".to_string(), "127.0.0.1:9".to_string(), db)
    }

    #[tokio::test]
    async fn import_preview_counts_normalized_duplicates_and_conflicts() {
        let state = state().await;
        sqlx::query(
            "INSERT INTO account_allowlist
                (account_key, display_account, batch_id, imported_at, imported_by)
             VALUES ('existing', 'existing', 'test', 1, 'test')",
        )
        .execute(&state.db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO account_blacklist
                (account_key, source, reason, created_at, created_by)
             VALUES ('blocked', 'manual_ban', 'test', 1, 'test')",
        )
        .execute(&state.db)
        .await
        .unwrap();

        let preview = preview_batch(
            &state.db,
            normalize_batch(vec![
                "Player01".to_string(),
                "player01".to_string(),
                "existing".to_string(),
                "blocked".to_string(),
                "bad account".to_string(),
            ])
            .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(
            preview,
            ImportPreview {
                valid: 3,
                invalid: 1,
                duplicates: 1,
                existing: 1,
                conflicts: 1,
            }
        );
    }

    #[tokio::test]
    async fn ban_revokes_tokens_and_records_unknown_when_disconnect_is_unavailable() {
        let state = state().await;
        sqlx::query(
            "INSERT INTO users
                (id, username, email, token, refresh_token, token_expires_at, created_at, updated_at)
             VALUES (7, 'player_7', 'player01', 'token', 'refresh', 99, 1, 1)",
        )
        .execute(&state.db)
        .await
        .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("idempotency-key", "ban-1".parse().unwrap());
        let response = ban(
            State(state.clone()),
            Path("player01".to_string()),
            headers,
            Json(BanInput {
                reason: "test".to_string(),
            }),
        )
        .await
        .unwrap();

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let token: String = sqlx::query_scalar("SELECT token FROM users WHERE id = 7")
            .fetch_one(&state.db)
            .await
            .unwrap();
        let status: String = sqlx::query_scalar(
            "SELECT status FROM account_admin_requests WHERE request_id = 'ban-1'",
        )
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(token, "");
        assert_eq!(status, "unknown");
    }

    #[tokio::test]
    async fn transactional_import_check_rejects_blacklist_conflicts() {
        let state = state().await;
        sqlx::query(
            "INSERT INTO account_blacklist
                (account_key, source, reason, created_at, created_by)
             VALUES ('blocked', 'manual_ban', 'test', 1, 'test')",
        )
        .execute(&state.db)
        .await
        .unwrap();
        let mut tx = state.db.begin_with("BEGIN IMMEDIATE").await.unwrap();
        let error = ensure_no_blacklist_conflicts(&mut tx, &["blocked".to_string()])
            .await
            .unwrap_err();
        assert_eq!(error.status, StatusCode::CONFLICT);
        tx.rollback().await.unwrap();
    }
}
