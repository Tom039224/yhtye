-- What a paired device is (`ClientKind`, serde name): the Android client or another
-- Yhtye desktop app (`desktop`). Devices paired before this column are Android.
ALTER TABLE remote_devices ADD COLUMN kind TEXT NOT NULL DEFAULT 'android';
