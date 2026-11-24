//! E-mails de l'instance (invitations, reinitialisation du mot de passe,
//! notifications de `domain::notifications`) par le serveur SMTP configure
//! dans Parametres › Notifications.
//!
//! - Transport lettre, TLS rustls (magasin de certificats du systeme) :
//!   STARTTLS obligatoire (`starttls`) ou TLS implicite (`tls`), jamais en
//!   clair, jamais de certificat invalide accepte.
//! - Le mot de passe SMTP est stocke chiffre par le coffre de l'instance
//!   (`SecretBox`) ; il n'est jamais renvoye par l'API ni journalise.
//! - Un e-mail ne part que si SMTP est active ET `CIPHERA_PUBLIC_URL` est
//!   defini (les liens sont absolus) : voir `ready`.
//! - L'envoi se fait hors du chemin de la requete (`Delivery::spawn`) : la
//!   duree de reponse ne depend ni du serveur SMTP ni de l'existence du compte.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use lettre::message::Mailbox;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use sqlx::PgPool;
use zeroize::Zeroizing;

use crate::secrets::SecretBox;
use crate::state::AppState;

/// Delai maximal d'une operation SMTP (connexion, commande).
pub const SMTP_TIMEOUT: Duration = Duration::from_secs(20);

/// Chiffrement de la connexion SMTP. Pas de variante « aucun ».
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlsMode {
    /// Connexion en clair aussitot chiffree par STARTTLS (refus si le serveur
    /// ne le propose pas) ; port 587 en general.
    StartTls,
    /// TLS des l'ouverture (SMTPS) ; port 465 en general.
    Tls,
}

impl TlsMode {
    /// `starttls` ou `tls`, exactement ; toute autre valeur est refusee.
    pub fn parse(value: &str) -> Option<TlsMode> {
        match value {
            "starttls" => Some(TlsMode::StartTls),
            "tls" => Some(TlsMode::Tls),
            _ => None,
        }
    }

    /// Valeur de la colonne `smtp_settings.tls` et de l'API.
    pub fn as_str(self) -> &'static str {
        match self {
            TlsMode::StartTls => "starttls",
            TlsMode::Tls => "tls",
        }
    }
}

/// Reglages SMTP, mot de passe dechiffre.
#[derive(Clone)]
pub struct SmtpSettings {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub tls: TlsMode,
    /// Vide : pas d'authentification.
    pub username: String,
    pub password: Option<Zeroizing<String>>,
    pub from_address: String,
    pub from_name: String,
}

impl fmt::Debug for SmtpSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmtpSettings")
            .field("enabled", &self.enabled)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("tls", &self.tls)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "***"))
            .field("from_address", &self.from_address)
            .field("from_name", &self.from_name)
            .finish()
    }
}

/// Erreur d'envoi ou de lecture des reglages (sans secret ni contenu).
#[derive(Debug)]
pub struct MailError(String);

impl fmt::Display for MailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "e-mail : {}", self.0)
    }
}

impl std::error::Error for MailError {}

impl MailError {
    /// Erreur portant ce seul texte (sans secret ni contenu).
    pub fn other(message: impl Into<String>) -> Self {
        MailError(message.into())
    }

    fn new(context: &str, error: impl fmt::Display) -> Self {
        MailError(format!("{context} ({error})"))
    }

    /// Serveur SMTP injoignable ou inutilisable (connexion, TLS, delai,
    /// reponse illisible), et non un refus du serveur.
    pub fn connection(detail: impl fmt::Display) -> Self {
        Self::new(UNREACHABLE, detail)
    }

    /// Vrai pour une erreur de [`MailError::connection`] : les envois
    /// suivants echoueraient de meme, inutile de les tenter.
    pub fn is_connection(&self) -> bool {
        self.0.starts_with(UNREACHABLE)
    }
}

/// Debut du message d'une erreur de connexion ([`MailError::connection`]).
const UNREACHABLE: &str = "serveur SMTP injoignable";

/// Message a envoyer, en texte brut.
#[derive(Clone)]
pub struct OutgoingMail {
    pub to: String,
    pub subject: String,
    pub text: String,
}

// Le corps porte un lien a usage unique : jamais affiche.
impl fmt::Debug for OutgoingMail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OutgoingMail")
            .field("to", &"***")
            .field("subject", &self.subject)
            .field("text", &"***")
            .finish()
    }
}

/// Transport SMTP pret a l'emploi (la connexion s'ouvre a l'envoi).
#[derive(Clone)]
pub struct Mailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl fmt::Debug for Mailer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Mailer").finish_non_exhaustive()
    }
}

