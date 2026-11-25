-- Retraits de projet a confirmer par les agents : une ligne par projet
-- supprime et par serveur, inseree dans la transaction de la suppression.
-- L'ordre `RemoveProject` part apres la validation et repart a chaque
-- reconnexion de l'agent ; la ligne disparait a son accuse
-- (`ProjectRemoved`). Le projet n'existe plus en base : pas de cle
-- etrangere sur lui.
CREATE TABLE project_removals (
    project_id     uuid        NOT NULL,
    server_id      uuid        NOT NULL REFERENCES servers (id) ON DELETE CASCADE,
    -- Volumes Compose du projet supprimes aussi (choix de l'utilisateur).
    purge_volumes  boolean     NOT NULL,
    requested_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, server_id)
);

CREATE INDEX project_removals_server ON project_removals (server_id);
