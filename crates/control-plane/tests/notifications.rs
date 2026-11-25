//! Notifications e-mail (Profil › Notifications) : tache `notify` mise en
//! file a l'evenement, destinataires selon les preferences et les roles,
//! envoi idempotent par evenement et par destinataire, plafond horaire par
//! compte, rien sans SMTP. L'envoi SMTP est remplace par une boite d'envoi
//! en memoire.

mod common;

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use control_plane::domain::notifications::{self, Event, MAX_PER_HOUR};
use control_plane::jobs::{self, JobKind};
use control_plane::mail::{Delivery, MailError, OutgoingMail};
use control_plane::state::{AppState, Clock};
use shared::config::Config;
use uuid::Uuid;

const PUBLIC_URL: &str = "https://panel.example.com";

type Outbox = Arc<Mutex<Vec<OutgoingMail>>>;

struct Env {
    state: AppState,
    _pg: common::TestDb,
    outbox: Outbox,
    /// Adresses dont le prochain envoi echoue (une fois chacune).
    failing: Arc<Mutex<HashSet<String>>>,
    offset: Arc<AtomicU64>,
}

impl Env {
    async fn new() -> Self {
        let (mut state, pg) = common::state().await;
        state.cfg = Arc::new(
            Config::from_lookup(|name| match name {
                "CIPHERA_DATABASE_URL" => Some("postgres://u:p@127.0.0.1/c".to_string()),
                "CIPHERA_PUBLIC_URL" => Some(PUBLIC_URL.to_string()),
                _ => None,
            })
            .unwrap(),
        );
        let offset = Arc::new(AtomicU64::new(0));
        let o = offset.clone();
        state.clock = Clock::from_fn(move || {
            SystemTime::now() + Duration::from_secs(o.load(Ordering::SeqCst))
        });
        let outbox: Outbox = Arc::default();
        let failing: Arc<Mutex<HashSet<String>>> = Arc::default();
        let (sink, fail) = (outbox.clone(), failing.clone());
        state.mail = Delivery::from_fn(move |_mailer, mail| {
            let refused = fail.lock().unwrap().remove(&mail.to);
            if !refused {
                sink.lock().unwrap().push(mail);
            }
            Box::pin(async move {
                if refused {
                    Err(MailError::other("serveur SMTP indisponible"))
                } else {
                    Ok(())
                }
            })
        });
        Self {
            state,
            _pg: pg,
            outbox,
            failing,
            offset,
        }
    }

    fn advance_clock(&self, by: Duration) {
        self.offset.fetch_add(by.as_secs(), Ordering::SeqCst);
    }

    async fn smtp(&self, enabled: bool) {
        sqlx::query(
            "INSERT INTO smtp_settings (enabled, host, port, tls, from_address, from_name) \
             VALUES ($1, 'smtp.example.com', 587, 'starttls', 'ciphera@example.com', 'Ciphera') \
             ON CONFLICT (id) DO UPDATE SET enabled = excluded.enabled",
        )
        .bind(enabled)
        .execute(&self.state.db)
        .await
        .unwrap();
    }

    /// Compte de role `role` ; `prefs` : (e-mail, reussi, echec, certificats)
    /// ou valeurs par defaut.
    async fn user(&self, role: &str, prefs: Option<(bool, bool, bool, bool)>) -> String {
        let id = Uuid::new_v4();
        let email = format!("{role}-{}@example.com", id.simple());
        sqlx::query(
            "INSERT INTO users (id, email, password_hash, role, name, email_verified_at) VALUES ($1, $2, 'x', $3, 'Test', now())",
        )
        .bind(id)
        .bind(&email)
        .bind(role)
        .execute(&self.state.db)
        .await
        .unwrap();
        if let Some((email_on, ok, failed, certs)) = prefs {
            sqlx::query(
                "INSERT INTO notification_prefs \
                 (user_id, email_enabled, deploy_succeeded, deploy_failed, certificates, team) \
                 VALUES ($1, $2, $3, $4, $5, true)",
            )
            .bind(id)
            .bind(email_on)
            .bind(ok)
            .bind(failed)
            .bind(certs)
            .execute(&self.state.db)
            .await
            .unwrap();
        }
        email
    }

