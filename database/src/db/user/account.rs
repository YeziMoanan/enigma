use crate::db::{starter_data, user::access};
use anyhow::Result;
use bcrypt::{DEFAULT_COST, hash, verify};
use sqlx::{Row, Sqlite, SqlitePool, Transaction, prelude::FromRow};

#[derive(Debug, Clone, Copy)]
pub struct InitialMailDefinition {
    pub category: &'static str,
    pub mail_id: i32,
    pub title: &'static str,
    pub content: &'static str,
    pub attachment: &'static str,
    pub expire_time: i64,
}

const INITIAL_MAILS: [InitialMailDefinition; 1] = [InitialMailDefinition {
    category: "welcome",
    mail_id: 900001,
    title: "Welcome to Reverse: 1999",
    content: "Your journey begins here.",
    attachment: "",
    expire_time: 0,
}];

pub fn initial_mail_definitions() -> &'static [InitialMailDefinition] {
    &INITIAL_MAILS
}

pub fn normalize_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

#[derive(Debug, Clone)]
pub struct UserAccount {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub account_type: i32,
    pub registration_account_type: i32,
    pub vip_level: i32,
    pub first_join: bool,
    pub need_real_name: bool,
    pub real_name_status: bool,
    pub age: i32,
    pub is_adult: bool,
    pub need_activate: bool,
    pub cipher_mark: bool,
    pub account_tags: String,
}

#[derive(Debug, Clone)]
pub struct TokenInfo {
    pub token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}

/// Get user account by email
pub async fn get_user_by_email(pool: &SqlitePool, email: &str) -> Result<Option<UserAccount>> {
    let email = normalize_email(email);
    let row = sqlx::query(
        "SELECT id, username, email, account_type, registration_account_type, vip_level,
                first_join, need_real_name, real_name_status,
                age, is_adult, need_activate, cipher_mark, account_tags
         FROM users WHERE LOWER(TRIM(email)) = ?1",
    )
    .bind(&email)
    .fetch_optional(pool)
    .await?;

    match row {
        Some(r) => Ok(Some(UserAccount {
            id: r.try_get("id")?,
            username: r.try_get("username")?,
            email: r.try_get("email")?,
            account_type: r.try_get::<i64, _>("account_type")? as i32,
            registration_account_type: r.try_get::<i64, _>("registration_account_type")? as i32,
            vip_level: r.try_get::<i64, _>("vip_level")? as i32,
            first_join: r.try_get::<i64, _>("first_join")? != 0,
            need_real_name: r.try_get::<i64, _>("need_real_name")? != 0,
            real_name_status: r.try_get::<i64, _>("real_name_status")? != 0,
            age: r.try_get::<Option<i64>, _>("age")?.unwrap_or(18) as i32,
            is_adult: r.try_get::<i64, _>("is_adult")? != 0,
            need_activate: r.try_get::<i64, _>("need_activate")? != 0,
            cipher_mark: r.try_get::<i64, _>("cipher_mark")? != 0,
            account_tags: r
                .try_get::<Option<String>, _>("account_tags")?
                .unwrap_or_default(),
        })),
        None => Ok(None),
    }
}

