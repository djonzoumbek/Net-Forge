-- Clipboard behaviour of the terminal (application settings).
--   copy_on_select:    a mouse selection is copied to the clipboard at once.
--   right_click_paste: a right click pastes the clipboard (PuTTY / SecureCRT style).
--   ctrl_v_paste:      Ctrl+V pastes instead of sending ^V to the remote shell.
ALTER TABLE settings ADD COLUMN copy_on_select INTEGER NOT NULL DEFAULT 1 CHECK (copy_on_select IN (0, 1));
ALTER TABLE settings ADD COLUMN right_click_paste INTEGER NOT NULL DEFAULT 1 CHECK (right_click_paste IN (0, 1));
ALTER TABLE settings ADD COLUMN ctrl_v_paste INTEGER NOT NULL DEFAULT 1 CHECK (ctrl_v_paste IN (0, 1));
