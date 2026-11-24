-- Reglages de securite par service : utilisateur du conteneur (`uid[:gid]`
-- numerique ; sans valeur, l'utilisateur de l'image) et racine en lecture
-- seule. `runs_as_root` : utilisateur effectif signale par l'agent pour le
-- conteneur du deploiement (faux tant que l'agent ne l'a pas dit).
ALTER TABLE services
    ADD COLUMN run_as text
        CONSTRAINT services_run_as_format CHECK (run_as ~ '^[0-9]{1,10}(:[0-9]{1,10})?$'),
    ADD COLUMN read_only_rootfs boolean NOT NULL DEFAULT false;

ALTER TABLE deployments
    ADD COLUMN runs_as_root boolean NOT NULL DEFAULT false;
