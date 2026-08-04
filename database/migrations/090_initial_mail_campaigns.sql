CREATE TABLE IF NOT EXISTS user_mail_campaign_deliveries (
    campaign_id TEXT NOT NULL,
    user_id INTEGER NOT NULL REFERENCES users(id),
    sequence INTEGER NOT NULL,
    mail_incr_id INTEGER NOT NULL UNIQUE REFERENCES user_mails(incr_id),
    manifest_sha256 TEXT NOT NULL,
    delivered_at INTEGER NOT NULL,
    PRIMARY KEY (campaign_id, user_id, sequence)
);

CREATE INDEX IF NOT EXISTS idx_user_mail_campaign_deliveries_user
    ON user_mail_campaign_deliveries(user_id, campaign_id);
