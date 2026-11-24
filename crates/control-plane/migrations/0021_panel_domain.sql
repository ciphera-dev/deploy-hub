-- Domaine du panneau : une ligne de `domains` sans projet (`panel`), pour
-- reprendre la verification DNS, l'emission ACME (`acme_issue`) et l'unicite
-- des hotes de l'instance. Confirme depuis le nouveau domaine, il ferme
-- l'acces 8443 auto-signe (etat garde ici : sur au redemarrage).

ALTER TABLE domains
    ALTER COLUMN project_id DROP NOT NULL,
    ALTER COLUMN environment_id DROP NOT NULL,
    ADD COLUMN panel boolean NOT NULL DEFAULT false,
    ADD COLUMN panel_confirmed_at timestamptz,
    ADD CONSTRAINT domains_panel_shape CHECK (
        CASE WHEN panel
            THEN project_id IS NULL AND environment_id IS NULL AND service_id IS NULL
                 AND NOT generated AND NOT is_primary AND NOT redirect AND path_prefix = '/'
            ELSE project_id IS NOT NULL AND environment_id IS NOT NULL
                 AND panel_confirmed_at IS NULL
        END
    );

-- Un seul domaine du panneau.
CREATE UNIQUE INDEX domains_single_panel ON domains (panel) WHERE panel;
