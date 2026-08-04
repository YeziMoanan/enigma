use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use database::db::{game::mail_campaign, user::access};
use logic::mail::catalog::{CatalogEntry, build_initial_catalog};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, Transaction};
use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::account_api::ApiState;

const MAX_TITLE_CHARS: usize = 80;
const MAX_BODY_CHARS: usize = 1_000;

#[derive(Debug)]
pub(crate) struct MailApiError {
    status: StatusCode,
    message: &'static str,
}

impl MailApiError {
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

    fn unavailable(error: impl std::fmt::Display) -> Self {
        tracing::error!("Reverse1999 mail operation failed: {error}");
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: "operation unavailable",
        }
    }
}

impl IntoResponse for MailApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.message })),
        )
            .into_response()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MailAttachment {
    material_type: i32,
    id: i32,
    quantity: i32,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct SendMailInput {
    title: String,
    body: String,
    attachments: Vec<MailAttachment>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CampaignInput {
    campaign_id: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MailOperation {
    request_id: String,
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    mail_incr_id: Option<i64>,
    #[serde(default)]
    accounts: usize,
    #[serde(default)]
    mails: usize,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CampaignPreview {
    campaign_id: String,
    eligible_accounts: usize,
    pending_mails: usize,
}

#[derive(Debug)]
struct LedgerRow {
    operation: String,
    payload_sha256: String,
    result_json: String,
}

pub(crate) async fn catalog() -> Result<Json<Vec<CatalogEntry>>, MailApiError> {
    Ok(Json(build_initial_catalog(config::configs::get())))
}

pub(crate) async fn send_mail(
    State(state): State<ApiState>,
    Path(account): Path<String>,
    headers: HeaderMap,
    Json(input): Json<SendMailInput>,
) -> Result<Json<MailOperation>, MailApiError> {
    let request_id = idempotency_key(&headers)?;
    let account = access::normalize_account(&account)
        .map_err(|_| MailApiError::invalid("invalid account format"))?;
    validate_mail(&input)?;
    let payload_hash = payload_hash(&serde_json::json!({
        "account": account,
        "title": input.title.trim(),
        "body": input.body.trim(),
        "attachments": input.attachments,
    }))?;
    let mut tx = state
        .db()
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(MailApiError::unavailable)?;
    if let Some(row) = request_row(&mut tx, &request_id).await? {
        ensure_replay(&row, "send_mail", &payload_hash)?;
        let result = serde_json::from_str(&row.result_json).map_err(MailApiError::unavailable)?;
        tx.rollback().await.map_err(MailApiError::unavailable)?;
        return Ok(Json(result));
    }
    let user_id = sqlx::query_scalar::<_, i64>(
        "SELECT u.id FROM users u
         JOIN account_allowlist a ON a.activated_user_id = u.id
         LEFT JOIN account_blacklist b ON b.account_key = a.account_key
         WHERE a.account_key = ? AND b.account_key IS NULL LIMIT 1",
    )
    .bind(&account)
    .fetch_optional(&mut *tx)
    .await
    .map_err(MailApiError::unavailable)?
    .ok_or_else(|| MailApiError::invalid("account is unavailable"))?;
    let attachment = input
        .attachments
        .iter()
        .map(|item| format!("{}#{}#{}", item.material_type, item.id, item.quantity))
        .collect::<Vec<_>>()
        .join("|");
    let insert = sqlx::query(
        "INSERT INTO user_mails
            (user_id, mail_id, params, attachment, state, create_time, sender, title, content, expire_time)
         VALUES (?, 920001, 'platform-admin', ?, 0, ?, '重返未来1999', ?, ?, 0)",
    )
    .bind(user_id)
    .bind(attachment)
    .bind(now_ms())
    .bind(input.title.trim())
    .bind(input.body.trim())
    .execute(&mut *tx)
    .await
    .map_err(MailApiError::unavailable)?;
    let result = MailOperation {
        request_id: request_id.clone(),
        state: "applied".to_string(),
        mail_incr_id: Some(insert.last_insert_rowid()),
        accounts: 1,
        mails: 1,
    };
    insert_applied_request(&mut tx, &request_id, "send_mail", &payload_hash, &result).await?;
    tx.commit().await.map_err(MailApiError::unavailable)?;
    Ok(Json(result))
}

pub(crate) async fn campaign_preview(
    State(state): State<ApiState>,
    Json(input): Json<CampaignInput>,
) -> Result<Json<CampaignPreview>, MailApiError> {
    let manifest = checked_campaign(&input.campaign_id)?;
    let user_ids = eligible_user_ids(state.db()).await?;
    let delivered: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_mail_campaign_deliveries d
         JOIN account_allowlist a ON a.activated_user_id = d.user_id
         LEFT JOIN account_blacklist b ON b.account_key = a.account_key
         WHERE d.campaign_id = ? AND b.account_key IS NULL",
    )
    .bind(&manifest.campaign_id)
    .fetch_one(state.db())
    .await
    .map_err(MailApiError::unavailable)?;
    let total = user_ids.len().saturating_mul(manifest.mails.len());
    Ok(Json(CampaignPreview {
        campaign_id: manifest.campaign_id.clone(),
        eligible_accounts: user_ids.len(),
        pending_mails: total.saturating_sub(delivered as usize),
    }))
}

pub(crate) async fn apply_campaign(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(input): Json<CampaignInput>,
) -> Result<Json<MailOperation>, MailApiError> {
    let request_id = idempotency_key(&headers)?;
    let manifest = checked_campaign(&input.campaign_id)?;
    let payload_hash = payload_hash(&input)?;
    let mut tx = state
        .db()
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(MailApiError::unavailable)?;
    if let Some(row) = request_row(&mut tx, &request_id).await? {
        ensure_replay(&row, "mail_campaign", &payload_hash)?;
        let result = serde_json::from_str(&row.result_json).map_err(MailApiError::unavailable)?;
        tx.rollback().await.map_err(MailApiError::unavailable)?;
        return Ok(Json(result));
    }
    let user_ids = eligible_user_ids_tx(&mut tx).await?;
    let mut mails = 0;
    for user_id in &user_ids {
        mails += mail_campaign::deliver_initial_campaign(&mut tx, *user_id, now_ms())
            .await
            .map_err(MailApiError::unavailable)?;
    }
    let result = MailOperation {
        request_id: request_id.clone(),
        state: "applied".to_string(),
        mail_incr_id: None,
        accounts: user_ids.len(),
        mails,
    };
    insert_applied_request(
        &mut tx,
        &request_id,
        "mail_campaign",
        &payload_hash,
        &result,
    )
    .await?;
    tx.commit().await.map_err(MailApiError::unavailable)?;
    debug_assert_eq!(manifest.campaign_id, input.campaign_id);
    Ok(Json(result))
}

fn validate_mail(input: &SendMailInput) -> Result<(), MailApiError> {
    if input.title.trim().is_empty() || input.title.chars().count() > MAX_TITLE_CHARS {
        return Err(MailApiError::invalid("invalid mail title"));
    }
    if input.body.trim().is_empty() || input.body.chars().count() > MAX_BODY_CHARS {
        return Err(MailApiError::invalid("invalid mail body"));
    }
    if !(1..=5).contains(&input.attachments.len()) {
        return Err(MailApiError::invalid(
            "mail must contain one to five attachments",
        ));
    }
    let catalog = build_initial_catalog(config::configs::get())
        .into_iter()
        .map(|entry| ((entry.material_type, entry.id), entry.quantity))
        .collect::<HashMap<_, _>>();
    let mut seen = std::collections::HashSet::new();
    for attachment in &input.attachments {
        let Some(maximum) = catalog.get(&(attachment.material_type, attachment.id)) else {
            return Err(MailApiError::invalid("unknown mail attachment"));
        };
        if attachment.quantity <= 0
            || attachment.quantity > *maximum
            || !seen.insert((attachment.material_type, attachment.id))
        {
            return Err(MailApiError::invalid("invalid mail attachment"));
        }
    }
    Ok(())
}

fn checked_campaign(id: &str) -> Result<&'static mail_campaign::InitialMailManifest, MailApiError> {
    let manifest = mail_campaign::initial_manifest().map_err(MailApiError::unavailable)?;
    if id != manifest.campaign_id {
        return Err(MailApiError::invalid("unknown mail campaign"));
    }
    Ok(manifest)
}

async fn eligible_user_ids(pool: &sqlx::SqlitePool) -> Result<Vec<i64>, MailApiError> {
    sqlx::query_scalar(
        "SELECT u.id FROM users u
         JOIN account_allowlist a ON a.activated_user_id = u.id
         LEFT JOIN account_blacklist b ON b.account_key = a.account_key
         WHERE b.account_key IS NULL ORDER BY u.id",
    )
    .fetch_all(pool)
    .await
    .map_err(MailApiError::unavailable)
}

async fn eligible_user_ids_tx(tx: &mut Transaction<'_, Sqlite>) -> Result<Vec<i64>, MailApiError> {
    sqlx::query_scalar(
        "SELECT u.id FROM users u
         JOIN account_allowlist a ON a.activated_user_id = u.id
         LEFT JOIN account_blacklist b ON b.account_key = a.account_key
         WHERE b.account_key IS NULL ORDER BY u.id",
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(MailApiError::unavailable)
}

fn idempotency_key(headers: &HeaderMap) -> Result<String, MailApiError> {
    headers
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
        .map(str::to_string)
        .ok_or_else(|| MailApiError::invalid("invalid idempotency key"))
}

fn payload_hash(value: &impl Serialize) -> Result<String, MailApiError> {
    let bytes = serde_json::to_vec(value).map_err(MailApiError::unavailable)?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

async fn request_row(
    tx: &mut Transaction<'_, Sqlite>,
    request_id: &str,
) -> Result<Option<LedgerRow>, MailApiError> {
    sqlx::query(
        "SELECT operation, payload_sha256, result_json FROM account_admin_requests WHERE request_id = ?",
    )
    .bind(request_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(MailApiError::unavailable)
    .map(|row| row.map(|row| LedgerRow {
        operation: row.get("operation"),
        payload_sha256: row.get("payload_sha256"),
        result_json: row.get("result_json"),
    }))
}

fn ensure_replay(row: &LedgerRow, operation: &str, hash: &str) -> Result<(), MailApiError> {
    if row.operation != operation || row.payload_sha256 != hash {
        return Err(MailApiError::conflict("idempotency key payload mismatch"));
    }
    Ok(())
}

async fn insert_applied_request(
    tx: &mut Transaction<'_, Sqlite>,
    request_id: &str,
    operation: &str,
    hash: &str,
    result: &MailOperation,
) -> Result<(), MailApiError> {
    let result_json = serde_json::to_string(result).map_err(MailApiError::unavailable)?;
    let now = now_ms();
    sqlx::query(
        "INSERT INTO account_admin_requests
            (request_id, operation, payload_sha256, status, result_json, created_at, updated_at)
         VALUES (?, ?, ?, 'applied', ?, ?, ?)",
    )
    .bind(request_id)
    .bind(operation)
    .bind(hash)
    .bind(result_json)
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await
    .map_err(MailApiError::unavailable)?;
    Ok(())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    fn init_config() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("data")
            .join("excel2json");
        let _ = config::init(path.to_str().unwrap());
    }

    #[test]
    fn mail_validation_accepts_catalog_entries_and_rejects_six_attachments() {
        init_config();
        let entries = build_initial_catalog(config::configs::get());
        let valid = entries
            .iter()
            .take(5)
            .map(|entry| MailAttachment {
                material_type: entry.material_type,
                id: entry.id,
                quantity: entry.quantity,
            })
            .collect::<Vec<_>>();
        assert!(
            validate_mail(&SendMailInput {
                title: "test".to_string(),
                body: "test".to_string(),
                attachments: valid.clone(),
            })
            .is_ok()
        );
        let mut invalid = valid;
        invalid.push(MailAttachment {
            material_type: 1,
            id: -1,
            quantity: 1,
        });
        assert_eq!(
            validate_mail(&SendMailInput {
                title: "test".to_string(),
                body: "test".to_string(),
                attachments: invalid,
            })
            .unwrap_err()
            .status,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[tokio::test]
    async fn campaign_preview_counts_only_eligible_accounts() {
        init_config();
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        database::run_migrations(&db).await.unwrap();
        sqlx::query("INSERT INTO users (id, username, email, created_at, updated_at) VALUES (1, 'one', 'one1', 0, 0), (2, 'two', 'two2', 0, 0);
                     INSERT INTO account_allowlist (account_key, display_account, batch_id, imported_at, imported_by, activated_user_id) VALUES ('one1','one1','t',0,'t',1), ('two2','two2','t',0,'t',2);
                     INSERT INTO account_blacklist (account_key, user_id, source, reason, created_at, created_by) VALUES ('two2',2,'manual_ban','t',0,'t');")
            .execute(&db).await.unwrap();
        let preview = campaign_preview(
            State(ApiState::new("token".into(), "127.0.0.1:9".into(), db)),
            Json(CampaignInput {
                campaign_id: "initial-full-v1".into(),
            }),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(preview.eligible_accounts, 1);
        assert_eq!(
            preview.pending_mails,
            mail_campaign::initial_manifest().unwrap().mails.len()
        );
    }

    #[tokio::test]
    async fn targeted_mail_replay_inserts_exactly_one_mail() {
        init_config();
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        database::run_migrations(&db).await.unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, email, created_at, updated_at)
             VALUES (1, 'one', 'one1', 0, 0);
             INSERT INTO account_allowlist
                (account_key, display_account, batch_id, imported_at, imported_by, activated_user_id)
             VALUES ('one1', 'one1', 't', 0, 't', 1);",
        )
        .execute(&db)
        .await
        .unwrap();
        let entry = build_initial_catalog(config::configs::get())
            .into_iter()
            .next()
            .unwrap();
        let input = || {
            Json(SendMailInput {
                title: "test".to_string(),
                body: "test".to_string(),
                attachments: vec![MailAttachment {
                    material_type: entry.material_type,
                    id: entry.id,
                    quantity: entry.quantity,
                }],
            })
        };
        let mut headers = HeaderMap::new();
        headers.insert("idempotency-key", "mail-1".parse().unwrap());
        let state = ApiState::new("token".into(), "127.0.0.1:9".into(), db.clone());
        let first = send_mail(
            State(state.clone()),
            Path("ONE1".to_string()),
            headers.clone(),
            input(),
        )
        .await
        .unwrap()
        .0;
        let replay = send_mail(State(state), Path("one1".to_string()), headers, input())
            .await
            .unwrap()
            .0;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_mails WHERE user_id = 1")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(first, replay);
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn campaign_replay_only_delivers_missing_ledger_rows_once() {
        init_config();
        let db = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        database::run_migrations(&db).await.unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, email, created_at, updated_at)
             VALUES (1, 'one', 'one1', 0, 0);
             INSERT INTO account_allowlist
                (account_key, display_account, batch_id, imported_at, imported_by, activated_user_id)
             VALUES ('one1', 'one1', 't', 0, 't', 1);",
        )
        .execute(&db)
        .await
        .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("idempotency-key", "campaign-1".parse().unwrap());
        let state = ApiState::new("token".into(), "127.0.0.1:9".into(), db.clone());
        let input = || {
            Json(CampaignInput {
                campaign_id: "initial-full-v1".into(),
            })
        };
        let first = apply_campaign(State(state.clone()), headers.clone(), input())
            .await
            .unwrap()
            .0;
        let replay = apply_campaign(State(state), headers, input())
            .await
            .unwrap()
            .0;
        let expected = mail_campaign::initial_manifest().unwrap().mails.len();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_mails WHERE user_id = 1")
            .fetch_one(&db)
            .await
            .unwrap();
        assert_eq!(first, replay);
        assert_eq!(first.mails, expected);
        assert_eq!(count as usize, expected);
    }
}
