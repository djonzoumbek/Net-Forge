-- Configuration backup: the command last used for the profile
-- (`show running-config`, `display current-configuration`...).
ALTER TABLE profiles ADD COLUMN backup_command TEXT NOT NULL DEFAULT '';
