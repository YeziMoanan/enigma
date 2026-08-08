CREATE TABLE user_mail_campaign_deliveries_v2 (
    campaign_id TEXT NOT NULL,
    user_id INTEGER NOT NULL REFERENCES users(id),
    sequence INTEGER NOT NULL,
    mail_incr_id INTEGER NOT NULL UNIQUE,
    manifest_sha256 TEXT NOT NULL,
    delivered_at INTEGER NOT NULL,
    PRIMARY KEY (campaign_id, user_id, sequence)
);

INSERT INTO user_mail_campaign_deliveries_v2
    (campaign_id, user_id, sequence, mail_incr_id, manifest_sha256, delivered_at)
SELECT campaign_id, user_id, sequence, mail_incr_id, manifest_sha256, delivered_at
FROM user_mail_campaign_deliveries;

DROP TABLE user_mail_campaign_deliveries;
ALTER TABLE user_mail_campaign_deliveries_v2 RENAME TO user_mail_campaign_deliveries;
