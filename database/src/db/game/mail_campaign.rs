use crate::models::game::mail::localized_text;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::{Sqlite, SqlitePool, Transaction};
use std::{collections::HashSet, sync::OnceLock};

const INITIAL_MANIFEST: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../data/reverse1999/initial-mail-full-v1.json"
));

const RELEASE_ANNOUNCEMENTS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../data/reverse1999/release-announcements-v1.json"
));

#[derive(Debug, Clone, Deserialize)]
pub struct InitialMailManifest {
    pub campaign_id: String,
    pub source_data_sha: String,
    pub mails: Vec<ManifestMail>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ManifestMail {
    pub sequence: i32,
    pub category: String,
    pub title: String,
    pub body: String,
    pub attachment: String,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ManifestEntry {
    pub material_type: i32,
    pub id: i32,
    pub quantity: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseAnnouncementManifest {
    pub announcements: Vec<ReleaseAnnouncement>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseAnnouncement {
    pub campaign_id: String,
    pub title: String,
    pub body: String,
}

pub fn initial_manifest() -> anyhow::Result<&'static InitialMailManifest> {
    static MANIFEST: OnceLock<Result<InitialMailManifest, String>> = OnceLock::new();
    match MANIFEST.get_or_init(|| {
        let manifest: InitialMailManifest =
            serde_json::from_slice(INITIAL_MANIFEST).map_err(|error| error.to_string())?;
        validate_manifest(&manifest).map_err(|error| error.to_string())?;
        Ok(manifest)
    }) {
        Ok(manifest) => Ok(manifest),
        Err(error) => anyhow::bail!(error.clone()),
    }
}

pub fn initial_manifest_sha256() -> String {
    Sha256::digest(INITIAL_MANIFEST)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn release_announcement_manifest() -> anyhow::Result<&'static ReleaseAnnouncementManifest> {
    static MANIFEST: OnceLock<Result<ReleaseAnnouncementManifest, String>> = OnceLock::new();
    match MANIFEST.get_or_init(|| {
        let manifest: ReleaseAnnouncementManifest =
            serde_json::from_slice(RELEASE_ANNOUNCEMENTS).map_err(|error| error.to_string())?;
        validate_release_announcements(&manifest).map_err(|error| error.to_string())?;
        Ok(manifest)
    }) {
        Ok(manifest) => Ok(manifest),
        Err(error) => anyhow::bail!(error.clone()),
    }
}

fn release_announcement_manifest_sha256() -> String {
    Sha256::digest(RELEASE_ANNOUNCEMENTS)
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
    for mail in &manifest.mails {
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
             VALUES (?, 0, ?, ?, 0, ?, ?, ?, ?, 0)",
        )
        .bind(user_id)
        .bind(format!("{}:{}", manifest.campaign_id, mail.category))
        .bind(&mail.attachment)
        .bind(now)
        .bind(localized_text("重返未来1999"))
        .bind(localized_text(&mail.title))
        .bind(localized_text(&mail.body))
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

pub async fn deliver_all_campaigns(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    now: i64,
) -> anyhow::Result<usize> {
    let initial = deliver_initial_campaign(tx, user_id, now).await?;
    let announcements = deliver_release_announcements(tx, user_id, now).await?;
    Ok(initial + announcements)
}

pub async fn deliver_release_announcements(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    now: i64,
) -> anyhow::Result<usize> {
    let manifest = release_announcement_manifest()?;
    let manifest_sha256 = release_announcement_manifest_sha256();
    let mut delivered = 0;
    for (index, announcement) in manifest.announcements.iter().enumerate() {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM user_mail_campaign_deliveries
             WHERE campaign_id = ? AND user_id = ? AND sequence = 1 LIMIT 1",
        )
        .bind(&announcement.campaign_id)
        .bind(user_id)
        .fetch_optional(&mut **tx)
        .await?
        .is_some();
        if exists {
            continue;
        }

        let create_time = now.saturating_add(index as i64 + 1);
        let result = sqlx::query(
            "INSERT INTO user_mails
                (user_id, mail_id, params, attachment, state, create_time, sender, title, content,
                 expire_time)
             VALUES (?, 0, ?, '', 0, ?, ?, ?, ?, 0)",
        )
        .bind(user_id)
        .bind(format!("{}:announcement", announcement.campaign_id))
        .bind(create_time)
        .bind(localized_text("重返未来1999"))
        .bind(localized_text(&announcement.title))
        .bind(localized_text(&announcement.body))
        .execute(&mut **tx)
        .await?;
        let mail_incr_id = result.last_insert_rowid();

        sqlx::query(
            "INSERT INTO user_mail_campaign_deliveries
                (campaign_id, user_id, sequence, mail_incr_id, manifest_sha256, delivered_at)
             VALUES (?, ?, 1, ?, ?, ?)",
        )
        .bind(&announcement.campaign_id)
        .bind(user_id)
        .bind(mail_incr_id)
        .bind(&manifest_sha256)
        .bind(now)
        .execute(&mut **tx)
        .await?;
        delivered += 1;
    }
    Ok(delivered)
}

pub async fn reconcile_release_announcements(
    pool: &SqlitePool,
    now: i64,
) -> anyhow::Result<(usize, usize)> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let user_ids = sqlx::query_scalar::<_, i64>("SELECT id FROM users ORDER BY id")
        .fetch_all(&mut *tx)
        .await?;
    let users = user_ids.len();
    let mut delivered = 0;
    for user_id in user_ids {
        delivered += deliver_release_announcements(&mut tx, user_id, now).await?;
    }
    tx.commit().await?;
    Ok((users, delivered))
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

fn validate_release_announcements(manifest: &ReleaseAnnouncementManifest) -> anyhow::Result<()> {
    anyhow::ensure!(
        !manifest.announcements.is_empty(),
        "release announcement manifest is empty"
    );
    let mut campaign_ids = HashSet::new();
    for announcement in &manifest.announcements {
        anyhow::ensure!(
            !announcement.campaign_id.trim().is_empty(),
            "release announcement campaign id is empty"
        );
        anyhow::ensure!(
            campaign_ids.insert(&announcement.campaign_id),
            "duplicate release announcement campaign id"
        );
        anyhow::ensure!(
            !announcement.title.trim().is_empty(),
            "release announcement title is empty"
        );
        anyhow::ensure!(
            !announcement.body.trim().is_empty(),
            "release announcement body is empty"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use sqlx::sqlite::SqlitePoolOptions;

    const CLIENT_LANGUAGES: [&str; 8] = ["zh", "tw", "en", "kr", "jp", "de", "fr", "thai"];

    fn assert_localized_text(encoded: &str, expected: &str) {
        let value: Value = serde_json::from_str(encoded).unwrap();
        for language in CLIENT_LANGUAGES {
            assert_eq!(value[language], expected);
        }
    }

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

    #[test]
    fn embedded_release_announcements_are_valid_and_unique() {
        let manifest = release_announcement_manifest().unwrap();
        assert!(!manifest.announcements.is_empty());
        assert_eq!(
            manifest
                .announcements
                .iter()
                .map(|announcement| announcement.campaign_id.as_str())
                .collect::<HashSet<_>>()
                .len(),
            manifest.announcements.len()
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

        let first = initial_manifest().unwrap().mails.first().unwrap();
        let (mail_id, sender, title, content, state): (i32, String, String, String, i32) =
            sqlx::query_as(
                "SELECT mail_id, sender, title, content, state
                 FROM user_mails WHERE user_id = 7 ORDER BY incr_id LIMIT 1",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(mail_id, 0);
        assert_eq!(state, 0);
        assert_localized_text(&sender, "重返未来1999");
        assert_localized_text(&title, &first.title);
        assert_localized_text(&content, &first.body);
    }

    #[tokio::test]
    async fn release_announcements_reconcile_existing_users_once_and_sort_above_initial_mail() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::run_migrations(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, created_at, updated_at)
             VALUES (7, 'existing', 0, 0), (8, 'future', 0, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        deliver_all_campaigns(&mut tx, 7, 100).await.unwrap();
        tx.commit().await.unwrap();
        let announcement_count = release_announcement_manifest().unwrap().announcements.len();

        assert_eq!(
            reconcile_release_announcements(&pool, 200).await.unwrap(),
            (2, announcement_count)
        );
        assert_eq!(
            reconcile_release_announcements(&pool, 300).await.unwrap(),
            (2, 0)
        );

        let top_params: String = sqlx::query_scalar(
            "SELECT params FROM user_mails WHERE user_id = 7
             ORDER BY create_time DESC, incr_id DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(top_params.ends_with(":announcement"));
        let user_seven_announcements: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_mail_campaign_deliveries
             WHERE user_id = 7 AND campaign_id != 'initial-full-v1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(user_seven_announcements as usize, announcement_count);
    }

    #[tokio::test]
    async fn legacy_campaign_mail_repair_preserves_claimed_state() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::run_migrations(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO users (id, username, created_at, updated_at)
             VALUES (8, 'legacy-mail', 0, 0);
             INSERT INTO user_mails
                (incr_id, user_id, mail_id, params, attachment, state, create_time,
                 sender, title, content, expire_time)
             VALUES (81, 8, 910001, 'initial-full-v1:货币', '2#11#25', 1, 10,
                     '重返未来1999', '货币-1', '公益服', 0);
             INSERT INTO user_mail_campaign_deliveries
                (campaign_id, user_id, sequence, mail_incr_id, manifest_sha256, delivered_at)
             VALUES ('initial-full-v1', 8, 1, 81, 'legacy', 10);",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::raw_sql(include_str!(
            "../../../migrations/091_dynamic_custom_mails.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();

        let (mail_id, sender, title, content, state): (i32, String, String, String, i32) =
            sqlx::query_as(
                "SELECT mail_id, sender, title, content, state
                 FROM user_mails WHERE incr_id = 81",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(mail_id, 0);
        assert_eq!(state, 1);
        assert_localized_text(&sender, "重返未来1999");
        assert_localized_text(&title, "货币-1");
        assert_localized_text(&content, "公益服");
    }
}
