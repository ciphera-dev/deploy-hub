-- Projets, environnements, services et sources Git.
-- Un projet porte sa source (Git ou image) et sa methode de build ; l'interface
-- v0.1 ne cree que l'environnement `production`, le schema en accepte d'autres.

CREATE TABLE projects (
    id             uuid        PRIMARY KEY,
    name           text        NOT NULL CHECK (name ~ '^[a-z0-9][a-z0-9-]{0,62}$'),
    source_kind    text        NOT NULL CHECK (source_kind IN ('git', 'image')),
    -- Reference d'image (source `image` seulement).
    image_ref      text,
    build_kind     text        CHECK (build_kind IN ('railpack', 'dockerfile', 'compose')),
    build_path     text,
    build_service  text,
    created_by     uuid        REFERENCES users (id) ON DELETE SET NULL,
    created_at     timestamptz NOT NULL DEFAULT now(),
    updated_at     timestamptz NOT NULL DEFAULT now(),
    CHECK ((source_kind = 'image') = (image_ref IS NOT NULL)),
    CHECK ((source_kind = 'image') = (build_kind IS NULL)),
    CHECK ((build_kind IN ('dockerfile', 'compose')) = (build_path IS NOT NULL)),
    CHECK ((build_kind = 'compose') = (build_service IS NOT NULL))
);
CREATE UNIQUE INDEX projects_name_key ON projects (name);

CREATE TABLE git_sources (
    project_id  uuid PRIMARY KEY REFERENCES projects (id) ON DELETE CASCADE,
    -- `proprietaire/nom` sur GitHub.
    repository  text NOT NULL,
    branch      text NOT NULL
);

CREATE TABLE environments (
    id          uuid        PRIMARY KEY,
    project_id  uuid        NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    name        text        NOT NULL CHECK (name ~ '^[a-z0-9][a-z0-9-]{0,62}$'),
    created_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, name)
);

CREATE TABLE services (
    id              uuid        PRIMARY KEY,
    environment_id  uuid        NOT NULL REFERENCES environments (id) ON DELETE CASCADE,
    name            text        NOT NULL,
    created_at      timestamptz NOT NULL DEFAULT now(),
    UNIQUE (environment_id, name)
);
