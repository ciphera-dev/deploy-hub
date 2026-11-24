-- Passkeys (WebAuthn) : credentials des comptes et ceremonies en cours.

-- passkey : credential serialise par webauthn-rs (cle publique, compteur de
-- signatures, drapeaux) ; jamais renvoye par l'API. credential_id : identifiant
-- attribue par l'authentificateur, unique sur toute l'instance.
CREATE TABLE webauthn_credentials (
    id             uuid PRIMARY KEY,
    user_id        uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    credential_id  bytea       NOT NULL UNIQUE,
    passkey        jsonb       NOT NULL,
    name           text        NOT NULL,
    created_at     timestamptz NOT NULL DEFAULT now(),
    last_used_at   timestamptz
);
CREATE INDEX webauthn_credentials_user_id_idx ON webauthn_credentials (user_id);

-- Ceremonies (defi et etat cote serveur), a usage unique, 5 minutes.
-- register / second_factor : liees a la session qui les a demandees (une
-- rotation de session les supprime) ; passwordless : sans session, bornees
-- par adresse IP.
CREATE TABLE webauthn_ceremonies (
    id          uuid PRIMARY KEY,
    kind        text        NOT NULL CHECK (kind IN ('register', 'second_factor', 'passwordless')),
    session_id  uuid        REFERENCES sessions (id) ON DELETE CASCADE,
    ip          text        NOT NULL,
    state       jsonb       NOT NULL,
    expires_at  timestamptz NOT NULL,
    CHECK ((kind = 'passwordless') = (session_id IS NULL))
);
CREATE INDEX webauthn_ceremonies_session_id_idx ON webauthn_ceremonies (session_id);
CREATE INDEX webauthn_ceremonies_ip_idx ON webauthn_ceremonies (ip, expires_at);
CREATE INDEX webauthn_ceremonies_expires_at_idx ON webauthn_ceremonies (expires_at);