impl Mailer {
    /// Transport pour ces reglages ; `None` si l'envoi est desactive ou si
    /// les reglages sont inutilisables (motif journalise, sans secret).
    pub fn from_settings(settings: &SmtpSettings) -> Option<Mailer> {
        if !settings.enabled {
            return None;
        }
        let builder = match settings.tls {
            TlsMode::StartTls => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&settings.host)
            }
            TlsMode::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&settings.host),
        };
        let mut builder = match builder {
            Ok(builder) => builder.port(settings.port).timeout(Some(SMTP_TIMEOUT)),
            Err(e) => {
                tracing::warn!(error = %e, "transport SMTP inutilisable");
                return None;
            }
        };
        if !settings.username.is_empty() {
            let password = settings
                .password
                .as_ref()
                .map(|p| p.to_string())
                .unwrap_or_default();
            builder = builder.credentials(Credentials::new(settings.username.clone(), password));
        }
        let address = match settings.from_address.parse() {
            Ok(address) => address,
            Err(e) => {
                tracing::warn!(error = %e, "adresse d'expedition SMTP invalide");
                return None;
            }
        };
        let name = Some(settings.from_name.clone()).filter(|n| !n.is_empty());
        Some(Mailer {
            transport: builder.build(),
            from: Mailbox::new(name, address),
        })
    }

    /// Envoie le message (connexion, chiffrement, authentification, envoi).
    pub async fn send(&self, mail: OutgoingMail) -> Result<(), MailError> {
        let to: Mailbox = mail
            .to
            .parse()
            .map_err(|e| MailError::new("destinataire invalide", e))?;
        let message = Message::builder()
            .from(self.from.clone())
            .to(to)
            .subject(mail.subject)
            .header(ContentType::TEXT_PLAIN)
            .body(mail.text)
            .map_err(|e| MailError::new("message invalide", e))?;
        self.transport.send(message).await.map_err(|e| {
            // Code SMTP 4xx ou 5xx : refus du serveur ; sinon il n'a pas ete
            // joint ou utilise.
            if e.is_transient() || e.is_permanent() {
                MailError::new("envoi SMTP refuse", e)
            } else {
                MailError::connection(e)
            }
        })?;
        Ok(())
    }
}

/// Futur d'un envoi.
pub type SendFuture = Pin<Box<dyn Future<Output = Result<(), MailError>> + Send>>;

/// Facon d'envoyer un message : SMTP, ou une fonction fournie par un test.
#[derive(Clone)]
pub struct Delivery(Arc<dyn Fn(Mailer, OutgoingMail) -> SendFuture + Send + Sync>);

impl Delivery {
    /// Envoi par le transport SMTP du `Mailer`.
    pub fn smtp() -> Self {
        Self(Arc::new(|mailer, mail| {
            Box::pin(async move { mailer.send(mail).await })
        }))
    }

    pub fn from_fn(
        send: impl Fn(Mailer, OutgoingMail) -> SendFuture + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(send))
    }

    /// Envoie et attend le resultat (a appeler hors du chemin d'une requete).
    pub async fn send(&self, mailer: Mailer, mail: OutgoingMail) -> Result<(), MailError> {
        (self.0)(mailer, mail).await
    }

    /// Envoie dans une tache separee : la requete n'attend pas le serveur
    /// SMTP. Un echec est journalise (sans destinataire ni contenu).
    pub fn spawn(&self, mailer: Mailer, mail: OutgoingMail, kind: &'static str) {
        let sending = (self.0)(mailer, mail);
        tokio::spawn(async move {
            if let Err(e) = sending.await {
                tracing::warn!(error = %e, kind, "e-mail non envoye");
            }
        });
    }
}

impl Default for Delivery {
    fn default() -> Self {
        Self::smtp()
    }
}

impl fmt::Debug for Delivery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Delivery")
    }
}

/// Reglages enregistres (`None` : aucun serveur SMTP), mot de passe dechiffre.
pub async fn load_settings(
    db: &PgPool,
    secrets: &SecretBox,
) -> Result<Option<SmtpSettings>, MailError> {
    type Row = (
        bool,
        String,
        i32,
        String,
        String,
        Option<Vec<u8>>,
        String,
        String,
    );
    let row: Option<Row> = sqlx::query_as(
        "SELECT enabled, host, port, tls, username, password_sealed, from_address, from_name \
         FROM smtp_settings",
    )
    .fetch_optional(db)
    .await
    .map_err(|e| MailError::new("lecture des reglages SMTP", e))?;
    let Some((enabled, host, port, tls, username, sealed, from_address, from_name)) = row else {
        return Ok(None);
    };
    let password = match sealed {
        Some(sealed) => {
            let plain = secrets
                .open_bound(crate::secrets::labels::SMTP_PASSWORD, &sealed)
                .map_err(|e| MailError::new("mot de passe SMTP", e))?;
            let text = std::str::from_utf8(&plain)
                .map_err(|_| MailError("mot de passe SMTP illisible".into()))?;
            Some(Zeroizing::new(text.to_string()))
        }
        None => None,
    };
    Ok(Some(SmtpSettings {
        enabled,
        host,
        port: u16::try_from(port).map_err(|_| MailError("port SMTP invalide".into()))?,
        tls: TlsMode::parse(&tls).ok_or_else(|| MailError("mode TLS invalide".into()))?,
        username,
        password,
        from_address,
        from_name,
    }))
}

