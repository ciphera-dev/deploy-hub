//! Domaine du panneau : une ligne de `domains` marquee `panel` (sans projet),
//! verifiee et emise comme les domaines des projets (`acme_issue`), servie par
//! le proxy du serveur local (route vers l'ecoute HTTP locale du panneau).
//!
//! Etats : `issuing` (DNS verifie, certificat demande), `awaiting_confirmation`
//! (certificat actif : le panneau repond sur le domaine), `active` (confirme
//! depuis le domaine : l'acces 8443 auto-signe se ferme), `error` (DNS ou
//! certificat). Avant la confirmation, l'acces 8443 reste ouvert et le retrait
//! (`DELETE`) sert d'annulation ; une bascule restee sans certificat 24 h est
//! abandonnee ([`expire`]). La route du proxy n'a d'amont qu'une fois le
//! certificat actif : jamais de panneau en HTTP clair. L'etat est en base : un redemarrage le
//! reprend ([`PanelSwitch::load`]). Dernier recours si le domaine confirme ne
//! repond plus : `ciphera admin reset-panel-domain` (rouvre 8443 au
//! redemarrage du service).

use std::sync::{Arc, RwLock};

use api_types::panel_domain::{PanelDomain, PanelDomainStatus};
use shared::config::Config;
use sqlx::{PgConnection, PgPool};
use tokio::sync::watch;
use uuid::Uuid;

/// Domaine du panneau tel qu'enregistre.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Panel {
    pub id: Uuid,
    pub host: String,
    pub confirmed: bool,
}

struct Inner {
    current: RwLock<Option<Panel>>,
    /// Vrai tant que l'acces 8443 doit rester ouvert.
    open: watch::Sender<bool>,
}

/// Etat en memoire du domaine du panneau, partage par les handlers et la
/// boucle qui ouvre ou ferme l'ecoute 8443.
#[derive(Clone)]
pub struct PanelSwitch(Arc<Inner>);

impl Default for PanelSwitch {
    fn default() -> Self {
        Self::with(None)
    }
}

impl std::fmt::Debug for PanelSwitch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("PanelSwitch").field(&self.current()).finish()
    }
}

impl PanelSwitch {
    fn with(panel: Option<Panel>) -> Self {
        let open = !panel.as_ref().is_some_and(|p| p.confirmed);
        let (tx, _) = watch::channel(open);
        Self(Arc::new(Inner {
            current: RwLock::new(panel),
            open: tx,
        }))
    }

    /// Etat enregistre en base (demarrage).
    pub async fn load(db: &PgPool) -> sqlx::Result<Self> {
        let mut conn = db.acquire().await?;
        Ok(Self::with(read(&mut conn).await?))
    }

    /// Remplace l'etat (apres une ecriture validee en base).
    pub fn set(&self, panel: Option<Panel>) {
        let open = !panel.as_ref().is_some_and(|p| p.confirmed);
        *self.0.current.write().unwrap_or_else(|e| e.into_inner()) = panel;
        self.0.open.send_if_modified(|v| {
            let changed = *v != open;
            *v = open;
            changed
        });
    }

    pub fn current(&self) -> Option<Panel> {
        self.0
            .current
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// L'acces 8443 doit-il etre ouvert (aucun domaine confirme) ?
    pub fn listens_8443(&self) -> bool {
        *self.0.open.borrow()
    }

    /// Suit l'ouverture de l'acces 8443.
    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.0.open.subscribe()
    }

    /// Origine `https://<domaine>` du panneau, des qu'un domaine est
    /// enregistre (admise pour les WebSocket avant meme la confirmation).
    pub fn origin(&self) -> Option<String> {
        self.current().map(|p| format!("https://{}", p.host))
    }

    /// Origine publique : le domaine confirme, sinon `CIPHERA_PUBLIC_URL`.
    pub fn public_url(&self, cfg: &Config) -> Option<String> {
        match self.current() {
            Some(p) if p.confirmed => Some(format!("https://{}", p.host)),
            _ => cfg.public_url.clone(),
        }
    }
}

async fn read(conn: &mut PgConnection) -> sqlx::Result<Option<Panel>> {
    let row: Option<(Uuid, String, bool)> =
        sqlx::query_as("SELECT id, host, panel_confirmed_at IS NOT NULL FROM domains WHERE panel")
            .fetch_optional(conn)
            .await?;
    Ok(row.map(|(id, host, confirmed)| Panel {
        id,
        host,
        confirmed,
    }))
}

