-- Telnet profiles have no SSH authentication (auth_method 'none') and the
-- protocol column is now constrained. SQLite cannot alter a CHECK constraint:
-- the table is rebuilt and the data copied (official SQLite procedure). Nothing
-- references `profiles`, so foreign keys can stay enabled.
CREATE TABLE profiles_new (
    id          INTEGER PRIMARY KEY,
    folder_id   INTEGER REFERENCES folders(id) ON DELETE SET NULL,
    name        TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    protocol    TEXT    NOT NULL DEFAULT 'ssh' CHECK (protocol IN ('ssh', 'telnet')),
    host        TEXT    NOT NULL,
    port        INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
    username    TEXT    NOT NULL,
    auth_method TEXT    NOT NULL CHECK (auth_method IN ('none', 'password', 'keyFile', 'agent')),
    key_path    TEXT,
    secret_ref  TEXT,
    legacy_mode INTEGER NOT NULL DEFAULT 0 CHECK (legacy_mode IN (0, 1)),
    created_at  TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at  TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO profiles_new (id, folder_id, name, protocol, host, port, username, auth_method,
                          key_path, secret_ref, legacy_mode, created_at, updated_at)
SELECT id, folder_id, name, protocol, host, port, username, auth_method,
       key_path, secret_ref, legacy_mode, created_at, updated_at
FROM profiles;

DROP TABLE profiles;
ALTER TABLE profiles_new RENAME TO profiles;
CREATE INDEX profiles_folder ON profiles(folder_id);
