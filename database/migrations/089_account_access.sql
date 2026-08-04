CREATE TABLE account_allowlist (
    account_key TEXT PRIMARY KEY,
    display_account TEXT NOT NULL,
    batch_id TEXT NOT NULL,
    imported_at INTEGER NOT NULL,
    imported_by TEXT NOT NULL,
    activated_user_id INTEGER UNIQUE REFERENCES users(id)
);

CREATE TABLE account_blacklist (
    account_key TEXT PRIMARY KEY,
    user_id INTEGER REFERENCES users(id),
    source TEXT NOT NULL CHECK(source IN ('unauthorized_registration','manual_ban')),
    reason TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    created_by TEXT NOT NULL
);

CREATE TABLE account_admin_requests (
    request_id TEXT PRIMARY KEY,
    operation TEXT NOT NULL,
    payload_sha256 TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('prepared','applied','unknown','rejected')),
    result_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
