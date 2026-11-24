-- Verification d'adresse e-mail : changement d'adresse et inscription ouverte.
-- Le jeton n'est jamais stocke : seule son empreinte SHA-256 l'est. Usage
-- unique (`used_at`), valable 24 heures (`expires_at`).
CREATE TABLE email_verifications (
    id          uuid        PRIMARY KEY,
    token_hash  bytea       NOT NULL UNIQUE,
    user_id     uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    new_email   text        NOT NULL,
    purpose     text        NOT NULL CHECK (purpose IN ('change', 'signup')),
    created_at  timestamptz NOT NULL,
    expires_at  timestamptz NOT NULL,
    used_at     timestamptz
);
CREATE INDEX email_verifications_user_id_idx ON email_verifications (user_id);

-- NULL : adresse non verifiee (inscription ouverte en attente), connexion
-- refusee. Les comptes existants et ceux crees par l'assistant ou par une
-- invitation (lien recu a cette adresse) prennent la date de creation par defaut.
ALTER TABLE users ADD COLUMN email_verified_at timestamptz DEFAULT now();
