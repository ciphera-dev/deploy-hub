-- Certificat renouvele d'un agent : emis et envoye sur le lien, accepte en
-- plus de l'actuel jusqu'a la premiere connexion qui le presente, ou il le
-- remplace (cert_fingerprint). L'ancien reste valable si l'agent n'a pas pu
-- le garder.
ALTER TABLE servers ADD COLUMN next_cert_fingerprint text UNIQUE;
