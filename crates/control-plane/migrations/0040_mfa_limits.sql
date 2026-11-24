-- Limites du second facteur : adresse (regroupee) des tentatives de passkey et
-- de code de secours, pour le plafond par IP ; index des echecs de mot de
-- passe par compte (delai croissant a la connexion).
ALTER TABLE mfa_attempts ADD COLUMN ip text;
CREATE INDEX mfa_attempts_ip_at_idx ON mfa_attempts (ip, at) WHERE ip IS NOT NULL;
CREATE INDEX login_attempts_email_at_idx ON login_attempts (email_hash, at);
