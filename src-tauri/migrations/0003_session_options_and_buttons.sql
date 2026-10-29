-- Phase 5: per-profile session behaviour, and the command bar buttons.

-- SSH keepalive / Telnet NOP interval in seconds (0 = disabled).
ALTER TABLE profiles ADD COLUMN keepalive_secs INTEGER NOT NULL DEFAULT 30
    CHECK (keepalive_secs BETWEEN 0 AND 3600);
-- Reconnect automatically when the connection is lost (never after `exit`).
ALTER TABLE profiles ADD COLUMN auto_reconnect INTEGER NOT NULL DEFAULT 0
    CHECK (auto_reconnect IN (0, 1));
-- Answer pager prompts (--More--, ---- More ----) with a space.
ALTER TABLE profiles ADD COLUMN pager_auto_reply INTEGER NOT NULL DEFAULT 0
    CHECK (pager_auto_reply IN (0, 1));
-- One command per line, sent once at the first prompt (e.g. `terminal length 0`).
-- Stored in clear: never put a password here.
ALTER TABLE profiles ADD COLUMN login_commands TEXT NOT NULL DEFAULT '';

CREATE TABLE command_buttons (
    id         INTEGER PRIMARY KEY,
    label      TEXT    NOT NULL CHECK (length(trim(label)) > 0),
    -- One command per line, typed in order.
    commands   TEXT    NOT NULL,
    -- Pause between two lines, in milliseconds.
    delay_ms   INTEGER NOT NULL DEFAULT 0 CHECK (delay_ms BETWEEN 0 AND 60000),
    position   INTEGER NOT NULL DEFAULT 0,
    created_at TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
);
