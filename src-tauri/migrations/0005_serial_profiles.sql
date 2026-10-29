-- Phase 7: serial console profiles.
--
-- For a serial profile, `host` holds the device path (e.g. /dev/ttyUSB0), `port`
-- is 0 and `auth_method` is 'none' (the device asks for credentials in the
-- terminal). The line settings get their own columns.
--
-- The `protocol` and `port` CHECK constraints change: SQLite cannot alter them,
-- so the table is rebuilt and the data copied (same procedure as 0002).
CREATE TABLE profiles_new (
    id                 INTEGER PRIMARY KEY,
    folder_id          INTEGER REFERENCES folders(id) ON DELETE SET NULL,
    name               TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    protocol           TEXT    NOT NULL DEFAULT 'ssh' CHECK (protocol IN ('ssh', 'telnet', 'serial')),
    host               TEXT    NOT NULL,
    port               INTEGER NOT NULL CHECK (
                           (protocol = 'serial' AND port = 0) OR port BETWEEN 1 AND 65535),
    username           TEXT    NOT NULL,
    auth_method        TEXT    NOT NULL CHECK (auth_method IN ('none', 'password', 'keyFile', 'agent')),
    key_path           TEXT,
    secret_ref         TEXT,
    legacy_mode        INTEGER NOT NULL DEFAULT 0 CHECK (legacy_mode IN (0, 1)),
    created_at         TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at         TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP,
    keepalive_secs     INTEGER NOT NULL DEFAULT 30 CHECK (keepalive_secs BETWEEN 0 AND 3600),
    auto_reconnect     INTEGER NOT NULL DEFAULT 0 CHECK (auto_reconnect IN (0, 1)),
    pager_auto_reply   INTEGER NOT NULL DEFAULT 0 CHECK (pager_auto_reply IN (0, 1)),
    login_commands     TEXT    NOT NULL DEFAULT '',
    session_log        TEXT    NOT NULL DEFAULT 'off' CHECK (session_log IN ('off', 'text', 'raw')),
    -- Line settings, used by serial profiles only (defaults: 9600 8N1, the
    -- console settings of Cisco and Huawei devices).
    serial_baud_rate   INTEGER NOT NULL DEFAULT 9600 CHECK (serial_baud_rate BETWEEN 50 AND 4000000),
    serial_data_bits   INTEGER NOT NULL DEFAULT 8 CHECK (serial_data_bits IN (5, 6, 7, 8)),
    serial_parity      TEXT    NOT NULL DEFAULT 'none' CHECK (serial_parity IN ('none', 'odd', 'even')),
    serial_stop_bits   INTEGER NOT NULL DEFAULT 1 CHECK (serial_stop_bits IN (1, 2)),
    serial_flow_control TEXT   NOT NULL DEFAULT 'none'
                           CHECK (serial_flow_control IN ('none', 'software', 'hardware'))
);

INSERT INTO profiles_new (id, folder_id, name, protocol, host, port, username, auth_method,
                          key_path, secret_ref, legacy_mode, created_at, updated_at,
                          keepalive_secs, auto_reconnect, pager_auto_reply, login_commands,
                          session_log)
SELECT id, folder_id, name, protocol, host, port, username, auth_method,
       key_path, secret_ref, legacy_mode, created_at, updated_at,
       keepalive_secs, auto_reconnect, pager_auto_reply, login_commands,
       session_log
FROM profiles;

DROP TABLE profiles;
ALTER TABLE profiles_new RENAME TO profiles;
CREATE INDEX profiles_folder ON profiles(folder_id);
