-- File de taches PostgreSQL (SELECT ... FOR UPDATE SKIP LOCKED). Les
-- charges utiles ne portent que des identifiants, jamais de secret.

CREATE TABLE jobs (
    id            uuid        PRIMARY KEY,
    kind          text        NOT NULL
                  CHECK (kind IN ('deploy', 'cleanup', 'acme_issue', 'metrics_rollup', 'update')),
    payload       jsonb       NOT NULL DEFAULT '{}'::jsonb,
    -- Au plus une tache non terminale par cle (voir l'index partiel).
    dedupe_key    text,
    -- queued -> running -> done | dead ; une echeance de bail ou un echec avec
    -- reprise ramene a queued/running ; done et dead sont terminaux.
    status        text        NOT NULL DEFAULT 'queued'
                  CHECK (status IN ('queued', 'running', 'done', 'dead')),
    -- Plus grand = plus urgent.
    priority      smallint    NOT NULL DEFAULT 0,
    attempts      integer     NOT NULL DEFAULT 0,
    max_attempts  integer     NOT NULL DEFAULT 5 CHECK (max_attempts >= 1),
    run_at        timestamptz NOT NULL DEFAULT now(),
    -- Bail du travailleur courant : passe l'echeance, la tache est reprenable.
    locked_by     text,
    lease_until   timestamptz,
    last_error    text,
    created_at    timestamptz NOT NULL DEFAULT now(),
    finished_at   timestamptz
);

CREATE UNIQUE INDEX jobs_active_dedupe
    ON jobs (dedupe_key)
    WHERE dedupe_key IS NOT NULL AND status IN ('queued', 'running');

CREATE INDEX jobs_queued_order ON jobs (priority DESC, run_at, created_at)
    WHERE status = 'queued';
CREATE INDEX jobs_running_lease ON jobs (lease_until) WHERE status = 'running';
