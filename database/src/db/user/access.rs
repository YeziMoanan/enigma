use sqlx::{Sqlite, Transaction};
use std::{error::Error, fmt};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RegistrationDecision {
    Allowed,
}

#[derive(Debug)]
pub enum AccountAccessError {
    InvalidFormat,
    Blacklisted,
    NotAllowlisted,
    Database(sqlx::Error),
}

impl fmt::Display for AccountAccessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat => formatter.write_str("invalid account format"),
            Self::Blacklisted => formatter.write_str("account is blacklisted"),
            Self::NotAllowlisted => formatter.write_str("account is not allowlisted"),
            Self::Database(error) => error.fmt(formatter),
        }
    }
}

impl Error for AccountAccessError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for AccountAccessError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

pub fn normalize_account(value: &str) -> Result<String, AccountAccessError> {
    let value = value.trim();
    if !(4..=64).contains(&value.len())
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'@' | b'.' | b'_' | b'+' | b'-')
        })
    {
        return Err(AccountAccessError::InvalidFormat);
    }

    Ok(value.to_ascii_lowercase())
}

pub async fn registration_decision(
    tx: &mut Transaction<'_, Sqlite>,
    account_key: &str,
    now: i64,
) -> Result<RegistrationDecision, AccountAccessError> {
    let blacklisted = sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM account_blacklist WHERE account_key = ? LIMIT 1",
    )
    .bind(account_key)
    .fetch_optional(&mut **tx)
    .await?
    .is_some();
    if blacklisted {
        return Err(AccountAccessError::Blacklisted);
    }

    let allowlisted = sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM account_allowlist WHERE account_key = ? LIMIT 1",
    )
    .bind(account_key)
    .fetch_optional(&mut **tx)
    .await?
    .is_some();
    if allowlisted {
        return Ok(RegistrationDecision::Allowed);
    }

    sqlx::query(
        "INSERT OR IGNORE INTO account_blacklist
            (account_key, user_id, source, reason, created_at, created_by)
         VALUES (?, NULL, 'unauthorized_registration', 'registration attempted without allowlist entry', ?, 'system')",
    )
    .bind(account_key)
    .bind(now)
    .execute(&mut **tx)
    .await?;

    Err(AccountAccessError::NotAllowlisted)
}

#[cfg(test)]
mod tests {
    use super::{
        AccountAccessError, RegistrationDecision, normalize_account, registration_decision,
    };
    use sqlx::sqlite::SqlitePoolOptions;

    #[test]
    fn normalizes_supported_account_formats() {
        assert_eq!(normalize_account("player01").unwrap(), "player01");
        assert_eq!(normalize_account("123456").unwrap(), "123456");
        assert_eq!(
            normalize_account("Player@Example.COM").unwrap(),
            "player@example.com"
        );
        assert_eq!(normalize_account("  Player01  ").unwrap(), "player01");
    }

    #[test]
    fn rejects_whitespace_unicode_and_invalid_lengths() {
        for invalid in [
            "pla yer01",
            "player\t01",
            "玩家01",
            "abc",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert!(matches!(
                normalize_account(invalid),
                Err(AccountAccessError::InvalidFormat)
            ));
        }
    }

    async fn migrated_pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::run_migrations(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn allowlist_account_key_is_unique() {
        let pool = migrated_pool().await;
        sqlx::query(
            "INSERT INTO account_allowlist
                (account_key, display_account, batch_id, imported_at, imported_by)
             VALUES ('player01', 'Player01', 'batch-1', 10, 'admin')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let duplicate = sqlx::query(
            "INSERT INTO account_allowlist
                (account_key, display_account, batch_id, imported_at, imported_by)
             VALUES ('player01', 'PLAYER01', 'batch-2', 20, 'admin')",
        )
        .execute(&pool)
        .await;

        assert!(duplicate.is_err());
    }

    #[tokio::test]
    async fn blacklist_takes_precedence_over_allowlist() {
        let pool = migrated_pool().await;
        sqlx::query(
            "INSERT INTO account_allowlist
                (account_key, display_account, batch_id, imported_at, imported_by)
             VALUES ('player01', 'Player01', 'batch-1', 10, 'admin')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO account_blacklist
                (account_key, source, reason, created_at, created_by)
             VALUES ('player01', 'manual_ban', 'policy violation', 20, 'admin')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        let result = registration_decision(&mut tx, "player01", 30).await;

        assert!(matches!(result, Err(AccountAccessError::Blacklisted)));
    }

    #[tokio::test]
    async fn allowlisted_account_is_allowed() {
        let pool = migrated_pool().await;
        sqlx::query(
            "INSERT INTO account_allowlist
                (account_key, display_account, batch_id, imported_at, imported_by)
             VALUES ('player01', 'Player01', 'batch-1', 10, 'admin')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut tx = pool.begin().await.unwrap();
        let result = registration_decision(&mut tx, "player01", 30)
            .await
            .unwrap();

        assert_eq!(result, RegistrationDecision::Allowed);
    }

    #[tokio::test]
    async fn missing_allowlist_entry_is_recorded_and_rejected() {
        let pool = migrated_pool().await;
        let mut tx = pool.begin().await.unwrap();

        let result = registration_decision(&mut tx, "player01", 30).await;
        assert!(matches!(result, Err(AccountAccessError::NotAllowlisted)));

        let source: String = sqlx::query_scalar(
            "SELECT source FROM account_blacklist WHERE account_key = 'player01'",
        )
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(source, "unauthorized_registration");
    }
}