    /// Projet `web`, environnement `production` ; renvoie (projet, env).
    async fn project(&self, name: &str) -> (Uuid, Uuid) {
        let (p, e) = (Uuid::new_v4(), Uuid::new_v4());
        sqlx::query(
            "INSERT INTO projects (id, name, source_kind, image_ref) VALUES ($1, $2, 'image', 'ghcr.io/acme/app:1')",
        )
        .bind(p)
        .bind(name)
        .execute(&self.state.db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO environments (id, project_id, name) VALUES ($1, $2, 'production')",
        )
        .bind(e)
        .bind(p)
        .execute(&self.state.db)
        .await
        .unwrap();
        (p, e)
    }

    /// Deploiement termine (`status`) avec une cause d'echec qui ne doit
    /// jamais partir par e-mail.
    async fn deployment(&self, project: (Uuid, Uuid), status: &str) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO deployments (id, project_id, environment_id, status, trigger, stage, error, created_at, finished_at) \
             VALUES ($1, $2, $3, $4, 'manual', 'healthcheck', 'API_TOKEN=sk-live-NEVER-MAILED', now(), now())",
        )
        .bind(id)
        .bind(project.0)
        .bind(project.1)
        .bind(status)
        .execute(&self.state.db)
        .await
        .unwrap();
        id
    }

    async fn notify(&self, event: Event) {
        let mut conn = self.state.db.acquire().await.unwrap();
        notifications::enqueue(&mut conn, &self.state, event)
            .await
            .unwrap();
    }

    async fn notify_jobs(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM jobs WHERE kind = 'notify'")
            .fetch_one(&self.state.db)
            .await
            .unwrap()
    }

    /// Execute les taches `notify` dues comme le travailleur ; renvoie le
    /// nombre d'echecs (reprise demandee).
    async fn run(&self) -> usize {
        let mut retries = 0;
        for _ in 0..100 {
            let Some(job) = jobs::claim(&self.state.db, &self.state.clock, "test")
                .await
                .unwrap()
            else {
                return retries;
            };
            assert_eq!(job.kind, JobKind::Notify);
            let id = job.id;
            match notifications::handle(&self.state, job).await {
                Ok(()) => {
                    jobs::complete(&self.state.db, &self.state.clock, id, "test")
                        .await
                        .unwrap();
                }
                Err(e) => {
                    assert!(e.retry, "{}", e.message);
                    retries += 1;
                    jobs::fail(
                        &self.state.db,
                        &self.state.clock,
                        id,
                        "test",
                        &e.message,
                        jobs::Failure::Retry(Duration::from_secs(60)),
                    )
                    .await
                    .unwrap();
                }
            }
        }
        panic!("file de taches sans fin");
    }

    /// Taches `notify` en file, telles quelles : la file n'en laisse qu'une
    /// en cours a la fois, mais une reprise apres bail echu peut en faire
    /// tourner deux (les tests de concurrence les executent sans la file).
    async fn queued_notify_jobs(&self) -> Vec<jobs::Job> {
        let rows: Vec<(uuid::Uuid, serde_json::Value)> = sqlx::query_as(
            "SELECT id, payload FROM jobs WHERE kind = 'notify' AND status = 'queued' \
             ORDER BY created_at",
        )
        .fetch_all(&self.state.db)
        .await
        .unwrap();
        rows.into_iter()
            .map(|(id, payload)| jobs::Job {
                id: shared::ids::JobId(id),
                kind: JobKind::Notify,
                payload,
                attempts: 1,
                max_attempts: 6,
            })
            .collect()
    }

    /// Execute les taches `notify` en file hors de la file (voir
    /// `queued_notify_jobs`) et les marque terminees.
    async fn run_outside_queue(&self) {
        for job in self.queued_notify_jobs().await {
            let id = job.id;
            notifications::handle(&self.state, job).await.unwrap();
            sqlx::query("UPDATE jobs SET status = 'done', finished_at = now() WHERE id = $1")
                .bind(id)
                .execute(&self.state.db)
                .await
                .unwrap();
        }
    }

    fn sent(&self) -> Vec<OutgoingMail> {
        self.outbox.lock().unwrap().clone()
    }

    fn recipients(&self) -> HashSet<String> {
        self.sent().into_iter().map(|m| m.to).collect()
    }
}

