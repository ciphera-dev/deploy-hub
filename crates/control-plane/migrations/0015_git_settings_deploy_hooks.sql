-- Reglages Git du projet (onglet Git) et deploy hooks.

-- Reglages lus par le webhook GitHub entrant :
-- - `root_directory` : dossier de l'application dans le depot ('' = racine) ;
-- - `watch_paths` : motifs glob ; un push dont aucun fichier modifie ne
--   correspond ne deploie rien (liste vide = toujours deployer) ;
-- - `auto_deploy` : un push sur la branche deploie ;
-- - `skip_ci` : un commit de tete marque `[skip ci]` ou `[ciphera skip]`
--   ne deploie pas ;
-- - `ignore_bots` : un push d'un compte bot (`*[bot]`) ne deploie pas.
ALTER TABLE git_sources
    ADD COLUMN root_directory text    NOT NULL DEFAULT ''
        CHECK (root_directory = '' OR root_directory ~ '^[A-Za-z0-9._][A-Za-z0-9._/-]{0,254}$'),
    ADD COLUMN watch_paths    text[]  NOT NULL DEFAULT '{}'
        CHECK (cardinality(watch_paths) <= 20),
    ADD COLUMN auto_deploy    boolean NOT NULL DEFAULT true,
    ADD COLUMN skip_ci        boolean NOT NULL DEFAULT true,
    ADD COLUMN ignore_bots    boolean NOT NULL DEFAULT false;

-- URL secrete qui declenche un deploiement de la production. Seul le
-- SHA-256 du secret (`dh_` + 32 octets aleatoires) est stocke : il n'est
-- montre qu'une fois, a la creation.
CREATE TABLE deploy_hooks (
    id            uuid        PRIMARY KEY,
    project_id    uuid        NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    name          text        NOT NULL CHECK (char_length(name) BETWEEN 1 AND 64),
    token_hash    bytea       NOT NULL CHECK (octet_length(token_hash) = 32),
    created_by    uuid        REFERENCES users (id) ON DELETE SET NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    last_used_at  timestamptz
);
CREATE INDEX deploy_hooks_project_idx ON deploy_hooks (project_id, created_at);

-- Nouveau declencheur : deploy hook.
ALTER TABLE deployments DROP CONSTRAINT deployments_trigger_check;
ALTER TABLE deployments ADD CONSTRAINT deployments_trigger_check
    CHECK (trigger IN ('push', 'manual', 'rollback', 'hook'));
