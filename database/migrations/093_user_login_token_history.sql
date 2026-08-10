CREATE TABLE IF NOT EXISTS user_login_token_history (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL,
    expires_at INTEGER,
    created_at INTEGER NOT NULL,
    source TEXT NOT NULL,
    PRIMARY KEY (user_id, token_hash)
);

CREATE INDEX IF NOT EXISTS idx_user_login_token_history_expiry
    ON user_login_token_history (expires_at);