#[tokio::test]
async fn deploy_failure_mails_members_whose_pref_is_on() {
    let env = Env::new().await;
    env.smtp(true).await;
    let owner = env.user("owner", None).await;
    let viewer = env.user("viewer", None).await;
    let _no_failures = env.user("developer", Some((true, true, false, true))).await;
    let _no_email = env.user("ops", Some((false, true, true, true))).await;
    let p = env.project("web").await;
    let id = env.deployment(p, "failed").await;

    env.notify(Event::DeployFailed { deployment_id: id }).await;
    assert_eq!(env.notify_jobs().await, 1, "tache mise en file");
    assert_eq!(env.run().await, 0);

    assert_eq!(env.recipients(), HashSet::from([owner, viewer]));
    for mail in env.sent() {
        assert!(mail.subject.contains("web"), "{}", mail.subject);
        assert!(mail.subject.contains("chec"), "{}", mail.subject);
        assert!(
            mail.text
                .contains(&format!("{PUBLIC_URL}/deployments/{id}")),
            "{}",
            mail.text
        );
        assert!(mail.text.contains("production"));
        assert!(mail.text.contains("healthcheck"), "etape en echec");
        assert!(
            !mail.text.contains("NEVER-MAILED") && !mail.text.contains("API_TOKEN"),
            "jamais la cause brute (journal) : {}",
            mail.text
        );
    }
}

#[tokio::test]
async fn success_and_certificate_mails_follow_their_own_prefs() {
    let env = Env::new().await;
    env.smtp(true).await;
    let default = env.user("developer", None).await;
    let no_success = env.user("developer", Some((true, false, true, true))).await;
    let p = env.project("api").await;
    let id = env.deployment(p, "success").await;
    env.notify(Event::DeploySucceeded { deployment_id: id })
        .await;
    env.run().await;
    assert_eq!(env.recipients(), HashSet::from([default.clone()]));
    assert!(env.sent()[0].subject.contains("api"));

    // Certificats : desactives par defaut, actives pour `no_success`.
    env.outbox.lock().unwrap().clear();
    let domain = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO domains (id, project_id, environment_id, host, dns_status, cert_status, cert_error) \
         VALUES ($1, $2, $3, 'api.example.com', 'ok', 'error', 'detail interne du proxy')",
    )
    .bind(domain)
    .bind(p.0)
    .bind(p.1)
    .execute(&env.state.db)
    .await
    .unwrap();
    env.notify(Event::CertFailed {
        domain_id: domain,
        job_id: Uuid::new_v4(),
        renewal: false,
    })
    .await;
    env.run().await;
    assert_eq!(env.recipients(), HashSet::from([no_success]));
    let mail = &env.sent()[0];
    assert!(mail.subject.contains("api.example.com"), "{}", mail.subject);
    assert!(
        mail.text
            .contains(&format!("{PUBLIC_URL}/projects/{}/domains", p.0)),
        "{}",
        mail.text
    );
    assert!(!mail.text.contains("detail interne"));
}

