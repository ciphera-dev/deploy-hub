-- Notifications : plafond exact par compte et par budget (alertes d'un cote,
-- deploiements reussis de l'autre), reservation avant l'envoi (`pending`) et
-- resume des notifications retenues (`digested`).
--
-- - `budget` : `alert` (echecs, certificats), `success` (deploiement
--   reussi), `digest` (resume, hors plafond) ;
-- - `event` : evenement de la ligne (identifiants seulement), relu par le
--   resume et le balayage horaire ;
-- - `sent_at` : heure de la reservation puis de l'envoi (`pending`, `sent`).

ALTER TABLE notification_deliveries
    DROP CONSTRAINT notification_deliveries_status_check,
    DROP CONSTRAINT notification_deliveries_check,
    ADD COLUMN budget text NOT NULL DEFAULT 'alert'
        CHECK (budget IN ('alert', 'success', 'digest')),
    ADD COLUMN event jsonb;

-- Lignes de 0027 : evenement reconstruit depuis sa cle (le balayage horaire
-- met ensuite les retenues dans un resume) ; reussites dans leur budget.
UPDATE notification_deliveries SET budget = 'success' WHERE event_key LIKE 'deploy:%:success';
UPDATE notification_deliveries SET event = CASE
        WHEN event_key LIKE 'deploy:%:failed' THEN jsonb_build_object(
            'event', 'deploy_failed', 'deployment_id', split_part(event_key, ':', 2))
        WHEN event_key LIKE 'deploy:%:success' THEN jsonb_build_object(
            'event', 'deploy_succeeded', 'deployment_id', split_part(event_key, ':', 2))
        WHEN event_key LIKE 'cert:%:failed:%' THEN jsonb_build_object(
            'event', 'cert_failed', 'domain_id', split_part(event_key, ':', 2),
            'job_id', split_part(event_key, ':', 4), 'renewal', false)
        WHEN event_key LIKE 'cert:%:expiring:%' AND split_part(event_key, ':', 4) ~ '^-?[0-9]+$'
            THEN jsonb_build_object(
            'event', 'cert_expiring', 'domain_id', split_part(event_key, ':', 2),
            'expires_at', split_part(event_key, ':', 4)::bigint)
    END
    WHERE status = 'suppressed';
-- Cle inconnue : rien a resumer.
UPDATE notification_deliveries SET status = 'digested'
    WHERE status = 'suppressed' AND event IS NULL;

ALTER TABLE notification_deliveries
    ADD CONSTRAINT notification_deliveries_status_check
        CHECK (status IN ('pending', 'sent', 'suppressed', 'digested')),
    ADD CONSTRAINT notification_deliveries_sent_check
        CHECK ((status IN ('pending', 'sent')) = (sent_at IS NOT NULL)),
    ADD CONSTRAINT notification_deliveries_event_check
        CHECK (status <> 'suppressed' OR event IS NOT NULL);

CREATE INDEX notification_deliveries_budget
    ON notification_deliveries (user_id, budget, sent_at);