/// Get user account by ID
pub async fn get_user_by_id(pool: &SqlitePool, user_id: i64) -> Result<Option<UserAccount>> {
    let row = sqlx::query(
        "SELECT id, username, email, account_type, registration_account_type, vip_level,
                first_join, need_real_name, real_name_status,
                age, is_adult, need_activate, cipher_mark, account_tags
         FROM users WHERE id = ?1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    match row {
        Some(r) => Ok(Some(UserAccount {
            id: r.try_get("id")?,
            username: r.try_get("username")?,
            email: r.try_get("email")?,
            account_type: r.try_get::<i64, _>("account_type")? as i32,
            registration_account_type: r.try_get::<i64, _>("registration_account_type")? as i32,
            vip_level: r.try_get::<i64, _>("vip_level")? as i32,
            first_join: r.try_get::<i64, _>("first_join")? != 0,
            need_real_name: r.try_get::<i64, _>("need_real_name")? != 0,
            real_name_status: r.try_get::<i64, _>("real_name_status")? != 0,
            age: r.try_get::<Option<i64>, _>("age")?.unwrap_or(18) as i32,
            is_adult: r.try_get::<i64, _>("is_adult")? != 0,
            need_activate: r.try_get::<i64, _>("need_activate")? != 0,
            cipher_mark: r.try_get::<i64, _>("cipher_mark")? != 0,
            account_tags: r
                .try_get::<Option<String>, _>("account_tags")?
                .unwrap_or_default(),
        })),
        None => Ok(None),
    }
}

/// Verify user password
pub async fn verify_user_password(pool: &SqlitePool, email: &str, password: &str) -> Result<bool> {
    let email = normalize_email(email);
    let row = sqlx::query("SELECT password_hash FROM users WHERE LOWER(TRIM(email)) = ?1")
        .bind(&email)
        .fetch_optional(pool)
        .await?;

    match row {
        Some(r) => {
            let password_hash: String = r.try_get("password_hash")?;
            Ok(verify(password, &password_hash)?)
        }
        None => Ok(false),
    }
}

/// Create a new user account
pub async fn create_user(
    pool: &SqlitePool,
    email: &str,
    password: &str,
    token_info: &TokenInfo,
    now: i64,
) -> Result<UserAccount> {
    let email = access::normalize_account(email).map_err(anyhow::Error::new)?;
    validate_password(password)?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;

    if let Err(access_error) = access::registration_decision(&mut tx, &email, now).await {
        if matches!(access_error, access::AccountAccessError::NotAllowlisted) {
            if let Err(commit_error) = tx.commit().await {
                return Err(anyhow::Error::new(access::AccountAccessError::Database(
                    commit_error,
                )));
            }
        }
        return Err(anyhow::Error::new(access_error));
    }

    let password_hash = hash(password, DEFAULT_COST)?;

    let insert = sqlx::query(
        "INSERT INTO users (
            username, email, password_hash, account_type, registration_account_type,
            token, refresh_token, token_expires_at,
            vip_level, level, exp,
            need_real_name, real_name_status, age, is_adult,
            need_activate, cipher_mark, first_join, account_tags,
            created_at, updated_at, last_login_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)"
    )
    .bind(&email)
    .bind(&email)
    .bind(&password_hash)
    .bind(10) // AccountType::Email
    .bind(1)
    .bind(&token_info.token)
    .bind(&token_info.refresh_token)
    .bind(token_info.expires_at)
    .bind(0) // vip_level
    .bind(1) // level
    .bind(0) // exp
    .bind(false) // need_real_name
    .bind(true)  // real_name_status
    .bind(18)    // age
    .bind(true)  // is_adult
    .bind(false) // need_activate
    .bind(true)  // cipher_mark
    .bind(false) // first_join
    .bind("") // account_tags
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    let user_id = insert.last_insert_rowid();
    let username = format!("player_{user_id}");
    sqlx::query("UPDATE users SET username = ? WHERE id = ?")
        .bind(&username)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let activation = sqlx::query(
        "UPDATE account_allowlist SET activated_user_id = ?
         WHERE account_key = ? AND activated_user_id IS NULL",
    )
    .bind(user_id)
    .bind(&email)
    .execute(&mut *tx)
    .await?;
    if activation.rows_affected() != 1 {
        return Err(anyhow::Error::new(
            access::AccountAccessError::ActivationConflict,
        ));
    }
    starter_data::load_all_starter_data_tx(&mut tx, user_id).await?;
    insert_initial_mails(&mut tx, user_id, now).await?;
    tx.commit().await?;

    Ok(UserAccount {
        id: user_id,
        username,
        email,
        account_type: 10,
        registration_account_type: 1,
        vip_level: 0,
        first_join: false,
        need_real_name: false,
        real_name_status: true,
        age: 18,
        is_adult: true,
        need_activate: false,
        cipher_mark: true,
        account_tags: String::new(),
    })
}

fn validate_password(password: &str) -> Result<(), access::AccountAccessError> {
    if !(6..=64).contains(&password.len()) {
        return Err(access::AccountAccessError::InvalidPassword);
    }
    Ok(())
}