#[tokio::test]
async fn panel_certificate_mails_only_roles_with_settings_access() {
    let env = Env::new().await;
    env.smtp(true).await;
    let all = Some((true, true, true, true));
    let owner = env.user("owner", all).await;
    let admin = env.user("admin", all).await;
    for role in ["ops", "developer", "viewer"] {
        env.user(role, all).await;
    }
    let domain = Uuid::new_v4();
    let expires = env.state.clock.unix_secs() as i64 + 3 * 86_400;
    sqlx::query(
        "INSERT INTO domains (id, host, panel, dns_status, cert_status, cert_expires_at) \
         VALUES ($1, 'panel.example.com', true, 'ok', 'active', to_timestamp($2::float8))",
    )
    .bind(domain)
    .bind(expires as f64)
    .execute(&env.state.db)
    .await
    .unwrap();
    env.notify(Event::CertExpiring {
        domain_id: domain,
        expires_at: expires,
    })
    .await;
    env.run().await;
    assert_eq!(env.recipients(), HashSet::from([owner, admin]));
    let mail = &env.sent()[0];
    assert!(mail.subject.contains("expire"), "{}", mail.subject);
    assert!(
        mail.text
            .contains(&format!("{PUBLIC_URL}/settings/panel-domain")),
        "{}",
        mail.text
    );
}

#[tokio::test]
async fn retries_never_send_twice_to_the_same_recipient() {
    let env = Env::new().await;
    env.smtp(true).await;
    let a = env.user("owner", None).await;
    let b = env.user("developer", None).await;
    let p = env.project("retry").await;
    let id = env.deployment(p, "failed").await;
    env.failing.lock().unwrap().insert(b.clone());

    env.notify(Event::DeployFailed { deployment_id: id }).await;
    // Meme evenement signale deux fois (statut confirme) : une seule tache.
    env.notify(Event::DeployFailed { deployment_id: id }).await;
    assert_eq!(env.notify_jobs().await, 1);
    assert_eq!(env.run().await, 1, "echec pour b : reprise demandee");
    assert_eq!(env.recipients(), HashSet::from([a.clone()]));
    // La reprise n'envoie qu'a b.
    env.advance_clock(Duration::from_secs(61));
    env.run().await;
    let sent: Vec<String> = env.sent().into_iter().map(|m| m.to).collect();
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert_eq!(HashSet::from_iter(sent), HashSet::from([a, b]));

    // Evenement rejoue apres coup (tache terminee) : rien de plus.
    env.notify(Event::DeployFailed { deployment_id: id }).await;
    env.run().await;
    assert_eq!(env.sent().len(), 2);
}

/// Notifications retenues (statut `suppressed`) du compte.
async fn held(env: &Env) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM notification_deliveries WHERE status = 'suppressed'")
        .fetch_one(&env.state.db)
        .await
        .unwrap()
}

#[tokio::test]
async fn held_alerts_arrive_in_a_digest_when_the_hour_frees_up() {
    let env = Env::new().await;
    env.smtp(true).await;
    let user = env.user("developer", None).await;
    let p = env.project("loop").await;
    let cap = usize::try_from(MAX_PER_HOUR).unwrap();
    let mut ids = Vec::new();
    for _ in 0..cap + 2 {
        let id = env.deployment(p, "failed").await;
        env.notify(Event::DeployFailed { deployment_id: id }).await;
        ids.push(id);
    }
    env.run().await;
    assert_eq!(env.sent().len(), cap, "plafond horaire");
    assert_eq!(held(&env).await, 2);

    // Aucun autre evenement : le resume part quand l'heure se libere, pas avant.
    env.advance_clock(Duration::from_secs(1800));
    env.run().await;
    assert_eq!(env.sent().len(), cap);
    env.advance_clock(Duration::from_secs(1801));
    env.run().await;
    let sent = env.sent();
    assert_eq!(sent.len(), cap + 1, "un resume");
    let digest = sent.last().unwrap();
    assert_eq!(digest.to, user);
    assert!(
        digest.subject.contains("2 notifications"),
        "{}",
        digest.subject
    );
    // Les alertes retenues sont dans le resume, avec leur lien.
    for id in &ids[cap..] {
        assert!(
            digest
                .text
                .contains(&format!("{PUBLIC_URL}/deployments/{id}")),
            "{}",
            digest.text
        );
    }
    assert_eq!(held(&env).await, 0);
    // Rien de plus ensuite.
    env.advance_clock(Duration::from_secs(7200));
    env.run().await;
    assert_eq!(env.sent().len(), cap + 1);
}

