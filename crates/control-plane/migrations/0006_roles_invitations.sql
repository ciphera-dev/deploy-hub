-- Roles fixes et invitations.

-- Inscription publique : desactivee par defaut (comptes crees sur invitation).
ALTER TABLE instance ADD COLUMN public_signup boolean NOT NULL DEFAULT false;

-- Invitations : lien a usage unique valable 72 heures. Seul le SHA-256 du
-- jeton est stocke, jamais le jeton. used_at : invitation consommee (la
-- consommation est un UPDATE ... WHERE used_at IS NULL, atomique).
CREATE TABLE invitations (
    id          uuid PRIMARY KEY,
    token_hash  bytea       NOT NULL UNIQUE,
    email       text        NOT NULL,
    name        text        NOT NULL DEFAULT '',
    role        text        NOT NULL CHECK (role IN ('owner', 'admin', 'ops', 'developer', 'viewer')),
    invited_by  uuid        REFERENCES users (id) ON DELETE SET NULL,
    created_at  timestamptz NOT NULL,
    expires_at  timestamptz NOT NULL,
    used_at     timestamptz
);
CREATE INDEX invitations_email_lower_idx ON invitations (lower(email));
