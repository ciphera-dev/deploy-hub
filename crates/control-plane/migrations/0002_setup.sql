-- Assistant d'installation : organisation de l'instance et nom du compte.

ALTER TABLE instance
    ADD COLUMN slug      text,
    ADD COLUMN timezone  text NOT NULL DEFAULT 'UTC',
    ADD COLUMN language  text NOT NULL DEFAULT 'fr';

-- Une seule instance par base.
CREATE UNIQUE INDEX instance_singleton ON instance ((true));

ALTER TABLE users
    ADD COLUMN name text NOT NULL DEFAULT '';
