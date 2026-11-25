-- Nettoyage confirme par l'agent : `cleaned_at` n'est pose qu'a la reception
-- de son accuse (`DeploymentRemoved`). Entre l'ordre de suppression et
-- l'accuse, le deploiement n'est plus proposable au retour arriere : son
-- image peut deja avoir disparu.
ALTER TABLE deployments ADD COLUMN removal_requested_at timestamptz;
