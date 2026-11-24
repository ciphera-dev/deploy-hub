-- Connexion par mot de passe : etat des sessions et limitation de debit.

-- mfa_pending : mot de passe verifie, double authentification attendue
--   (la session ne donne alors acces qu'a /api/auth/mfa/*).
-- persistent  : « rester connecte » (30 jours, cookie persistant) ; sinon
--   12 heures et cookie de session du navigateur. Conserve a la rotation.
-- ip / user_agent : metadonnees affichees dans la liste des sessions.
ALTER TABLE sessions
    ADD COLUMN mfa_pending   boolean     NOT NULL DEFAULT false,
    ADD COLUMN persistent    boolean     NOT NULL DEFAULT false,
    ADD COLUMN last_seen_at  timestamptz NOT NULL DEFAULT now(),
    ADD COLUMN ip            text,
    ADD COLUMN user_agent    text;
CREATE INDEX sessions_user_id_idx ON sessions (user_id);

-- Tentatives de connexion (fenetre glissante de 15 minutes). L'e-mail n'est
-- pas stocke : seulement le SHA-256 de sa forme en minuscules.
CREATE TABLE login_attempts (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    at          timestamptz NOT NULL DEFAULT now(),
    ip          text        NOT NULL,
    email_hash  bytea       NOT NULL,
    succeeded   boolean     NOT NULL
);
CREATE INDEX login_attempts_ip_email_at_idx ON login_attempts (ip, email_hash, at);
CREATE INDEX login_attempts_ip_at_idx ON login_attempts (ip, at);
CREATE INDEX login_attempts_at_idx ON login_attempts (at);
