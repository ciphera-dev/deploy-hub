-- Expiration facultative des deploy hooks. Les hooks existants gardent
-- `NULL` : ils n'expirent pas (une date ne leur est donnee que par
-- modification explicite) ; les nouveaux recoivent 90 jours par defaut,
-- fixes par le plan de controle a la creation.
ALTER TABLE deploy_hooks ADD COLUMN expires_at timestamptz;