/// Update user tokens and last login time
pub async fn update_user_login(
    pool: &SqlitePool,
    user_id: i64,
    token_info: &TokenInfo,
    now: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE users SET
            token = ?1,
            refresh_token = ?2,
            token_expires_at = ?3,
            last_login_at = ?4,
            updated_at = ?5
         WHERE id = ?6",
    )
    .bind(&token_info.token)
    .bind(&token_info.refresh_token)
    .bind(token_info.expires_at)
    .bind(now)
    .bind(now)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Handle user login with password verification - creates or updates user
pub async fn handle_user_login(
    pool: &SqlitePool,
    email: &str,
    password: &str,
    token_info: TokenInfo,
    now: i64,
) -> Result<UserAccount> {
    let email = access::normalize_account(email).map_err(anyhow::Error::new)?;
    match get_user_by_email(pool, &email).await? {
        Some(user) => {
            access::require_login_access(pool, user.id, &email)
                .await
                .map_err(anyhow::Error::new)?;
            validate_password(password).map_err(anyhow::Error::new)?;
            if !verify_user_password(pool, &email, password).await? {
                return Err(anyhow::Error::new(
                    access::AccountAccessError::InvalidPassword,
                ));
            }

            // Update tokens
            update_user_login(pool, user.id, &token_info, now).await?;
            Ok(user)
        }
        None => match create_user(pool, &email, password, &token_info, now).await {
            Ok(user) => Ok(user),
            Err(create_error) => {
                let Some(user) = get_user_by_email(pool, &email).await? else {
                    return Err(create_error);
                };
                access::require_login_access(pool, user.id, &email)
                    .await
                    .map_err(anyhow::Error::new)?;
                validate_password(password).map_err(anyhow::Error::new)?;
                if !verify_user_password(pool, &email, password).await? {
                    return Err(anyhow::Error::new(
                        access::AccountAccessError::InvalidPassword,
                    ));
                }
                update_user_login(pool, user.id, &token_info, now).await?;
                Ok(user)
            }
        },
    }
}

