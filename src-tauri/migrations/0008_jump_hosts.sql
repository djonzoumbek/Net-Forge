-- Jump host (ssh -J): an SSH profile may be reached through another SSH profile.
-- Deleting the jump profile leaves the other profiles direct (SET NULL).
ALTER TABLE profiles ADD COLUMN jump_profile_id INTEGER REFERENCES profiles(id) ON DELETE SET NULL;
