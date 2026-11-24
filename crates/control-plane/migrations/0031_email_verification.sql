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
-- refusee. Les comptes existants sont verifies a leur date de creation. Pas de
-- valeur par defaut : chaque chemin de creation de compte (assistant,
-- invitation, inscription) dit explicitement s'il est verifie.
ALTER TABLE users ADD COLUMN email_verified_at timestamptz;
UPDATE users SET email_verified_at = created_at;

-- Derniere demande de changement d'adresse (adresse libre, prise ou autre) :
-- le delai entre deux demandes se mesure ici, pas sur les liens restants.
ALTER TABLE users ADD COLUMN email_change_requested_at timestamptz;
