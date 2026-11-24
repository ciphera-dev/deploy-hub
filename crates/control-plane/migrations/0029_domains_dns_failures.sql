-- Verdicts DNS negatifs consecutifs d'un domaine : le domaine ne passe en
-- erreur DNS qu'apres plusieurs verdicts negatifs d'affilee (une reponse
-- erronee passagere du resolveur ne coupe plus un domaine).
ALTER TABLE domains ADD COLUMN dns_failures smallint NOT NULL DEFAULT 0;
