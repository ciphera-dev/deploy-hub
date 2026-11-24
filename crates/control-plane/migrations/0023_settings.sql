-- Parametres v0.1 de l'organisation.
-- ip_allowlist : adresses ou blocs CIDR (forme canonique) autorises sur
-- l'API et le panneau ; vide : aucune restriction. Liste videe en dernier
-- recours par `ciphera admin clear-ip-allowlist`.
-- deploy_auto : deploiement automatique a chaque push, valeur donnee aux
-- nouveaux projets Git.

ALTER TABLE instance
    ADD COLUMN ip_allowlist text[]  NOT NULL DEFAULT '{}'
        CHECK (cardinality(ip_allowlist) <= 100),
    ADD COLUMN deploy_auto  boolean NOT NULL DEFAULT true;
