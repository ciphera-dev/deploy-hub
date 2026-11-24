-- Origine d'un lien de reinitialisation : demande anonyme (`forgot`), lien
-- genere par un administrateur (`admin`) ou par la CLI root (`cli`). Une
-- demande anonyme ne supprime que les liens `forgot` du compte.
ALTER TABLE password_resets
    ADD COLUMN origin text NOT NULL DEFAULT 'forgot' CHECK (origin IN ('forgot', 'admin', 'cli'));
