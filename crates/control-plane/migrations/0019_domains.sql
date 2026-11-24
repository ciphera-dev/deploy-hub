-- Domaines des projets : domaines personnalises et domaine genere
-- (`<projet>.<ip>.sslip.io`), verification DNS, certificat ACME (emis et
-- garde par le proxy du noeud ; seuls son etat et sa fin de validite sont
-- ici), domaine principal et redirection 308 vers lui.

-- Domaine genere active pour le projet (desactivable).
ALTER TABLE projects ADD COLUMN generated_domain boolean NOT NULL DEFAULT true;

CREATE TABLE domains (
    id               uuid        PRIMARY KEY,
    project_id       uuid        NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    environment_id   uuid        NOT NULL REFERENCES environments (id) ON DELETE CASCADE,
    -- Service servi (sans valeur : redirection seulement, ou service retire).
    service_id       uuid        REFERENCES services (id) ON DELETE SET NULL,
    -- Minuscules, sans point final ni port (valide par l'API).
    host             text        NOT NULL CHECK (host ~ '^[a-z0-9]([a-z0-9.-]{0,251}[a-z0-9])?$'),
    path_prefix      text        NOT NULL DEFAULT '/' CHECK (path_prefix ~ '^/[!-~]*$' AND length(path_prefix) <= 1024),
    generated        boolean     NOT NULL DEFAULT false,
    is_primary       boolean     NOT NULL DEFAULT false,
    -- Redirection 308 vers le domaine principal de l'environnement.
    redirect         boolean     NOT NULL DEFAULT false,
    dns_status       text        NOT NULL DEFAULT 'pending'
                     CHECK (dns_status IN ('pending', 'ok', 'error')),
    dns_error        text,
    dns_checked_at   timestamptz,
    cert_status      text        NOT NULL DEFAULT 'none'
                     CHECK (cert_status IN ('none', 'issuing', 'active', 'error')),
    cert_expires_at  timestamptz,
    cert_issuer      text,
    cert_error       text,
    created_by       uuid        REFERENCES users (id) ON DELETE SET NULL,
    created_at       timestamptz NOT NULL DEFAULT now(),
    updated_at       timestamptz NOT NULL DEFAULT now(),
    CHECK (NOT (is_primary AND redirect))
);

-- Un hote n'appartient qu'a un seul environnement de toute l'instance : un
-- projet ne peut pas capter le trafic d'un autre.
CREATE UNIQUE INDEX domains_host_key ON domains (lower(host));
-- Un seul domaine principal par environnement.
CREATE UNIQUE INDEX domains_primary_per_env ON domains (environment_id) WHERE is_primary;
CREATE INDEX domains_project ON domains (project_id);
