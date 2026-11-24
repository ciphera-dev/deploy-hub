-- Anti-rejeu sur l'empreinte du corps : une livraison identique sous un autre
-- identifiant est un doublon. NULL pour les lignes anterieures.
ALTER TABLE github_deliveries ADD COLUMN body_sha256 bytea;
CREATE UNIQUE INDEX github_deliveries_body_sha256_idx
    ON github_deliveries (body_sha256) WHERE body_sha256 IS NOT NULL;
