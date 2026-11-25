-- Rotation de la CA de l'instance (`ciphera admin rotate-ca`) : SHA-256 du
-- certificat (DER) de la CA qui a emis le certificat que l'agent presente,
-- note a chaque connexion. `rotate-ca --finish` n'est accepte que lorsque
-- tous les serveurs non revoques presentent un certificat de la CA suivante.
-- NULL : agent pas encore connecte depuis cette version.
ALTER TABLE servers ADD COLUMN cert_issuer_fingerprint bytea;
