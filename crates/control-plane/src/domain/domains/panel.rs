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

use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// Option HSTS « Inclure les sous-domaines » (`instance`), lue a chaque
    /// reponse.
    hsts_subdomains: AtomicBool,
}

/// Etat en memoire du domaine du panneau, partage par les handlers et la
/// boucle qui ouvre ou ferme l'ecoute 8443.
#[derive(Clone)]
pub struct PanelSwitch(Arc<Inner>);

impl Default for PanelSwitch {
    fn default() -> Self {
        Self::with(None, false)
    }
}

impl std::fmt::Debug for PanelSwitch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("PanelSwitch").field(&self.current()).finish()
    }
}

impl PanelSwitch {
    fn with(panel: Option<Panel>, hsts_subdomains: bool) -> Self {
        let open = !panel.as_ref().is_some_and(|p| p.confirmed);
        let (tx, _) = watch::channel(open);
        Self(Arc::new(Inner {
            current: RwLock::new(panel),
            open: tx,
            hsts_subdomains: AtomicBool::new(hsts_subdomains),
        }))
    }

    /// Etat enregistre en base (demarrage).
    pub async fn load(db: &PgPool) -> sqlx::Result<Self> {
        let mut conn = db.acquire().await?;
        let panel = read(&mut conn).await?;
        let hsts = read_hsts_option(&mut conn).await?;
        Ok(Self::with(panel, hsts))
    }

    /// Option HSTS « Inclure les sous-domaines » du proprietaire.
    pub fn hsts_include_subdomains(&self) -> bool {
        self.0.hsts_subdomains.load(Ordering::Acquire)
    }

    /// Remplace l'option HSTS en memoire. Son ecriture passe par
    /// [`PanelSwitch::commit_hsts_with`] : la memoire suit la base.
    pub fn set_hsts_include_subdomains(&self, on: bool) {
        self.0.hsts_subdomains.store(on, Ordering::Release);
    }

    /// Comme [`PanelSwitch::commit_with`], pour l'option HSTS : posee avant
    /// `commit` (verrou d'ecriture tenu), remise si `commit` echoue, le tout
    /// detache de l'appelant.
    pub async fn commit_hsts_with<E, F>(&self, on: bool, commit: F) -> Result<(), E>
    where
        E: Send + 'static,
        F: std::future::Future<Output = Result<(), E>> + Send + 'static,
    {
        let switch = self.clone();
        let task = tokio::spawn(async move {
            let previous = switch.hsts_include_subdomains();
            switch.set_hsts_include_subdomains(on);
            let done = commit.await;
            if done.is_err() {
                switch.set_hsts_include_subdomains(previous);
            }
            done
        });
        match task.await {
            Ok(done) => done,
            Err(e) => std::panic::resume_unwind(e.into_panic()),
        }
    }

    /// Remplace l'etat en memoire. Les ecritures du domaine passent par
    /// [`PanelSwitch::commit_with`] : l'etat suit la base.
    pub fn set(&self, panel: Option<Panel>) {
        let open = !panel.as_ref().is_some_and(|p| p.confirmed);
        *self.0.current.write().unwrap_or_else(|e| e.into_inner()) = panel;
        self.0.open.send_if_modified(|v| {
            let changed = *v != open;
            *v = open;
            changed
        });
    }

    /// Pose `next` (et, si `clear_hsts`, l'option HSTS a faux, comme
    /// [`remove`] ou [`clear_hsts_option`] dans la transaction) puis attend
    /// `commit` (validation de la transaction, verrou consultatif encore
    /// tenu) ; si `commit` echoue, l'etat precedent est remis. Le tout tourne dans une tache detachee : si l'appelant est
    /// abandonne (client deconnecte) pendant le commit, la sequence va
    /// quand meme a son terme et l'etat ne diverge pas de la base.
    pub async fn commit_with<E, F>(
        &self,
        next: Option<Panel>,
        clear_hsts: bool,
        commit: F,
    ) -> Result<(), E>
    where
        E: Send + 'static,
        F: std::future::Future<Output = Result<(), E>> + Send + 'static,
    {
        let switch = self.clone();
        let task = tokio::spawn(async move {
            let previous = switch.current();
            let previous_hsts = switch.hsts_include_subdomains();
            switch.set(next);
            if clear_hsts {
                switch.set_hsts_include_subdomains(false);
            }
            let done = commit.await;
            if done.is_err() {
                switch.set(previous);
                switch.set_hsts_include_subdomains(previous_hsts);
            }
            done
        });
        match task.await {
            Ok(done) => done,
            Err(e) => std::panic::resume_unwind(e.into_panic()),
        }
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

/// Option HSTS enregistree (faux avant la configuration de l'instance).
pub async fn read_hsts_option(conn: &mut PgConnection) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT coalesce((SELECT hsts_include_subdomains FROM instance LIMIT 1), false)",
    )
    .fetch_one(conn)
    .await
}

/// Place d'un hote par rapport a son domaine enregistrable (liste des
/// suffixes publics, embarquee).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Registrable {
    /// Strictement sous son domaine enregistrable (`panel.exemple.com`).
    Below,
    /// Domaine enregistrable lui-meme (`exemple.com`, `exemple.co.uk`).
    Apex,
    /// Ni l'un ni l'autre : suffixe public, nom d'un seul label, adresse IP.
    Neither,
}

