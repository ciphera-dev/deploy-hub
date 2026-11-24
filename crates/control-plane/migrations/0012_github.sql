-- GitHub App de l'instance (flux « manifest ») et webhooks entrants.

-- Une seule App par instance (id = true). La cle privee et le secret de
-- webhook sont chiffres par le coffre age de l'instance : jamais en clair,
-- jamais renvoyes par l'API. Les jetons d'installation ne sont jamais stockes
-- (cache en memoire seulement).
CREATE TABLE github_app (
    id                     boolean     PRIMARY KEY DEFAULT true CHECK (id),
    app_id                 bigint      NOT NULL,
    slug                   text        NOT NULL,
    name                   text        NOT NULL,
    owner_login            text        NOT NULL,
    html_url               text        NOT NULL,
    private_key_sealed     bytea       NOT NULL,
    webhook_secret_sealed  bytea       NOT NULL,
    created_by             uuid        REFERENCES users (id) ON DELETE SET NULL,
    created_at             timestamptz NOT NULL DEFAULT now()
);

-- `state` du flux manifest : aleatoire, usage unique, 1 h, lie a la session
-- de l'admin qui l'a demande. Seul le SHA-256 est stocke.
CREATE TABLE github_manifest_states (
    state_hash  bytea       PRIMARY KEY,
    session_id  uuid        NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    expires_at  timestamptz NOT NULL
);
CREATE INDEX github_manifest_states_expires_at_idx ON github_manifest_states (expires_at);

-- Anti-rejeu des webhooks (`X-GitHub-Delivery`) : entrees de plus de 24 h purgees.
CREATE TABLE github_deliveries (
    delivery_id  text        PRIMARY KEY,
    received_at  timestamptz NOT NULL
);
CREATE INDEX github_deliveries_received_at_idx ON github_deliveries (received_at);
