-- Notifications e-mail (Profil › Notifications) : tache `notify` mise en
-- file dans la transaction qui change le statut (deploiement, certificat),
-- et envois par evenement et par destinataire.

ALTER TABLE jobs DROP CONSTRAINT jobs_kind_check;
ALTER TABLE jobs ADD CONSTRAINT jobs_kind_check
    CHECK (kind IN ('deploy', 'cleanup', 'acme_issue', 'metrics_rollup', 'update', 'notify'));

-- Une ligne par evenement et par destinataire : une reprise de la tache
-- n'envoie jamais deux fois. `suppressed` : retenu par le plafond horaire
-- du compte (resume dans l'e-mail suivant). Ni adresse ni contenu ici.
CREATE TABLE notification_deliveries (
    event_key   text        NOT NULL,
    user_id     uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    status      text        NOT NULL CHECK (status IN ('sent', 'suppressed')),
    created_at  timestamptz NOT NULL,
    sent_at     timestamptz,
    PRIMARY KEY (event_key, user_id),
    CHECK ((status = 'sent') = (sent_at IS NOT NULL))
);
CREATE INDEX notification_deliveries_user ON notification_deliveries (user_id, created_at);
CREATE INDEX notification_deliveries_created ON notification_deliveries (created_at);
