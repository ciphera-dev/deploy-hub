-- Serveurs geres (agents). Un agent n'est accepte sur le lien QUIC que si
-- son certificat client chaine vers la CA de l'instance ET que son empreinte
-- correspond a une ligne non revoquee de cet identifiant.

CREATE TABLE servers (
    id                uuid        PRIMARY KEY,
    name              text        NOT NULL,
    kind              text        NOT NULL CHECK (kind IN ('local', 'remote')),
    -- SHA-256 du certificat DER de l'agent, `AB:CD:...` (majuscules).
    cert_fingerprint  text        NOT NULL UNIQUE,
    created_at        timestamptz NOT NULL DEFAULT now(),
    revoked_at        timestamptz
);

-- Un seul serveur local (l'agent de la machine du plan de controle).
CREATE UNIQUE INDEX servers_single_local ON servers (kind) WHERE kind = 'local';
