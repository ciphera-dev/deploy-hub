-- Deploiements : file par environnement, verrou, etapes, rollback, retention.
-- Les horodatages viennent de l'horloge de l'application (`Clock`).

CREATE TABLE deployments (
    id                uuid        PRIMARY KEY,
    -- Numero lisible (`#42`), croissant sur l'instance.
    number            bigint      GENERATED ALWAYS AS IDENTITY UNIQUE,
    project_id        uuid        NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    environment_id    uuid        NOT NULL REFERENCES environments (id) ON DELETE CASCADE,
    -- Serveur qui execute le deploiement (choisi au demarrage ; un rollback
    -- reste sur le serveur de l'image conservee).
    server_id         uuid        REFERENCES servers (id) ON DELETE SET NULL,
    -- queued -> running -> success | failed | cancelled (terminaux).
    status            text        NOT NULL DEFAULT 'queued'
                      CHECK (status IN ('queued', 'running', 'success', 'failed', 'cancelled')),
    trigger           text        NOT NULL CHECK (trigger IN ('push', 'manual', 'rollback')),
    -- Commit construit : connu des la mise en file (push, rollback) ou
    -- resolu au lancement du build (manuel).
    commit_sha        text        CHECK (commit_sha ~ '^([0-9a-f]{40}|[0-9a-f]{64})$'),
    branch            text,
    -- Rollback : deploiement choisi, et deploiement qui a construit l'image
    -- reutilisee (aucun build).
    rollback_of       uuid        REFERENCES deployments (id) ON DELETE SET NULL,
    image_from        uuid        REFERENCES deployments (id) ON DELETE SET NULL,
    -- Image principale `ciphera/<projet>-<env>:<deploiement>`.
    image             text,
    -- Projet Compose : service expose au moment du build.
    compose_service   text,
    -- Derniere etape signalee par l'agent.
    stage             text        CHECK (stage IN ('clone', 'build', 'image', 'start', 'healthcheck', 'switch')),
    error             text,
    -- Annulation demandee pendant l'execution (`Stop` envoye a l'agent).
    cancel_requested  boolean     NOT NULL DEFAULT false,
    -- Echec presume par le plan de controle (10 min sans nouvelles), sans
    -- verdict de l'agent : l'issue reelle rapportee a la reconnexion prime.
    presumed_failed   boolean     NOT NULL DEFAULT false,
    -- Tache de webhook qui l'a cree (une tache rejouee ne cree rien).
    job_id            uuid        UNIQUE,
    created_by        uuid        REFERENCES users (id) ON DELETE SET NULL,
    created_at        timestamptz NOT NULL,
    started_at        timestamptz,
    finished_at       timestamptz,
    -- Derniere nouvelle de l'agent (progression, journal, etat).
    last_event_at     timestamptz,
    build_sent_at     timestamptz,
    image_built_at    timestamptz,
    deploy_sent_at    timestamptz,
    -- Conteneurs et images supprimes sur l'agent (retention, projet supprime).
    cleaned_at        timestamptz
);

-- Un seul deploiement en cours par environnement, garanti par la base.
CREATE UNIQUE INDEX deployments_one_running_per_env
    ON deployments (environment_id) WHERE status = 'running';
CREATE INDEX deployments_env_queue
    ON deployments (environment_id, created_at, number) WHERE status = 'queued';
CREATE INDEX deployments_running_events
    ON deployments (last_event_at) WHERE status = 'running';
CREATE INDEX deployments_project_created ON deployments (project_id, created_at DESC);
CREATE INDEX deployments_created ON deployments (created_at DESC, number DESC);
CREATE INDEX deployments_env_finished ON deployments (environment_id, finished_at DESC);

-- Etapes de chaque deploiement (frise du panneau).
CREATE TABLE deployment_stages (
    deployment_id  uuid        NOT NULL REFERENCES deployments (id) ON DELETE CASCADE,
    stage          text        NOT NULL
                   CHECK (stage IN ('clone', 'build', 'image', 'start', 'healthcheck', 'switch')),
    status         text        NOT NULL CHECK (status IN ('running', 'succeeded', 'failed', 'cancelled')),
    message        text,
    started_at     timestamptz NOT NULL,
    finished_at    timestamptz,
    PRIMARY KEY (deployment_id, stage)
);

-- Reglages du service lus au deploiement (sans valeur : pas de port expose,
-- healthcheck `/`).
ALTER TABLE services
    ADD COLUMN internal_port integer CHECK (internal_port BETWEEN 1 AND 65535),
    ADD COLUMN healthcheck_path text
        CHECK (healthcheck_path ~ '^/[!-~]{0,1023}$');