/// Transport utilisable si l'instance peut envoyer des liens : origine
/// publique definie, SMTP configure et active. Ne depend d'aucun compte.
pub async fn ready(state: &AppState) -> Result<Option<Mailer>, MailError> {
    if state.public_url().is_none() {
        return Ok(None);
    }
    Ok(load_settings(&state.db, &state.secrets)
        .await?
        .as_ref()
        .and_then(Mailer::from_settings))
}

/// Meme condition que `ready`, sans dechiffrer le mot de passe ni construire
/// le transport : pour les lectures ouvertes (ecran « mot de passe oublie »,
/// Parametres). Ne depend d'aucun compte.
pub async fn available(state: &AppState) -> Result<bool, MailError> {
    if state.public_url().is_none() {
        return Ok(false);
    }
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM smtp_settings WHERE enabled)")
        .fetch_one(&state.db)
        .await
        .map_err(|e| MailError::new("lecture des reglages SMTP", e))
}

/// E-mail du lien de reinitialisation (30 minutes).
pub fn reset_mail(to: &str, link: &str) -> OutgoingMail {
    OutgoingMail {
        to: to.to_string(),
        subject: "Réinitialisation de votre mot de passe Ciphera".to_string(),
        text: format!(
            "Bonjour,\n\n\
             Une réinitialisation du mot de passe de votre compte Ciphera a été demandée.\n\
             Ouvrez ce lien dans les 30 minutes pour choisir un nouveau mot de passe :\n\n\
             {link}\n\n\
             Le lien ne sert qu'une fois. Si vous n'êtes pas à l'origine de cette demande, \
             ignorez ce message : votre mot de passe reste inchangé.\n"
        ),
    }
}

/// Avis envoye a l'ancienne adresse apres un changement d'e-mail (la nouvelle
/// adresse n'y figure pas).
pub fn email_changed_mail(to: &str) -> OutgoingMail {
    OutgoingMail {
        to: to.to_string(),
        subject: "Adresse e-mail de votre compte Ciphera modifiée".to_string(),
        text: "Bonjour,\n\n\
               L'adresse e-mail de votre compte Ciphera vient d'être remplacée : \
               les prochains messages ne seront plus envoyés à cette adresse.\n\n\
               Si vous n'êtes pas à l'origine de ce changement, contactez sans attendre \
               un administrateur de votre instance.\n"
            .to_string(),
    }
}

/// Type de second facteur ajoute (pour l'avis du titulaire).
#[derive(Clone, Copy, Debug)]
pub enum Factor {
    Totp,
    Passkey,
}

/// Avis envoye au titulaire quand un second facteur vient d'etre ajoute.
pub fn factor_added_mail(to: &str, factor: Factor) -> OutgoingMail {
    let what = match factor {
        Factor::Totp => "une application d'authentification (code à usage unique)",
        Factor::Passkey => "une passkey ou une clé de sécurité",
    };
    OutgoingMail {
        to: to.to_string(),
        subject: "Nouveau facteur ajouté à votre compte Ciphera".to_string(),
        text: format!(
            "Bonjour,\n\n\
             Un nouveau facteur de connexion vient d'être ajouté à votre compte Ciphera : {what}.\n\n\
             Si vous n'êtes pas à l'origine de cet ajout, changez votre mot de passe, \
             retirez ce facteur depuis votre profil et contactez sans attendre \
             un administrateur de votre instance.\n"
        ),
    }
}

/// E-mail du lien de verification d'adresse (24 heures), envoye a l'adresse a
/// confirmer : `signup` (inscription ouverte) ou changement d'adresse.
pub fn email_verification_mail(to: &str, link: &str, signup: bool) -> OutgoingMail {
    let (subject, intro) = if signup {
        (
            "Confirmez votre adresse e-mail Ciphera",
            "Une inscription à une instance Ciphera a été faite avec cette adresse.",
        )
    } else {
        (
            "Confirmez votre nouvelle adresse e-mail Ciphera",
            "Cette adresse a été indiquée comme nouvelle adresse d'un compte Ciphera.",
        )
    };
    OutgoingMail {
        to: to.to_string(),
        subject: subject.to_string(),
        text: format!(
            "Bonjour,\n\n\
             {intro}\n\
             Ouvrez ce lien dans les 24 heures pour la confirmer :\n\n\
             {link}\n\n\
             Le lien ne sert qu'une fois. Si vous n'êtes pas à l'origine de cette demande, \
             ignorez ce message : rien ne sera modifié.\n"
        ),
    }
}