#[derive(sqlx::FromRow)]
struct ViewRow {
    host: String,
    confirmed: bool,
    dns_status: String,
    dns_error: Option<String>,
    cert_status: String,
    cert_error: Option<String>,
}

/// Vue de l'API (`GET /api/settings/panel-domain`). Une chaine d'emission
/// arretee (tentatives epuisees) apparait en erreur, comme pour les domaines
/// des projets : jamais d'emission « en cours » sans fin.
pub async fn view(conn: &mut PgConnection) -> sqlx::Result<PanelDomain> {
    let row: Option<ViewRow> = sqlx::query_as(
        "SELECT d.host, d.panel_confirmed_at IS NOT NULL AS confirmed, d.dns_status, d.dns_error, \
         CASE WHEN s.stalled THEN 'error' ELSE d.cert_status END AS cert_status, \
         CASE WHEN s.stalled THEN 'Émission du certificat interrompue : enregistrez à nouveau le domaine pour la relancer.' \
           ELSE d.cert_error END AS cert_error \
         FROM domains d CROSS JOIN LATERAL (SELECT d.dns_status = 'ok' AND d.cert_status <> 'active' \
           AND NOT EXISTS (SELECT 1 FROM jobs j WHERE j.dedupe_key = 'acme:' || d.id::text \
                           AND j.status IN ('queued', 'running')) AS stalled) s \
         WHERE d.panel",
    )
    .fetch_optional(conn)
    .await?;
    let Some(ViewRow {
        host,
        confirmed,
        dns_status: dns,
        dns_error,
        cert_status: cert,
        cert_error,
    }) = row
    else {
        return Ok(PanelDomain {
            domain: None,
            status: PanelDomainStatus::None,
            error: None,
        });
    };
    let (status, error) = status_of(confirmed, &dns, dns_error, &cert, cert_error);
    Ok(PanelDomain {
        domain: Some(host),
        status,
        error,
    })
}

/// Etat affiche : confirme (avec sa derniere erreur eventuelle), sinon certificat actif (a confirmer), erreur
/// DNS ou certificat, sinon emission en cours.
pub fn status_of(
    confirmed: bool,
    dns: &str,
    dns_error: Option<String>,
    cert: &str,
    cert_error: Option<String>,
) -> (PanelDomainStatus, Option<String>) {
    if confirmed {
        // Confirme : toujours actif, mais la derniere erreur (verification
        // ou renouvellement en echec) reste visible.
        return (PanelDomainStatus::Active, cert_error.or(dns_error));
    }
    if dns == "error" {
        return (PanelDomainStatus::Error, dns_error);
    }
    match cert {
        "active" => (PanelDomainStatus::AwaitingConfirmation, None),
        "error" => (PanelDomainStatus::Error, cert_error),
        _ => (PanelDomainStatus::Issuing, None),
    }
}

/// Evenement d'audit du domaine du panneau.
pub async fn audit(
    conn: &mut PgConnection,
    actor: Option<Uuid>,
    action: &str,
    detail: serde_json::Value,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO audit_events (id, actor_id, action, target, detail) \
         VALUES ($1, $2, $3, 'panel_domain', $4)",
    )
    .bind(Uuid::new_v4())
    .bind(actor)
    .bind(action)
    .bind(detail)
    .execute(conn)
    .await
    .map(|_| ())
}

/// Duree maximale d'une bascule sans certificat (emission en cours ou en
/// attente) : au-dela, elle est abandonnee.
pub const SWITCH_TTL_SECS: f64 = 24.0 * 3600.0;

/// Abandonne la bascule vers le domaine du panneau si elle n'a pas obtenu
/// son certificat avant son echeance (`panel_expires_at` <= `now`, en
/// secondes Unix) : erreur visible (DNS en erreur, donc route retiree et
/// emission arretee), taches d'emission en attente closes. L'acces 8443,
/// jamais ferme avant la confirmation, reste le seul acces. Renvoie l'hote
/// abandonne ; un nouvel enregistrement relance la bascule.
pub async fn expire(conn: &mut PgConnection, now: f64) -> sqlx::Result<Option<String>> {
    let expired: Option<(Uuid, String)> = sqlx::query_as(
        "UPDATE domains SET dns_status = 'error', \
         dns_error = 'Bascule abandonnée : le certificat de ' || host || ' n''a pas été obtenu en 24 h. \
Le panneau reste servi en 8443 seulement ; enregistrez à nouveau le domaine pour réessayer.', \
         cert_status = 'none', cert_error = NULL, panel_expires_at = NULL, updated_at = now() \
         WHERE panel AND panel_confirmed_at IS NULL AND cert_status <> 'active' \
           AND panel_expires_at <= to_timestamp($1::float8) \
         RETURNING id, host",
    )
    .bind(now)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((id, host)) = expired else {
        return Ok(None);
    };
    sqlx::query(
        "UPDATE jobs SET status = 'dead', last_error = 'bascule du domaine du panneau abandonnée', \
         finished_at = now() WHERE dedupe_key = 'acme:' || $1::text AND status = 'queued'",
    )
    .bind(id)
    .execute(&mut *conn)
    .await?;
    tracing::warn!(%host, "bascule du domaine du panneau abandonnee : certificat non obtenu");
    Ok(Some(host))
}

