-- Secrets des variables lies a leur ligne (`SecretBox::seal_bound`) :
-- une ligne ici quand les valeurs chiffrees avant la liaison ont ete
-- rechiffrees (`variables::rebind_legacy`, au demarrage). Ensuite, plus
-- aucune valeur non liee n'est acceptee.
CREATE TABLE secrets_rebound (
    singleton  boolean     PRIMARY KEY DEFAULT true CHECK (singleton),
    done_at    timestamptz NOT NULL
);
