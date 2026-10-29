-- Phase 6: session logging per profile and application settings.

-- 'off': no log; 'text': plain text, escape sequences removed (readable with any
-- editor); 'raw': exact bytes received (replayable with `cat`).
-- The log may contain secrets typed in the terminal: it is an explicit opt-in.
ALTER TABLE profiles ADD COLUMN session_log TEXT NOT NULL DEFAULT 'off'
    CHECK (session_log IN ('off', 'text', 'raw'));

-- A single row (id = 1): typed columns with their constraints.
CREATE TABLE settings (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    scrollback  INTEGER NOT NULL DEFAULT 10000 CHECK (scrollback BETWEEN 1000 AND 200000),
    theme       TEXT    NOT NULL DEFAULT 'netforge-dark',
    font_family TEXT    NOT NULL DEFAULT 'DejaVu Sans Mono',
    font_size   INTEGER NOT NULL DEFAULT 14 CHECK (font_size BETWEEN 8 AND 36),
    -- Empty: default directory (~/Documents/NetForge/logs).
    log_dir     TEXT    NOT NULL DEFAULT ''
);
INSERT INTO settings (id) VALUES (1);
