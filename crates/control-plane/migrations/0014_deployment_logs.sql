-- Journaux de build et de deploiement (lignes `LogLine` de l'agent dont la
-- cible est un deploiement), servis par `GET /api/deployments/{id}/logs.txt`
-- et en historique du flux en direct. Les valeurs secretes du projet sont
-- masquees avant l'insertion (et de nouveau a la lecture).

-- Lignes enregistrees (plafond : `store::MAX_LINES_PER_DEPLOYMENT`, les
-- suivantes sont ignorees).
ALTER TABLE deployments ADD COLUMN log_lines integer NOT NULL DEFAULT 0;

-- Supprimees avec le deploiement (projet supprime) et par le nettoyage de
-- la retention (`retention::cleanup`).
CREATE TABLE deployment_logs (
    deployment_id  uuid    NOT NULL REFERENCES deployments (id) ON DELETE CASCADE,
    seq            bigint  GENERATED ALWAYS AS IDENTITY,
    -- Millisecondes depuis l'epoque Unix (horodatage de l'agent).
    ts_ms          bigint  NOT NULL,
    stream         text    NOT NULL CHECK (stream IN ('stdout', 'stderr')),
    line           text    NOT NULL,
    PRIMARY KEY (deployment_id, seq)
);