#[tokio::test]
async fn failure_alerts_are_not_starved_by_successes() {
    let env = Env::new().await;
    env.smtp(true).await;
    env.user("developer", None).await;
    let p = env.project("busy").await;
    let cap = usize::try_from(MAX_PER_HOUR).unwrap();
    for _ in 0..cap + 2 {
        let id = env.deployment(p, "success").await;
        env.notify(Event::DeploySucceeded { deployment_id: id })
            .await;
    }
    env.run().await;
    assert_eq!(env.sent().len(), cap);
    // Budget propre aux alertes : l'echec part tout de suite.
    let failed = env.deployment(p, "failed").await;
    env.notify(Event::DeployFailed {
        deployment_id: failed,
    })
    .await;
    env.run().await;
    let sent = env.sent();
    assert_eq!(sent.len(), cap + 1);
    assert!(
        sent.last()
            .unwrap()
            .text
            .contains(&format!("/deployments/{failed}"))
    );
    // Les reussites retenues ne sont qu'un compte dans le resume.
    env.advance_clock(Duration::from_secs(3601));
    env.run().await;
    let digest = env.sent().last().unwrap().clone();
    assert!(
        digest.text.contains("2 déploiements réussis"),
        "{}",
        digest.text
    );
}

#[tokio::test]
async fn the_hourly_cap_is_exact_with_concurrent_jobs() {
    let env = Env::new().await;
    env.smtp(true).await;
    env.user("developer", None).await;
    let p = env.project("race").await;
    let cap = usize::try_from(MAX_PER_HOUR).unwrap();
    for _ in 0..cap + 6 {
        let id = env.deployment(p, "failed").await;
        env.notify(Event::DeployFailed { deployment_id: id }).await;
    }
    // Envoi lent (serveur SMTP distant) : les taches se chevauchent.
    let mut slow = env.state.clone();
    let sink = env.outbox.clone();
    slow.mail = Delivery::from_fn(move |_mailer, mail| {
        let sink = sink.clone();
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            sink.lock().unwrap().push(mail);
            Ok(())
        })
    });
    // Toutes les taches executees en meme temps (reprises simultanees).
    let mut set = tokio::task::JoinSet::new();
    for job in env.queued_notify_jobs().await {
        let state = slow.clone();
        set.spawn(async move { notifications::handle(&state, job).await });
    }
    while let Some(r) = set.join_next().await {
        r.unwrap().unwrap();
    }
    assert_eq!(env.sent().len(), cap);
    assert_eq!(held(&env).await, 6);
}

#[tokio::test]
async fn certificate_mails_default_on_for_owners_and_admins_only() {
    let env = Env::new().await;
    env.smtp(true).await;
    let owner = env.user("owner", None).await;
    let admin = env.user("admin", None).await;
    env.user("developer", None).await;
    env.user("ops", None).await;
    let p = env.project("certs").await;
    let domain = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO domains (id, project_id, environment_id, host, dns_status, cert_status) \
         VALUES ($1, $2, $3, 'certs.example.com', 'ok', 'error')",
    )
    .bind(domain)
    .bind(p.0)
    .bind(p.1)
    .execute(&env.state.db)
    .await
    .unwrap();
    env.notify(Event::CertFailed {
        domain_id: domain,
        job_id: Uuid::new_v4(),
        renewal: false,
    })
    .await;
    env.run().await;
    assert_eq!(env.recipients(), HashSet::from([owner, admin]));
}

