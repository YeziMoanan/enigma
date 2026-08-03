CREATE TABLE IF NOT EXISTS user_initial_mail_deliveries (
    user_id INTEGER NOT NULL,
    category TEXT NOT NULL,
    delivered_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, category)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_users_normalized_email
    ON users(LOWER(TRIM(email)))
    WHERE email IS NOT NULL;
