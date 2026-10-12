-- Remote access (the Android client, docs/planned/android-client.md).
-- Paired devices: only the SHA-256 of each device token is stored (hex), never the token.
CREATE TABLE remote_devices (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    token_hash   TEXT NOT NULL UNIQUE,
    created_ms   INTEGER NOT NULL,
    last_seen_ms INTEGER
);

-- App-wide settings as key → JSON text (`remote.settings`, `remote.server_id`).
CREATE TABLE app_settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_ms INTEGER NOT NULL
);
