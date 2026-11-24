-- Variables non appliquees : suppressions comprises, et heure de lecture des
-- variables par le deploiement (une modification entre la lecture et l'envoi
-- reste en attente).
ALTER TABLE environments ADD COLUMN variables_changed_at timestamptz;
ALTER TABLE deployments ADD COLUMN variables_resolved_at timestamptz;
