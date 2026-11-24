-- Echecs TOTP distingues des autres (codes de secours, passkeys, mot de
-- passe redemande) : seul la TOTP (10^6 codes) a un plafond journalier.
ALTER TABLE mfa_attempts ADD COLUMN totp boolean NOT NULL DEFAULT false;
