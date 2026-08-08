use sqlx::sqlite::SqlitePoolOptions;

fn init_test_data() {
    let data_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("sonetto-data/excel2json");
    let _ = config::init(data_dir.to_str().unwrap());
}

#[tokio::test]
async fn mail_red_dot_only_changes_when_last_unread_mail_is_claimed() {
    init_test_data();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    database::run_migrations(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO users (id, username, created_at, updated_at) VALUES (1, 'mail', 0, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO user_mails (incr_id, user_id, mail_id, create_time, expire_time)
             VALUES (1, 1, 1, 0, 0), (2, 1, 2, 0, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let manager = super::MailManager::new(1);
    let (_, first) = manager.claim_one(&pool, 1).await.unwrap();
    assert_eq!(first.mail_red_dot, None);

    let (_, last) = manager.claim_one(&pool, 2).await.unwrap();
    assert_eq!(last.mail_red_dot.map(|dot| dot.0), Some(0));
}

#[tokio::test]
async fn repeated_mail_claim_does_not_repeat_rewards() {
    init_test_data();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    database::run_migrations(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO users (id, username, created_at, updated_at) VALUES (2, 'claim', 0, 0);
         INSERT INTO user_mails
             (incr_id, user_id, mail_id, attachment, create_time, expire_time)
         VALUES (3, 2, 3, '2#11#25', 0, 0);",
    )
    .execute(&pool)
    .await
    .unwrap();

    let manager = super::MailManager::new(2);
    manager.claim_one(&pool, 3).await.unwrap();
    let first_quantity: i32 = sqlx::query_scalar(
        "SELECT quantity FROM currencies WHERE user_id = 2 AND currency_id = 11",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let (_, repeated) = manager.claim_one(&pool, 3).await.unwrap();
    let repeated_quantity: i32 = sqlx::query_scalar(
        "SELECT quantity FROM currencies WHERE user_id = 2 AND currency_id = 11",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(first_quantity, 25);
    assert_eq!(repeated_quantity, 25);
    assert!(repeated.rewards.currency_ids.is_empty());
    assert!(repeated.material_changes.is_empty());
}

#[tokio::test]
async fn claimed_mail_remains_in_mailbox() {
    init_test_data();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    database::run_migrations(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO users (id, username, created_at, updated_at) VALUES (4, 'retained', 0, 0);
         INSERT INTO user_mails
            (incr_id, user_id, mail_id, attachment, state, create_time, expire_time)
         VALUES (6, 4, 0, '2#11#25', 0, 0, 0);",
    )
    .execute(&pool)
    .await
    .unwrap();

    let manager = super::MailManager::new(4);
    manager.claim_batch(&pool).await.unwrap();
    let mailbox = manager.get_all(&pool).await.unwrap();

    assert_eq!(mailbox.mails.len(), 1);
    assert_eq!(mailbox.mails[0].incr_id, Some(6));
    assert_eq!(mailbox.mails[0].state, Some(1));
}

#[tokio::test]
async fn deleting_claimed_campaign_mail_preserves_delivery_ledger() {
    init_test_data();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    database::run_migrations(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO users (id, username, created_at, updated_at)
             VALUES (5, 'delete-campaign-mail', 0, 0);
         INSERT INTO user_mails
             (incr_id, user_id, mail_id, attachment, state, create_time, expire_time)
             VALUES (7, 5, 0, '2#11#1', 1, 0, 0);
         INSERT INTO user_mail_campaign_deliveries
             (campaign_id, user_id, sequence, mail_incr_id, manifest_sha256, delivered_at)
             VALUES ('initial-full-v1', 5, 1, 7, 'test-sha', 0);",
    )
    .execute(&pool)
    .await
    .unwrap();

    let deleted = super::MailManager::new(5)
        .delete_claimed_unlocked(&pool)
        .await
        .unwrap();

    assert_eq!(deleted.incr_ids, vec![7]);
    let mail_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_mails WHERE incr_id = 7")
        .fetch_one(&pool)
        .await
        .unwrap();
    let ledger_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM user_mail_campaign_deliveries
         WHERE campaign_id = 'initial-full-v1' AND user_id = 5 AND sequence = 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(mail_count, 0);
    assert_eq!(ledger_count, 1);
}

#[tokio::test]
async fn invalid_attachment_rolls_back_all_mail_state_and_rewards() {
    init_test_data();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    database::run_migrations(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO users (id, username, created_at, updated_at) VALUES (3, 'rollback', 0, 0);
         INSERT INTO user_mails
             (incr_id, user_id, mail_id, attachment, create_time, expire_time)
         VALUES (4, 3, 4, '2#11#25', 0, 0), (5, 3, 5, 'broken', 0, 0);",
    )
    .execute(&pool)
    .await
    .unwrap();

    assert!(super::MailManager::new(3).claim_batch(&pool).await.is_err());
    let claimed: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM user_mails WHERE user_id = 3 AND state = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    let currency_rows: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM currencies WHERE user_id = 3")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(claimed, 0);
    assert_eq!(currency_rows, 0);
}
