-- Variables d'environnement : par environnement d'un projet, et globales
-- (portee environnements x projets). Un secret n'est stocke que chiffre (age,
-- identite de l'instance) dans `value_sealed` et n'est jamais relu par l'API ;
-- une valeur publique est en clair dans `value`. Les horodatages viennent de
-- l'horloge de l'application (`Clock`), comme ceux des deploiements.

CREATE TABLE project_variables (
    id              uuid        PRIMARY KEY,
    environment_id  uuid        NOT NULL REFERENCES environments (id) ON DELETE CASCADE,
    key             text        NOT NULL CHECK (key ~ '^[A-Z_][A-Z0-9_]{0,127}$'),
    secret          boolean     NOT NULL,
    value           text        CHECK (octet_length(value) <= 32768),
    value_sealed    bytea,
    updated_by      uuid        REFERENCES users (id) ON DELETE SET NULL,
    updated_at      timestamptz NOT NULL,
    UNIQUE (environment_id, key),
    CHECK (secret = (value IS NULL)),
    CHECK (secret = (value_sealed IS NOT NULL))
);

-- `environments` : noms d'environnement (`production`, `preview`,
-- `staging`...) ; `all_projects` faux : projets de
-- `global_variable_projects`.
CREATE TABLE global_variables (
    id            uuid        PRIMARY KEY,
    key           text        NOT NULL CHECK (key ~ '^[A-Z_][A-Z0-9_]{0,127}$'),
    secret        boolean     NOT NULL,
    value         text        CHECK (octet_length(value) <= 32768),
    value_sealed  bytea,
    environments  text[]      NOT NULL
                  CHECK (cardinality(environments) BETWEEN 1 AND 16
                         AND array_to_string(environments, ',')
                             ~ '^[a-z0-9][a-z0-9-]{0,62}(,[a-z0-9][a-z0-9-]{0,62})*$'),
    all_projects  boolean     NOT NULL,
    updated_by    uuid        REFERENCES users (id) ON DELETE SET NULL,
    updated_at    timestamptz NOT NULL,
    CHECK (secret = (value IS NULL)),
    CHECK (secret = (value_sealed IS NOT NULL))
);
CREATE INDEX global_variables_key_idx ON global_variables (key);

CREATE TABLE global_variable_projects (
    variable_id  uuid NOT NULL REFERENCES global_variables (id) ON DELETE CASCADE,
    project_id   uuid NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    PRIMARY KEY (variable_id, project_id)
);
CREATE INDEX global_variable_projects_project_idx ON global_variable_projects (project_id);
