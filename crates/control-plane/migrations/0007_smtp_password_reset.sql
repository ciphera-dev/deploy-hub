-- Serveur SMTP de l'instance et reinitialisation du mot de passe.

-- Une seule ligne (id = true). Chiffrement obligatoire : STARTTLS ou TLS
-- implicite, jamais en clair. password_sealed : mot de passe chiffre par le
-- coffre age de l'instance (jamais en clair, jamais renvoye par l'API).
CREATE TABLE smtp_settings (
    id               boolean     PRIMARY KEY DEFAULT true CHECK (id),
    enabled          boolean     NOT NULL,
    host             text        NOT NULL,
    port             integer     NOT NULL CHECK (port BETWEEN 1 AND 65535),
    tls              text        NOT NULL CHECK (tls IN ('starttls', 'tls')),
    username         text        NOT NULL DEFAULT '',
    password_sealed  bytea,
    from_address     text        NOT NULL,
    from_name        text        NOT NULL,
    updated_at       timestamptz NOT NULL DEFAULT now(),
    updated_by       uuid        REFERENCES users (id) ON DELETE SET NULL
);

-- Liens de reinitialisation : usage unique, 30 minutes. Seul le SHA-256 du
-- jeton est stocke. Une nouvelle demande supprime les liens inutilises du
-- compte ; la consommation est un UPDATE ... WHERE used_at IS NULL, atomique.
CREATE TABLE password_resets (
    id          uuid        PRIMARY KEY,
    token_hash  bytea       NOT NULL UNIQUE,
    user_id     uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at  timestamptz NOT NULL,
    expires_at  timestamptz NOT NULL,
    used_at     timestamptz
);
CREATE INDEX password_resets_user_id_idx ON password_resets (user_id);
