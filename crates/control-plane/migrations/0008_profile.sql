-- Profil : preferences du compte, avatar, preferences de notification.

-- Langue de l'interface (fr, en) et fuseau horaire IANA (valide par le
-- serveur avant ecriture).
ALTER TABLE users
    ADD COLUMN language  text NOT NULL DEFAULT 'fr' CHECK (language IN ('fr', 'en')),
    ADD COLUMN timezone  text NOT NULL DEFAULT 'UTC';

-- Avatar : image deja decodee, recadree et re-encodee en PNG par le serveur
-- (jamais les octets envoyes). updated_at sert de version dans l'URL.
CREATE TABLE user_avatars (
    user_id     uuid        PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    png         bytea       NOT NULL,
    updated_at  timestamptz NOT NULL DEFAULT now()
);

-- Notifications du compte : canal e-mail seulement en v0.1. Sans ligne, les
-- valeurs par defaut du serveur s'appliquent (http::me::DEFAULT_PREFS).
CREATE TABLE notification_prefs (
    user_id           uuid        PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    email_enabled     boolean     NOT NULL,
    deploy_succeeded  boolean     NOT NULL,
    deploy_failed     boolean     NOT NULL,
    certificates      boolean     NOT NULL,
    team              boolean     NOT NULL,
    updated_at        timestamptz NOT NULL DEFAULT now()
);