async fn insert_initial_mails(
    tx: &mut Transaction<'_, Sqlite>,
    user_id: i64,
    now: i64,
) -> sqlx::Result<()> {
    for definition in initial_mail_definitions() {
        let marker = sqlx::query(
            "INSERT OR IGNORE INTO user_initial_mail_deliveries (user_id, category, delivered_at)
             VALUES (?, ?, ?)",
        )
        .bind(user_id)
        .bind(definition.category)
        .bind(now)
        .execute(&mut **tx)
        .await?;

        if marker.rows_affected() == 0 {
            continue;
        }

        sqlx::query(
            "INSERT INTO user_mails
                (user_id, mail_id, params, attachment, state, create_time, sender, title, content, expire_time)
             VALUES (?, ?, ?, ?, 0, ?, ?, ?, ?, ?)",
        )
        .bind(user_id)
        .bind(definition.mail_id)
        .bind(definition.category)
        .bind(definition.attachment)
        .bind(now)
        .bind("Reverse: 1999")
        .bind(definition.title)
        .bind(definition.content)
        .bind(definition.expire_time)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

#[derive(FromRow)]
pub struct UserToken {
    pub token: String,
}

pub async fn get_user_token(pool: &SqlitePool, user_id: i64) -> Result<UserToken> {
    let token = sqlx::query_as::<_, UserToken>(
        "SELECT token
         FROM users
         WHERE id = ?1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::Error::new(access::AccountAccessError::UserNotFound))?;

    Ok(token)
}

#[derive(Debug, Clone, FromRow)]
pub struct LoginToken {
    pub token: String,
    pub token_expires_at: Option<i64>,
}

pub async fn get_login_token(pool: &SqlitePool, user_id: i64) -> Result<Option<LoginToken>> {
    Ok(
        sqlx::query_as::<_, LoginToken>("SELECT token, token_expires_at FROM users WHERE id = ?1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn rename_user_and_update_guide(
    pool: &SqlitePool,
    user_id: i64,
    username: String,
    guide_id: i32,
    step_id: i32,
) -> Result<()> {
    let mut tx: Transaction<'_, Sqlite> = pool.begin().await?;

    // Update username
    sqlx::query(
        r#"
        UPDATE users
        SET username = ?
        WHERE id = ?
        "#,
    )
    .bind(username)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    // Upsert guide progress
    sqlx::query(
        r#"
        INSERT INTO guide_progress (user_id, guide_id, step_id)
        VALUES (?, ?, ?)
        ON CONFLICT(user_id, guide_id)
        DO UPDATE SET step_id = excluded.step_id
        "#,
    )
    .bind(user_id)
    .bind(guide_id)
    .bind(step_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

pub async fn update_user_level(pool: &SqlitePool, user_id: i64, level: i32) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE users
        SET level = ?
        WHERE id = ?
        "#,
    )
    .bind(level)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{TokenInfo, handle_user_login, initial_mail_definitions, normalize_email};
    use crate::db::user::access::AccountAccessError;
    use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
    use std::{collections::HashMap, path::PathBuf, time::Duration};

    async fn account_test_pool(label: &str) -> (SqlitePool, PathBuf) {
        let data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("sonetto-data/excel2json");
        let _ = config::init(data_dir.to_str().unwrap());
        let database_path = std::env::temp_dir().join(format!(
            "reverse1999-{label}-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .busy_timeout(Duration::from_secs(10));
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();
        crate::run_migrations(&pool).await.unwrap();
        (pool, database_path)
    }

    async fn allow_account(pool: &SqlitePool, account_key: &str) {
        sqlx::query(
            "INSERT INTO account_allowlist
                (account_key, display_account, batch_id, imported_at, imported_by)
             VALUES (?, ?, 'test', 1, 'test')",
        )
        .bind(account_key)
        .bind(account_key)
        .execute(pool)
        .await
        .unwrap();
    }

    fn token() -> TokenInfo {
        TokenInfo {
            token: "token".to_string(),
            refresh_token: "refresh".to_string(),
            expires_at: 100,
        }
    }

    fn legacy_user_id(email: &str) -> i64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        email.to_lowercase().hash(&mut hasher);
        1_000_000 + (hasher.finish() % 9_000_000) as i64
    }

    fn legacy_collision() -> (String, String) {
        let mut seen = HashMap::new();
        for index in 0..100_000 {
            let email = format!("collision-{index}@example.com");
            let id = legacy_user_id(&email);
            if let Some(first) = seen.insert(id, email.clone()) {
                return (first, email);
            }
        }
        panic!("expected to find a collision in the legacy 9,000,000-ID space");
    }

    #[test]
    fn equivalent_email_format_has_one_identity() {
        assert_eq!(
            normalize_email("  Player@Example.COM "),
            "player@example.com"
        );
    }

    #[test]
    fn initial_mail_is_explicitly_categorized_without_resource_injection() {
        let definitions = initial_mail_definitions();
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].category, "welcome");
        assert!(definitions[0].attachment.is_empty());
        assert!(!definitions[0].title.is_empty());
        assert!(!definitions[0].content.is_empty());
    }

    #[tokio::test]
    async fn initial_mail_delivery_is_idempotent() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE user_mails (
                incr_id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL,
                mail_id INTEGER NOT NULL,
                params TEXT NOT NULL DEFAULT '',
                attachment TEXT NOT NULL DEFAULT '',
                state INTEGER NOT NULL DEFAULT 0,
                create_time INTEGER NOT NULL,
                sender TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL DEFAULT '',
                content TEXT NOT NULL DEFAULT '',
                expire_time INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE user_initial_mail_deliveries (
                user_id INTEGER NOT NULL,
                category TEXT NOT NULL,
                delivered_at INTEGER NOT NULL,
                PRIMARY KEY (user_id, category)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        for now in [10_i64, 20_i64] {
            let mut tx = pool.begin().await.unwrap();
            super::insert_initial_mails(&mut tx, 7, now).await.unwrap();
            tx.commit().await.unwrap();
        }

        let mail_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_mails")
            .fetch_one(&pool)
            .await
            .unwrap();
        let delivery_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM user_initial_mail_deliveries")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(mail_count, 1);
        assert_eq!(delivery_count, 1);
    }

    #[tokio::test]
    async fn repeated_and_concurrent_login_create_one_account_and_one_initial_mail() {
        let data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("sonetto-data/excel2json");
        config::init(data_dir.to_str().unwrap()).unwrap();
        let database_path = std::env::temp_dir().join(format!(
            "reverse1999-login-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let options = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&database_path)
            .create_if_missing(true)
            .busy_timeout(Duration::from_secs(10));
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();
        crate::run_migrations(&pool).await.unwrap();

        allow_account(&pool, "concurrent@example.com").await;

        let token = TokenInfo {
            token: "token".to_string(),
            refresh_token: "refresh".to_string(),
            expires_at: 100,
        };
        let first = handle_user_login(
            &pool,
            " Concurrent@Example.COM ",
            "password",
            token.clone(),
            10,
        );
        let second = handle_user_login(
            &pool,
            "concurrent@example.com",
            "password",
            token.clone(),
            20,
        );
        let (first, second) = tokio::join!(first, second);
        let first = first.unwrap();
        let second = second.unwrap();
        assert_eq!(first.id, second.id);

        handle_user_login(&pool, "CONCURRENT@example.com", "password", token, 30)
            .await
            .unwrap();
        let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();
        let mail_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_mails")
            .fetch_one(&pool)
            .await
            .unwrap();
        let delivery_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM user_initial_mail_deliveries")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!((user_count, mail_count, delivery_count), (1, 1, 1));

        pool.close().await;
        let _ = std::fs::remove_file(database_path);
    }

    #[tokio::test]
    async fn different_emails_with_legacy_hash_collision_get_distinct_ids() {
        let (pool, database_path) = account_test_pool("identity-collision").await;
        let (first_email, second_email) = legacy_collision();
        allow_account(&pool, &first_email).await;
        allow_account(&pool, &second_email).await;
        assert_eq!(legacy_user_id(&first_email), legacy_user_id(&second_email));

        let first = handle_user_login(&pool, &first_email, "password", token(), 10)
            .await
            .unwrap();
        let second = handle_user_login(&pool, &second_email, "password", token(), 20)
            .await
            .unwrap();

        assert_ne!(first.id, second.id);
        let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(user_count, 2);

        pool.close().await;
        let _ = std::fs::remove_file(database_path);
    }

    #[tokio::test]
    async fn different_domains_with_same_local_part_create_distinct_accounts() {
        let (pool, database_path) = account_test_pool("identity-username").await;

        allow_account(&pool, "shared@example.com").await;
        allow_account(&pool, "shared@example.org").await;

        let first = handle_user_login(&pool, "shared@example.com", "password", token(), 10)
            .await
            .unwrap();
        let second = handle_user_login(&pool, "shared@example.org", "password", token(), 20)
            .await
            .unwrap();

        assert_ne!(first.id, second.id);
        let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(user_count, 2);

        pool.close().await;
        let _ = std::fs::remove_file(database_path);
    }

    #[tokio::test]
    async fn non_allowlisted_registration_is_rejected_and_durably_blacklisted() {
        let (pool, database_path) = account_test_pool("unauthorized-registration").await;
        let result =
            handle_user_login(&pool, " blocked@example.com ", "password", token(), 10).await;
        assert!(matches!(
            result.unwrap_err().downcast_ref::<AccountAccessError>(),
            Some(AccountAccessError::NotAllowlisted)
        ));

        let fresh = SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&database_path)
                .busy_timeout(Duration::from_secs(10)),
        )
        .await
        .unwrap();
        let source: String = sqlx::query_scalar(
            "SELECT source FROM account_blacklist WHERE account_key = 'blocked@example.com'",
        )
        .fetch_one(&fresh)
        .await
        .unwrap();
        assert_eq!(source, "unauthorized_registration");
        fresh.close().await;
        pool.close().await;
        let _ = std::fs::remove_file(database_path);
    }

    #[tokio::test]
    async fn invalid_account_and_password_errors_are_typed_and_sanitized() {
        let (pool, database_path) = account_test_pool("invalid-credentials").await;
        allow_account(&pool, "valid@example.com").await;

        let invalid_account = handle_user_login(
            &pool,
            "invalid account",
            "password-secret",
            TokenInfo {
                token: "token-secret".to_string(),
                refresh_token: "refresh-secret".to_string(),
                expires_at: 100,
            },
            10,
        )
        .await
        .unwrap_err();
        assert!(matches!(
            invalid_account.downcast_ref::<AccountAccessError>(),
            Some(AccountAccessError::InvalidFormat)
        ));
        let invalid_account_message = invalid_account.to_string();
        assert!(!invalid_account_message.contains("invalid account"));
        assert!(!invalid_account_message.contains("password-secret"));
        assert!(!invalid_account_message.contains("token-secret"));

        let invalid_password = handle_user_login(&pool, "valid@example.com", "short", token(), 20)
            .await
            .unwrap_err();
        assert!(matches!(
            invalid_password.downcast_ref::<AccountAccessError>(),
            Some(AccountAccessError::InvalidPassword)
        ));
        assert!(!invalid_password.to_string().contains("short"));

        let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();
        let activated_user_id: Option<i64> = sqlx::query_scalar(
            "SELECT activated_user_id FROM account_allowlist WHERE account_key = 'valid@example.com'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(user_count, 0);
        assert_eq!(activated_user_id, None);

        pool.close().await;
        let _ = std::fs::remove_file(database_path);
    }

    #[tokio::test]
    async fn allowlist_activation_binds_allocated_user_id() {
        let (pool, database_path) = account_test_pool("allowlist-binding").await;
        allow_account(&pool, "bound@example.com").await;
        let user = handle_user_login(&pool, "BOUND@example.com", "password", token(), 10)
            .await
            .unwrap();
        let activated: i64 = sqlx::query_scalar(
            "SELECT activated_user_id FROM account_allowlist WHERE account_key = 'bound@example.com'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(activated, user.id);
        pool.close().await;
        let _ = std::fs::remove_file(database_path);
    }

    #[tokio::test]
    async fn concurrent_allowed_registration_still_creates_one_user() {
        let (pool, database_path) = account_test_pool("concurrent-allowlisted").await;
        allow_account(&pool, "parallel@example.com").await;
        let first = handle_user_login(&pool, "parallel@example.com", "password", token(), 10);
        let second = handle_user_login(&pool, " PARALLEL@example.com ", "password", token(), 20);
        let (first, second) = tokio::join!(first, second);
        assert_eq!(first.unwrap().id, second.unwrap().id);
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1);
        pool.close().await;
        let _ = std::fs::remove_file(database_path);
    }

    #[tokio::test]
    async fn existing_login_rejects_removed_allowlist_without_blacklisting() {
        let (pool, database_path) = account_test_pool("removed-allowlist").await;
        allow_account(&pool, "removed@example.com").await;
        let user = handle_user_login(&pool, "removed@example.com", "password", token(), 10)
            .await
            .unwrap();
        sqlx::query("DELETE FROM account_allowlist WHERE account_key = 'removed@example.com'")
            .execute(&pool)
            .await
            .unwrap();

        let result = handle_user_login(
            &pool,
            "removed@example.com",
            "password",
            TokenInfo {
                token: "rotated".to_string(),
                refresh_token: "rotated-refresh".to_string(),
                expires_at: 200,
            },
            20,
        )
        .await;
        assert!(matches!(
            result.unwrap_err().downcast_ref::<AccountAccessError>(),
            Some(AccountAccessError::NotAllowlisted)
        ));
        let blacklist_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM account_blacklist WHERE account_key = 'removed@example.com'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let stored_token: String = sqlx::query_scalar("SELECT token FROM users WHERE id = ?")
            .bind(user.id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(blacklist_count, 0);
        assert_eq!(stored_token, "token");
        pool.close().await;
        let _ = std::fs::remove_file(database_path);
    }

    #[tokio::test]
    async fn existing_login_prioritizes_blacklist_over_password_verification() {
        let (pool, database_path) = account_test_pool("blacklisted-login").await;
        allow_account(&pool, "banned@example.com").await;
        let user = handle_user_login(&pool, "banned@example.com", "password", token(), 10)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO account_blacklist
                (account_key, user_id, source, reason, created_at, created_by)
             VALUES ('banned@example.com', ?, 'manual_ban', 'test', 20, 'test')",
        )
        .bind(user.id)
        .execute(&pool)
        .await
        .unwrap();

        let result =
            handle_user_login(&pool, "banned@example.com", "wrong-password", token(), 30).await;
        assert!(matches!(
            result.unwrap_err().downcast_ref::<AccountAccessError>(),
            Some(AccountAccessError::Blacklisted)
        ));
        pool.close().await;
        let _ = std::fs::remove_file(database_path);
    }
}
