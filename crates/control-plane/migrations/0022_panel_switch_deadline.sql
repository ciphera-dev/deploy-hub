-- Echeance de la bascule vers le domaine du panneau : sans certificat a
-- cette date (emission bloquee), la bascule est abandonnee (erreur visible,
-- route retiree, acces 8443 seul). Remise a zero a chaque enregistrement.

ALTER TABLE domains ADD COLUMN panel_expires_at timestamptz;