#[tokio::test]
async fn without_smtp_nothing_is_queued_or_sent() {
    let env = Env::new().await;
    env.user("owner", None).await;
    let p = env.project("quiet").await;
    let id = env.deployment(p, "failed").await;

    // Aucun serveur SMTP : rien en file.
    env.notify(Event::DeployFailed { deployment_id: id }).await;
    env.smtp(false).await;
    env.notify(Event::DeployFailed { deployment_id: id }).await;
    assert_eq!(env.notify_jobs().await, 0);

    // SMTP coupe entre la mise en file et l'envoi : tache conclue sans envoi.
    env.smtp(true).await;
    env.notify(Event::DeployFailed { deployment_id: id }).await;
    env.smtp(false).await;
    assert_eq!(env.run().await, 0);
    assert!(env.sent().is_empty());
}

#[tokio::test]
async fn an_outcome_overridden_before_sending_is_not_mailed() {
    let env = Env::new().await;
    env.smtp(true).await;
    env.user("owner", None).await;
    let p = env.project("late").await;
    // Echec presume, puis verdict reel `success` avant l'envoi.
    let id = env.deployment(p, "success").await;
    env.notify(Event::DeployFailed { deployment_id: id }).await;
    env.run().await;
    assert!(env.sent().is_empty());
}

/// 12 echecs : 10 envoyes, 2 retenus ; renvoie les identifiants.
async fn fill_the_hour(env: &Env, p: (Uuid, Uuid)) -> Vec<Uuid> {
    let mut ids = Vec::new();
    for _ in 0..MAX_PER_HOUR + 2 {
        let id = env.deployment(p, "failed").await;
        env.notify(Event::DeployFailed { deployment_id: id }).await;
        ids.push(id);
    }
    env.run().await;
    ids
}

#[tokio::test]
async fn an_alert_held_while_the_digest_runs_goes_out_in_a_later_digest() {
    let env = Env::new().await;
    env.smtp(true).await;
    env.user("developer", None).await;
    let p = env.project("race").await;
    fill_the_hour(&env, p).await;
    let cap = usize::try_from(MAX_PER_HOUR).unwrap();
    env.advance_clock(Duration::from_secs(3601));

    // Resume en cours (envoi lent) ; pendant ce temps une alerte est
    // retenue (budget toujours plein : envois recents simules).
    let job = jobs::claim(&env.state.db, &env.state.clock, "test")
        .await
        .unwrap()
        .expect("resume du");
    let digest_job = job.id;
    let mut slow = env.state.clone();
    let sink = env.outbox.clone();
    slow.mail = Delivery::from_fn(move |_mailer, mail| {
        let sink = sink.clone();
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            sink.lock().unwrap().push(mail);
            Ok(())
        })
    });
    let running = tokio::spawn(async move { notifications::handle(&slow, job).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    sqlx::query(
        "UPDATE notification_deliveries SET sent_at = to_timestamp($1::float8) WHERE status = 'sent'",
    )
    .bind(env.state.clock.unix_secs() - 10.0)
    .execute(&env.state.db)
    .await
    .unwrap();
    let late = env.deployment(p, "failed").await;
    env.notify(Event::DeployFailed {
        deployment_id: late,
    })
    .await;
    env.run_outside_queue().await;
    running.await.unwrap().unwrap();
    assert!(
        jobs::complete(&env.state.db, &env.state.clock, digest_job, "test")
            .await
            .unwrap()
    );
    assert_eq!(env.sent().len(), cap + 1, "premier resume");
    assert_eq!(held(&env).await, 1, "alerte retenue pendant le resume");

    // Un resume ulterieur l'envoie, sans autre evenement.
    env.advance_clock(Duration::from_secs(3601));
    env.run().await;
    let sent = env.sent();
    assert_eq!(sent.len(), cap + 2);
    assert!(
        sent.last()
            .unwrap()
            .text
            .contains(&format!("/deployments/{late}")),
        "{}",
        sent.last().unwrap().text
    );
    assert_eq!(held(&env).await, 0);
}

#[tokio::test]
async fn the_hourly_sweep_recovers_orphaned_reservations_and_held_alerts() {
    let env = Env::new().await;
    env.smtp(true).await;
    let user = env.user("developer", None).await;
    let p = env.project("orphan").await;
    let ids = fill_the_hour(&env, p).await;
    // Tentatives epuisees : resume abandonne, et une reservation laissee
    // par un arret sur la derniere tentative.
    sqlx::query("DELETE FROM jobs WHERE kind = 'notify'")
        .execute(&env.state.db)
        .await
        .unwrap();
    sqlx::query("UPDATE notification_deliveries SET status = 'pending' WHERE event_key = $1")
        .bind(format!("deploy:{}:failed", ids[0]))
        .execute(&env.state.db)
        .await
        .unwrap();

    env.advance_clock(Duration::from_secs(2 * 3600));
    notifications::sweep(&env.state.db, env.state.clock.unix_secs())
        .await
        .unwrap();
    env.run().await;
    let sent = env.sent();
    let digest = sent.last().unwrap();
    assert_eq!(digest.to, user);
    for id in [ids[0], ids[10], ids[11]] {
        assert!(
            digest.text.contains(&format!("/deployments/{id}")),
            "{}",
            digest.text
        );
    }
    assert_eq!(held(&env).await, 0);
}

/// Reservations encore `pending` (envoi en cours ou interrompu).
async fn pending(env: &Env) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM notification_deliveries WHERE status = 'pending'")
        .fetch_one(&env.state.db)
        .await
        .unwrap()
}

