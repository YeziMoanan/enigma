use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::{Sqlite, Transaction};
use std::collections::HashSet;

const INITIAL_MANIFEST: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../data/reverse1999/initial-mail-full-v1.json"
));

#[derive(Debug, Deserialize)]
pub struct InitialMailManifest {
    pub campaign_id: String,
    pub source_data_sha: String,
    pub mails: Vec<ManifestMail>,
}

#[derive(Debug, Deserialize)]
pub struct ManifestMail {
    pub sequence: i32,
    pub category: String,
    pub title: String,
    pub body: String,
    pub attachment: String,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Debug, Deserialize)]
pub struct ManifestEntry {
    pub material_type: i32,
    pub id: i32,
    pub quantity: i32,
}

pub fn initial_manifest() -> anyhow::Result<InitialMailManifest> {
    let manifest: InitialMailManifest = serde_json::from_slice(INITIAL_MANIFEST)?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

pub fn initial_manifest_sha256() -> String {
    Sha256::digest(INITIAL_MANIFEST)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub async fn deliver_initial_campaign(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    now: i64,
) -> anyhow::Result<usize> {
    let manifest = initial_manifest()?;
    let manifest_sha256 = initial_manifest_sha256();
    let mut delivered = 0;
    for mail in manifest.mails {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM user_mail_campaign_deliveries
             WHERE campaign_id = ? AND user_id = ? AND sequence = ? LIMIT 1",
        )
        .bind(&manifest.campaign_id)
        .bind(user_id)
        .bind(mail.sequence)
        .fetch_optional(&mut **tx)
        .await?
        .is_some();
        if exists {
            continue;
        }

        let result = sqlx::query(
            "INSERT INTO user_mails
                (user_id, mail_id, params, attachment, state, create_time, sender, title, content,
                 expire_time)
             VALUES (?, ?, ?, ?, 0, ?, '重返未来1999', ?, ?, 0)",
        )
        .bind(user_id)
        .bind(910_000 + mail.sequence)
        .bind(format!("{}:{}", manifest.campaign_id, mail.category))
        .bind(&mail.attachment)
        .bind(now)
        .bind(&mail.title)
        .bind(&mail.body)
        .execute(&mut **tx)
        .await?;
        let mail_incr_id = result.last_insert_rowid();

        sqlx::query(
            "INSERT INTO user_mail_campaign_deliveries
                (campaign_id, user_id, sequence, mail_incr_id, manifest_sha256, delivered_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&manifest.campaign_id)
        .bind(user_id)
        .bind(mail.sequence)
        .bind(mail_incr_id)
        .bind(&manifest_sha256)
        .bind(now)
        .execute(&mut **tx)
        .await?;
        delivered += 1;
    }
    Ok(delivered)
}

fn validate_manifest(manifest: &InitialMailManifest) -> anyhow::Result<()> {
    anyhow::ensure!(
        !manifest.campaign_id.trim().is_empty(),
        "campaign id is empty"
    );
    anyhow::ensure!(
        !manifest.source_data_sha.trim().is_empty(),
        "source data SHA is empty"
    );
    anyhow::ensure!(!manifest.mails.is_empty(), "mail manifest is empty");
    let mut sequences = HashSet::new();
    for mail in &manifest.mails {
        anyhow::ensure!(mail.sequence > 0, "invalid mail sequence");
        anyhow::ensure!(sequences.insert(mail.sequence), "duplicate mail sequence");
        anyhow::ensure!(
            (1..=5).contains(&mail.entries.len()),
            "mail attachment count is invalid"
        );
        anyhow::ensure!(!mail.title.trim().is_empty(), "mail title is empty");
        anyhow::ensure!(!mail.body.trim().is_empty(), "mail body is empty");
        let encoded = mail
            .entries
            .iter()
            .map(|entry| {
                anyhow::ensure!(
                    entry.material_type > 0 && entry.id > 0 && entry.quantity > 0,
                    "invalid manifest entry"
                );
                Ok(format!(
                    "{}#{}#{}",
                    entry.material_type, entry.id, entry.quantity
                ))
            })
            .collect::<anyhow::Result<Vec<_>>>()?
            .join("|");
        anyhow::ensure!(mail.attachment == encoded, "attachment encoding mismatch");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    #[test]
    fn embedded_manifest_is_valid_and_globally_sequenced() {
        let manifest = initial_manifest().unwrap();
        assert_eq!(manifest.campaign_id, "initial-full-v1");
        assert_eq!(
            manifest
                .mails
                .iter()
                .map(|mail| mail.sequence)
                .collect::<Vec<_>>(),
            (1..=manifest.mails.len() as i32).collect::<Vec<_>>()
        );
        assert!(
            manifest
                .mails
                .iter()
                .all(|mail| (1..=5).contains(&mail.entries.len()))
        );
    }

    #[tokio::test]
    async fn campaign_delivery_is_idempotent() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::run_migrations(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, created_at, updated_at) VALUES (7, 'mail', 0, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let expected = initial_manifest().unwrap().mails.len();
        for now in [10_i64, 20_i64] {
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
            deliver_initial_campaign(&mut tx, 7, now).await.unwrap();
            tx.commit().await.unwrap();
        }
        let mail_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM user_mails WHERE user_id = 7")
                .fetch_one(&pool)
                .await
                .unwrap();
        let ledger_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_mail_campaign_deliveries WHERE user_id = 7",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(mail_count as usize, expected);
        assert_eq!(ledger_count as usize, expected);
    }
}
