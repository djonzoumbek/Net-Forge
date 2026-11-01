-- SSH port forwards saved in a profile, started when the session connects.
--   kind 'local':   ssh -L bind_address:bind_port:target_host:target_port
--   kind 'dynamic': ssh -D bind_address:bind_port (SOCKS5), no target
-- `position` keeps the order chosen by the user.
CREATE TABLE port_forwards (
    profile_id   INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    position     INTEGER NOT NULL,
    kind         TEXT    NOT NULL CHECK (kind IN ('local', 'dynamic')),
    bind_address TEXT    NOT NULL,
    bind_port    INTEGER NOT NULL CHECK (bind_port BETWEEN 0 AND 65535),
    target_host  TEXT    NOT NULL DEFAULT '',
    target_port  INTEGER NOT NULL DEFAULT 0 CHECK (target_port BETWEEN 0 AND 65535),
    PRIMARY KEY (profile_id, position)
);