/// Serveur SMTP injoignable : la tache s'arrete au premier echec de
/// connexion (pas un essai par destinataire) et demande une reprise ;
/// aucune reservation ne reste.
#[tokio::test]
async fn notify_stops_at_first_connection_failure() {
    let mut env = Env::new().await;
    env.smtp(true).await;
    for _ in 0..3 {
        env.user("developer", None).await;
    }
    let p = env.project("down").await;
    let id = env.deployment(p, "failed").await;
    let attempts = Arc::new(AtomicU64::new(0));
    let a = attempts.clone();
    env.state.mail = Delivery::from_fn(move |_mailer, _mail| {
        a.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Err(MailError::connection("serveur SMTP injoignable")) })
    });
    env.notify(Event::DeployFailed { deployment_id: id }).await;
    assert_eq!(env.run().await, 1, "reprise demandee");
    assert_eq!(attempts.load(Ordering::SeqCst), 1, "arret au premier echec");
    assert_eq!(pending(&env).await, 0);
}

/// Duree totale d'une tache bornee : un envoi qui ne finit pas rend la
/// main a l'echeance, sa reservation est rendue et la reprise est demandee.
#[tokio::test]
async fn notify_job_duration_is_bounded() {
    let mut env = Env::new().await;
    env.smtp(true).await;
    let fast = env.user("owner", None).await;
    let slow = env.user("admin", None).await;
    let p = env.project("slow").await;
    let id = env.deployment(p, "failed").await;
    let sink = env.outbox.clone();
    let stuck = slow.clone();
    env.state.mail = Delivery::from_fn(move |_mailer, mail| {
        let stall = mail.to == stuck;
        if !stall {
            sink.lock().unwrap().push(mail);
        }
        Box::pin(async move {
            if stall {
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
            Ok(())
        })
    });
    env.notify(Event::DeployFailed { deployment_id: id }).await;
    let job = jobs::claim(&env.state.db, &env.state.clock, "test")
        .await
        .unwrap()
        .unwrap();
    let started = std::time::Instant::now();
    let outcome = notifications::handle_within(&env.state, job, Duration::from_millis(500)).await;
    assert!(started.elapsed() < Duration::from_secs(5));
    let err = outcome.expect_err("tache interrompue a l'echeance");
    assert!(err.retry, "{}", err.message);
    assert_eq!(env.recipients(), HashSet::from([fast]));
    assert_eq!(pending(&env).await, 0, "reservation rendue");
}
