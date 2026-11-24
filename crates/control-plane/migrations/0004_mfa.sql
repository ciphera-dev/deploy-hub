-- Double authentification : TOTP, codes de secours, 2FA imposee par
-- l'organisation.

-- Une TOTP par compte. secret_sealed : graine chiffree par le coffre age de
-- l'instance (jamais en clair). confirmed_at NULL : enrolement en cours (un
-- nouveau `start` remplace la graine). last_used_step : derniere fenetre de
-- 30 s acceptee ; une fenetre inferieure ou egale est refusee (pas de rejeu).
CREATE TABLE user_totp (
    user_id         uuid PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    secret_sealed   bytea       NOT NULL,
    confirmed_at    timestamptz,
    last_used_step  bigint,
    created_at      timestamptz NOT NULL DEFAULT now()
);

-- Codes de secours : hachage Argon2id (PHC) seulement, usage unique.
CREATE TABLE recovery_codes (
    id          uuid PRIMARY KEY,
    user_id     uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    code_hash   text        NOT NULL,
    used_at     timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX recovery_codes_user_id_idx ON recovery_codes (user_id);

-- Echecs de second facteur et de re-authentification (fenetre glissante de
-- 15 minutes), par compte et par session.
CREATE TABLE mfa_attempts (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    at          timestamptz NOT NULL DEFAULT now(),
    user_id     uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    session_id  uuid        NOT NULL,
    succeeded   boolean     NOT NULL
);
CREATE INDEX mfa_attempts_user_at_idx ON mfa_attempts (user_id, at);
CREATE INDEX mfa_attempts_session_at_idx ON mfa_attempts (session_id, at);

-- 2FA obligatoire pour toute l'organisation (reglage expose par la Task 32).
ALTER TABLE instance ADD COLUMN mfa_required boolean NOT NULL DEFAULT false;

-- Session ouverte par le mot de passe alors que l'organisation impose la 2FA
-- et que le compte n'en a pas : seuls GET /api/me, l'enrolement TOTP et la
-- deconnexion lui sont ouverts.
ALTER TABLE sessions ADD COLUMN enrollment_required boolean NOT NULL DEFAULT false;
