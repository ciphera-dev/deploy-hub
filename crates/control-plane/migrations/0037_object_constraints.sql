-- Contraintes d'integrite manquantes.
--
-- 1. `deploy_hooks.token_hash` : l'URL secrete d'un deploy hook retrouve son
--    hook par le SHA-256 du secret ; deux lignes au meme hash rendraient la
--    resolution ambigue. Les doublons eventuels sont purges (le plus ancien
--    reste), puis le hash devient unique.
DELETE FROM deploy_hooks d
USING (
    SELECT id,
           row_number() OVER (PARTITION BY token_hash ORDER BY created_at, id) AS rank
    FROM deploy_hooks
) dup
WHERE d.id = dup.id AND dup.rank > 1;
ALTER TABLE deploy_hooks ADD CONSTRAINT deploy_hooks_token_hash_key UNIQUE (token_hash);

-- 2. `mfa_attempts.session_id` : lie aux sessions. Les tentatives comptent
--    aussi par compte et par jour (plafond TOTP journalier, reinitialise
--    seulement par un nouveau mot de passe) : la fin de la session
--    d'enrolement ou de connexion ne doit pas les effacer. La colonne devient
--    donc nullable et la cle etrangere fait `ON DELETE SET NULL` (et non
--    `CASCADE`) ; les tentatives d'une session deja disparue sont detachees de
--    la meme facon.
ALTER TABLE mfa_attempts ALTER COLUMN session_id DROP NOT NULL;
UPDATE mfa_attempts m SET session_id = NULL
WHERE session_id IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM sessions s WHERE s.id = m.session_id);
ALTER TABLE mfa_attempts
    ADD CONSTRAINT mfa_attempts_session_id_fkey
    FOREIGN KEY (session_id) REFERENCES sessions (id) ON DELETE SET NULL;
