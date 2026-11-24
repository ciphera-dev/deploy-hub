-- Schema initial du plan de controle.

CREATE TABLE instance (
    id          uuid PRIMARY KEY,
    name        text NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE users (
    id             uuid PRIMARY KEY,
    email          text NOT NULL,
    password_hash  text NOT NULL,
    role           text NOT NULL CHECK (role IN ('owner', 'admin', 'ops', 'developer', 'viewer')),
    created_at     timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX users_email_lower_key ON users (lower(email));

-- Seul le hachage du jeton est stocke, jamais le jeton.
CREATE TABLE sessions (
    id          uuid PRIMARY KEY,
    user_id     uuid REFERENCES users (id) ON DELETE CASCADE,
    token_hash  bytea NOT NULL UNIQUE,
    created_at  timestamptz NOT NULL DEFAULT now(),
    expires_at  timestamptz NOT NULL
);
CREATE INDEX sessions_expires_at_idx ON sessions (expires_at);

CREATE TABLE audit_events (
    id          uuid PRIMARY KEY,
    at          timestamptz NOT NULL DEFAULT now(),
    actor_id    uuid REFERENCES users (id) ON DELETE SET NULL,
    action      text NOT NULL,
    target      text,
    detail      jsonb NOT NULL DEFAULT '{}'::jsonb
);
CREATE INDEX audit_events_at_idx ON audit_events (at DESC);