/// Avis envoye au titulaire quand un administrateur remet a zero son second
/// facteur (TOTP, passkeys, codes de secours supprimes, sessions revoquees).
pub fn mfa_reset_mail(to: &str) -> OutgoingMail {
    OutgoingMail {
        to: to.to_string(),
        subject: "Second facteur de votre compte Ciphera réinitialisé".to_string(),
        text: "Bonjour,\n\n\
               Un administrateur vient de supprimer le second facteur de votre compte Ciphera \
               (application d'authentification, passkeys et codes de secours). \
               Vos sessions ont été fermées.\n\n\
               Reconnectez-vous avec votre mot de passe, puis ajoutez un nouveau facteur \
               depuis votre profil. Si vous n'êtes pas à l'origine de cette demande, \
               changez votre mot de passe et contactez sans attendre un administrateur \
               de votre instance.\n"
            .to_string(),
    }
}

/// Avis aux proprietaires apres un changement de serveur ou d'expediteur
/// SMTP, envoye par l'ancien transport (jamais de mot de passe).
pub fn smtp_changed_mail(
    to: &str,
    actor: &str,
    old: (&str, u16, &str),
    new: (&str, u16, &str),
) -> OutgoingMail {
    OutgoingMail {
        to: to.to_string(),
        subject: "Serveur d'envoi des e-mails Ciphera modifié".to_string(),
        text: format!(
            "Bonjour,\n\n\
             Les réglages d'envoi des e-mails de votre instance Ciphera ont été modifiés par {actor}.\n\n\
             Avant : serveur {}:{}, expéditeur {}\n\
             Après : serveur {}:{}, expéditeur {}\n\n\
             Si vous n'êtes pas à l'origine de ce changement, vérifiez Paramètres › Notifications \
             et les comptes de votre instance sans attendre.\n",
            old.0, old.1, old.2, new.0, new.1, new.2
        ),
    }
}

/// E-mail d'invitation (72 heures).
pub fn invitation_mail(to: &str, link: &str) -> OutgoingMail {
    OutgoingMail {
        to: to.to_string(),
        subject: "Invitation à rejoindre Ciphera".to_string(),
        text: format!(
            "Bonjour,\n\n\
             Vous êtes invité à rejoindre une instance Ciphera.\n\
             Ouvrez ce lien dans les 72 heures pour créer votre compte :\n\n\
             {link}\n\n\
             Le lien ne sert qu'une fois. Si vous ne vous attendiez pas à cette invitation, \
             ignorez ce message.\n"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> SmtpSettings {
        SmtpSettings {
            enabled: true,
            host: "smtp.example.com".into(),
            port: 587,
            tls: TlsMode::StartTls,
            username: "user".into(),
            password: Some(Zeroizing::new("smtp-test-secret".into())),
            from_address: "ciphera@example.com".into(),
            from_name: "Ciphera".into(),
        }
    }

    #[test]
    fn debug_never_shows_secrets() {
        assert!(!format!("{:?}", settings()).contains("smtp-test-secret"));
        let mail = reset_mail("nora@example.com", "https://p.example.com/reset/TOKEN");
        let shown = format!("{mail:?}");
        assert!(!shown.contains("TOKEN") && !shown.contains("nora@"));
        assert!(mail.text.contains("https://p.example.com/reset/TOKEN"));
        assert!(
            !format!("{:?}", Mailer::from_settings(&settings()).unwrap())
                .contains("smtp-test-secret")
        );
    }

    /// Serveur SMTP injoignable (port local ferme) : l'erreur du vrai
    /// transport est classee « connexion », ce qui arrete la tache de
    /// notification au premier echec (`domain::notifications`).
    #[tokio::test]
    async fn unreachable_server_is_a_connection_error() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        for tls in [TlsMode::StartTls, TlsMode::Tls] {
            let mut s = settings();
            s.host = "127.0.0.1".into();
            s.port = port;
            s.tls = tls;
            let mailer = Mailer::from_settings(&s).unwrap();
            let err = mailer
                .send(OutgoingMail {
                    to: "nora@example.com".into(),
                    subject: "Test".into(),
                    text: "Corps".into(),
                })
                .await
                .unwrap_err();
            assert!(err.is_connection(), "{tls:?} : {err}");
        }
        assert!(!MailError::other("envoi SMTP refuse").is_connection());
    }

    #[test]
    fn invalid_sender_gives_no_mailer() {
        let mut s = settings();
        s.from_address = "pas une adresse".into();
        assert!(Mailer::from_settings(&s).is_none());
    }
}
