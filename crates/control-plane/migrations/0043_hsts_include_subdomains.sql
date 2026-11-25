-- Option HSTS du proprietaire : `includeSubDomains` quand le domaine du
-- panneau est lui-meme un domaine enregistrable (`exemple.com`). Desactivee
-- par defaut : elle imposerait HTTPS a tous les sous-domaines de la zone.
-- Un domaine du panneau place sous son domaine enregistrable
-- (`panel.exemple.com`) recoit `includeSubDomains` sans cette option.
ALTER TABLE instance ADD COLUMN hsts_include_subdomains boolean NOT NULL DEFAULT false;
