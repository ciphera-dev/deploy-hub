-- Origine d'un lien de reinitialisation : demande anonyme (orgot), lien
-- genere par un administrateur (dmin) ou par la CLI root (cli). Une
-- demande anonyme ne supprime que les liens orgot du compte.
ALTER TABLE password_resets
    ADD COLUMN origin text NOT NULL DEFAULT 'forgot' CHECK (origin IN ('forgot', 'admin', 'cli'));