/// Abandon des bascules echues, verifie chaque minute : la table des routes
/// est renvoyee aux agents des qu'une bascule est abandonnee.
pub async fn run_expiry(state: crate::state::AppState, mut stop: watch::Receiver<bool>) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            _ = stop.changed() => return,
        }
        if *stop.borrow() {
            return;
        }
        sweep(&state).await;
    }
}

/// Une verification : abandon eventuel, puis table des routes renvoyee.
pub async fn sweep(state: &crate::state::AppState) {
    let expired = match state.db.acquire().await {
        Ok(mut conn) => expire(&mut conn, state.clock.unix_secs()).await,
        Err(e) => Err(e),
    };
    match expired {
        Ok(Some(_)) => super::push_routes(state).await,
        Ok(None) => {}
        Err(e) => tracing::error!(error = %e, "echeance du domaine du panneau illisible"),
    }
}

/// Retire le domaine du panneau (annulation, retrait ou recours en ligne de
/// commande). Renvoie l'hote retire.
pub async fn remove(conn: &mut PgConnection) -> sqlx::Result<Option<String>> {
    sqlx::query_scalar("DELETE FROM domains WHERE panel RETURNING host")
        .fetch_optional(conn)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_follows_confirmation_dns_and_certificate() {
        let s = |c, d, ce| status_of(c, d, Some("dns".into()), ce, Some("cert".into()));
        assert_eq!(
            s(true, "error", "error"),
            (PanelDomainStatus::Active, Some("cert".into()))
        );
        assert_eq!(
            status_of(true, "ok", None, "active", None),
            (PanelDomainStatus::Active, None)
        );
        assert_eq!(
            s(false, "error", "active"),
            (PanelDomainStatus::Error, Some("dns".into()))
        );
        assert_eq!(
            s(false, "ok", "active").0,
            PanelDomainStatus::AwaitingConfirmation
        );
        assert_eq!(
            s(false, "ok", "error"),
            (PanelDomainStatus::Error, Some("cert".into()))
        );
        assert_eq!(s(false, "ok", "issuing").0, PanelDomainStatus::Issuing);
        assert_eq!(s(false, "ok", "none").0, PanelDomainStatus::Issuing);
    }

    #[test]
    fn port_8443_closes_only_once_confirmed() {
        let cfg = Config::from_lookup(|n| match n {
            "CIPHERA_DATABASE_URL" => Some("postgres://u:p@h/d".into()),
            "CIPHERA_PUBLIC_URL" => Some("https://203.0.113.7:8443".into()),
            _ => None,
        })
        .unwrap();
        let switch = PanelSwitch::default();
        let mut watch = switch.subscribe();
        assert!(switch.listens_8443());
        assert_eq!(switch.origin(), None);
        let panel = Panel {
            id: Uuid::new_v4(),
            host: "panel.example.org".into(),
            confirmed: false,
        };
        switch.set(Some(panel.clone()));
        assert!(switch.listens_8443());
        assert!(!watch.has_changed().unwrap());
        assert_eq!(
            switch.origin().as_deref(),
            Some("https://panel.example.org")
        );
        assert_eq!(
            switch.public_url(&cfg).as_deref(),
            Some("https://203.0.113.7:8443")
        );
        switch.set(Some(Panel {
            confirmed: true,
            ..panel
        }));
        assert!(watch.has_changed().unwrap());
        assert!(!*watch.borrow_and_update());
        assert_eq!(
            switch.public_url(&cfg).as_deref(),
            Some("https://panel.example.org")
        );
        switch.set(None);
        assert!(switch.listens_8443());
    }
}