/// Place de `host` (deja normalise : minuscules, sans port ni point final)
/// par rapport a son domaine enregistrable.
pub fn registrable(host: &str) -> Registrable {
    if host.parse::<IpAddr>().is_ok() {
        return Registrable::Neither;
    }
    match psl::domain_str(host) {
        Some(domain) if domain.len() == host.len() => Registrable::Apex,
        Some(_) => Registrable::Below,
        None => Registrable::Neither,
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
    let hsts_include_subdomains = read_hsts_option(&mut *conn).await?;
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
            apex: false,
            hsts_include_subdomains,
        });
    };
    let (status, error) = status_of(confirmed, &dns, dns_error, &cert, cert_error);
    Ok(PanelDomain {
        apex: registrable(&host) == Registrable::Apex,
        domain: Some(host),
        status,
        error,
        hsts_include_subdomains,
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

/// Retire le domaine du panneau (annulation, remplacement, retrait ou
/// recours en ligne de commande) et remet l'option HSTS a zero : l'accord
/// donne pour un domaine ne vaut jamais pour le suivant. Renvoie l'hote
/// retire.
pub async fn remove(conn: &mut PgConnection) -> sqlx::Result<Option<String>> {
    let host = sqlx::query_scalar("DELETE FROM domains WHERE panel RETURNING host")
        .fetch_optional(&mut *conn)
        .await?;
    clear_hsts_option(conn).await?;
    Ok(host)
}

/// Option HSTS « Inclure les sous-domaines » remise a zero (nouveau domaine
/// du panneau ou retrait), dans la transaction de l'appelant.
pub async fn clear_hsts_option(conn: &mut PgConnection) -> sqlx::Result<()> {
    sqlx::query("UPDATE instance SET hsts_include_subdomains = false WHERE hsts_include_subdomains")
        .execute(conn)
        .await
        .map(|_| ())
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

    fn panel(confirmed: bool) -> Panel {
        Panel {
            id: Uuid::new_v4(),
            host: "panel.example.org".into(),
            confirmed,
        }
    }

    #[tokio::test]
    async fn commit_failure_restores_the_previous_state() {
        let switch = PanelSwitch::default();
        let r: Result<(), &str> = switch
            .commit_with(Some(panel(true)), false, async { Err("commit") })
            .await;
        assert_eq!(r, Err("commit"));
        assert_eq!(switch.current(), None);
        assert!(switch.listens_8443());
        let ok: Result<(), &str> = switch
            .commit_with(Some(panel(true)), false, async { Ok(()) })
            .await;
        assert_eq!(ok, Ok(()));
        assert!(!switch.listens_8443());
    }

    /// L'appelant est abandonne pendant le commit : la sequence va a son
    /// terme (etat pose si le commit reussit, restaure s'il echoue).
    #[tokio::test]
    async fn dropping_the_caller_mid_commit_never_leaves_a_wrong_state() {
        for commit_ok in [true, false] {
            let switch = PanelSwitch::default();
            let release = Arc::new(tokio::sync::Notify::new());
            let started = Arc::new(tokio::sync::Notify::new());
            let (r, s) = (release.clone(), started.clone());
            let s2 = switch.clone();
            let caller = tokio::spawn(async move {
                s2.commit_with(Some(panel(true)), false, async move {
                    s.notify_one();
                    r.notified().await;
                    if commit_ok { Ok(()) } else { Err(()) }
                })
                .await
            });
            started.notified().await;
            // Etat pose avant le commit ; le client se deconnecte.
            assert!(!switch.listens_8443());
            caller.abort();
            let _ = caller.await;
            release.notify_one();
            for _ in 0..200 {
                if switch.listens_8443() != commit_ok {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            assert_eq!(!switch.listens_8443(), commit_ok, "commit_ok={commit_ok}");
        }
    }

    #[test]
    fn registrable_domain_follows_the_public_suffix_list() {
        for (host, want) in [
            ("panel.example.com", Registrable::Below),
            ("a.b.example.com", Registrable::Below),
            ("panel.example.co.uk", Registrable::Below),
            ("example.com", Registrable::Apex),
            ("example.co.uk", Registrable::Apex),
            // Suffixe prive de la liste : chaque compte est enregistrable.
            ("user.github.io", Registrable::Apex),
            ("panel.user.github.io", Registrable::Below),
            ("co.uk", Registrable::Neither),
            ("github.io", Registrable::Neither),
            ("localhost", Registrable::Neither),
            ("203.0.113.7", Registrable::Neither),
            ("2001:db8::1", Registrable::Neither),
        ] {
            assert_eq!(registrable(host), want, "{host}");
        }
    }

    #[tokio::test]
    async fn hsts_option_commit_failure_restores_it() {
        let switch = PanelSwitch::default();
        assert!(!switch.hsts_include_subdomains());
        let r: Result<(), &str> = switch.commit_hsts_with(true, async { Err("commit") }).await;
        assert_eq!(r, Err("commit"));
        assert!(!switch.hsts_include_subdomains());
        let ok: Result<(), &str> = switch.commit_hsts_with(true, async { Ok(()) }).await;
        assert_eq!(ok, Ok(()));
        assert!(switch.hsts_include_subdomains());
        // Changement de domaine : l'option tombe avec le commit, et revient
        // s'il echoue.
        let r: Result<(), &str> = switch
            .commit_with(None, true, async { Err("commit") })
            .await;
        assert_eq!(r, Err("commit"));
        assert!(switch.hsts_include_subdomains());
        let ok: Result<(), &str> = switch.commit_with(None, true, async { Ok(()) }).await;
        assert_eq!(ok, Ok(()));
        assert!(!switch.hsts_include_subdomains());
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
