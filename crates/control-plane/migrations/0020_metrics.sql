-- Metriques des agents (toutes les 30 s) : mesures brutes gardees 24 h,
-- agregats horaires gardes 30 jours (tache `metrics_rollup`). Etat du
-- conteneur servi remonte par l'agent.

CREATE TABLE node_metrics_raw (
    server_id        uuid        NOT NULL REFERENCES servers (id) ON DELETE CASCADE,
    at               timestamptz NOT NULL,
    cpu_percent      real        NOT NULL,
    mem_used_bytes   bigint      NOT NULL,
    mem_total_bytes  bigint      NOT NULL,
    disk_used_bytes  bigint      NOT NULL,
    disk_total_bytes bigint      NOT NULL,
    net_rx_bytes     bigint      NOT NULL,
    net_tx_bytes     bigint      NOT NULL
);
CREATE INDEX node_metrics_raw_server_at ON node_metrics_raw (server_id, at DESC);
CREATE INDEX node_metrics_raw_at ON node_metrics_raw (at);

CREATE TABLE node_metrics_hourly (
    server_id        uuid             NOT NULL REFERENCES servers (id) ON DELETE CASCADE,
    hour             timestamptz      NOT NULL,
    cpu_avg          double precision NOT NULL,
    mem_used_avg     bigint           NOT NULL,
    mem_total_bytes  bigint           NOT NULL,
    disk_used_bytes  bigint           NOT NULL,
    disk_total_bytes bigint           NOT NULL,
    samples          integer          NOT NULL,
    PRIMARY KEY (server_id, hour)
);

-- Conteneur servi d'un service (sans cle etrangere : un service supprime
-- garde ses mesures jusqu'a leur expiration).
CREATE TABLE container_metrics_raw (
    server_id        uuid        NOT NULL REFERENCES servers (id) ON DELETE CASCADE,
    deployment_id    uuid        NOT NULL,
    service_id       uuid        NOT NULL,
    at               timestamptz NOT NULL,
    cpu_percent      real        NOT NULL,
    mem_used_bytes   bigint      NOT NULL,
    running          boolean     NOT NULL
);
CREATE INDEX container_metrics_raw_service_at ON container_metrics_raw (service_id, at DESC);
CREATE INDEX container_metrics_raw_at ON container_metrics_raw (at);

CREATE TABLE container_metrics_hourly (
    service_id       uuid             NOT NULL,
    hour             timestamptz      NOT NULL,
    cpu_avg          double precision NOT NULL,
    mem_avg          bigint           NOT NULL,
    running_samples  integer          NOT NULL,
    samples          integer          NOT NULL,
    PRIMARY KEY (service_id, hour)
);

-- Etat du conteneur d'un deploiement remonte par l'agent (`State`,
-- `Metrics`) : faux = arrete, en echec ou disparu, y compris hors du
-- panneau. Sans valeur : rien de remonte encore.
ALTER TABLE deployments
    ADD COLUMN container_running     boolean,
    ADD COLUMN container_reported_at timestamptz;
