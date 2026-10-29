-- Folders can be nested. Deleting a folder deletes its sub-folders; the profiles
-- they contain are moved back to the root (folder_id set to NULL), never deleted.
CREATE TABLE folders (
    id         INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    parent_id  INTEGER REFERENCES folders(id) ON DELETE CASCADE,
    created_at TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- No secret here: `secret_ref` is only the name of the system keyring entry
-- holding the password or key passphrase (NULL when nothing is saved).
CREATE TABLE profiles (
    id          INTEGER PRIMARY KEY,
    folder_id   INTEGER REFERENCES folders(id) ON DELETE SET NULL,
    name        TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    protocol    TEXT    NOT NULL DEFAULT 'ssh',
    host        TEXT    NOT NULL,
    port        INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
    username    TEXT    NOT NULL,
    auth_method TEXT    NOT NULL CHECK (auth_method IN ('password', 'keyFile', 'agent')),
    key_path    TEXT,
    secret_ref  TEXT,
    legacy_mode INTEGER NOT NULL DEFAULT 0 CHECK (legacy_mode IN (0, 1)),
    created_at  TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at  TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX profiles_folder ON profiles(folder_id);
