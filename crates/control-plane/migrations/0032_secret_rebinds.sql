-- Marqueurs des rechiffrements au demarrage (`variables::rebind_all`) : une
-- ligne par portee (variables, smtp, github, totp) une fois les valeurs
-- chiffrees avant la liaison a leur ligne rechiffrees. Ensuite, plus aucune
-- valeur non liee n'est acceptee. A retirer en v0.2, avec `rebind_all`.
CREATE TABLE secret_rebinds (
    scope    text        PRIMARY KEY,
    done_at  timestamptz NOT NULL
);

-- Reprend le marqueur des variables (ancienne table `secrets_rebound`).
INSERT INTO secret_rebinds (scope, done_at)
SELECT 'variables', done_at FROM secrets_rebound;
