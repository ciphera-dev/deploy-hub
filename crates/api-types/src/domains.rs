//! Domaines d'un projet (`/api/domains?project=`) : domaines personnalises
//! et domaine genere `<projet>.<ip>.sslip.io`, verification DNS (A/AAAA vers
//! l'IP du noeud), HTTPS automatique (ACME HTTP-01, renouvellement a J-30),
//! domaine principal et redirection 308 vers lui.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Etat de la verification DNS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DnsStatus {
    /// Jamais verifie, ou verification a refaire.
    Pending,
    /// Resout vers l'IP du noeud : le domaine est en ligne.
    Ok,
    /// Ne resout pas vers l'IP du noeud (`dns_error`).
    Error,
}

/// Etat du certificat HTTPS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CertStatus {
    /// Aucun certificat (DNS non verifie).
    None,
    /// Emission ACME en cours.
    Issuing,
    /// Certificat valide (`cert_expires_at`).
    Active,
    /// Derniere emission en echec (`cert_error`) ; nouvel essai automatique.
    Error,
}

/// Domaine d'un projet.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
pub struct Domain {
    pub id: String,
    /// Nom d'hote, en minuscules.
    pub host: String,
    /// Prefixe de chemin servi (`/` : tout l'hote).
    pub path_prefix: String,
    /// Domaine genere par Ciphera (`<projet>.<ip>.sslip.io`).
    pub generated: bool,
    /// Domaine principal de l'environnement.
    pub primary: bool,
    /// Redirige en 308 vers le domaine principal (`redirect_to`).
    pub redirect: bool,
    /// Hote du domaine principal vise par la redirection.
    pub redirect_to: Option<String>,
    pub service_id: Option<String>,
    pub service_name: Option<String>,
    /// Port interne du service (requis pour mettre le domaine en ligne).
    pub internal_port: Option<u16>,
    pub dns_status: DnsStatus,
    /// Raison d'un DNS incorrect (adresses trouvees, adresse attendue).
    pub dns_error: Option<String>,
    pub cert_status: CertStatus,
    /// Fin de validite du certificat (RFC 3339).
    pub cert_expires_at: Option<String>,
    /// Nom de l'emetteur du certificat.
    pub cert_issuer: Option<String>,
    pub cert_error: Option<String>,
    /// RFC 3339.
    pub created_at: String,
}

/// Domaines d'un projet et ce qu'il faut pour configurer le DNS.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
pub struct ProjectDomains {
    /// IP publique du noeud (cible des enregistrements A/AAAA) ; absente si
    /// l'instance ne la connait pas (`CIPHERA_PUBLIC_IP`).
    pub node_ip: Option<String>,
    /// IPv6 du noeud (cible AAAA, `CIPHERA_PUBLIC_IP6`) quand `node_ip` est
    /// une IPv4 ; absente sinon.
    pub node_ip6: Option<String>,
    /// Domaine genere active pour ce projet.
    pub generated_domain: bool,
    pub domains: Vec<Domain>,
}

/// Corps de `POST /api/domains?project=`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NewDomain {
    /// Nom de domaine (FQDN) ; ni adresse IP, ni joker, ni port.
    pub host: String,
    /// Prefixe de chemin (`/` par defaut).
    #[serde(default)]
    pub path_prefix: Option<String>,
    /// Service servi (Compose : le service expose choisi). Absent : le
    /// service de l'environnement de production.
    #[serde(default)]
    pub service_id: Option<String>,
    /// Port interne du service : fixe ou confirme ici. Requis si le service
    /// n'en a pas encore (sauf redirection).
    #[serde(default)]
    pub internal_port: Option<u16>,
    /// Chemin du healthcheck du service (facultatif).
    #[serde(default)]
    pub healthcheck_path: Option<String>,
    /// Redirection 308 vers le domaine principal plutot qu'un service.
    #[serde(default)]
    pub redirect: bool,
    /// Ajoute aussi `www.<domaine>` (domaine apex), redirige en 308.
    #[serde(default)]
    pub www: bool,
}

/// Corps de `PATCH /api/domains/{id}`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateDomain {
    #[serde(default)]
    pub path_prefix: Option<String>,
    #[serde(default)]
    pub service_id: Option<String>,
    #[serde(default)]
    pub internal_port: Option<u16>,
    #[serde(default)]
    pub healthcheck_path: Option<String>,
}

/// Corps de `PATCH /api/domains?project=` : reglages du projet.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateProjectDomains {
    /// Active ou desactive le domaine genere.
    pub generated_domain: bool,
}

/// Corps de `POST /api/domains/{id}/redirect`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DomainRedirect {
    /// `true` : redirection 308 vers le domaine principal ; `false` : sert
    /// de nouveau son service.
    pub enabled: bool,
}

#[utoipa::path(
    get,
    path = "/api/domains",
    tag = "domains",
    params(("project" = String, Query, description = "Identifiant du projet")),
    responses(
        (status = 200, description = "Domaines du projet", body = ProjectDomains),
        (status = 403, description = "Role insuffisant"),
        (status = 404, description = "Projet introuvable"),
    )
)]
#[allow(dead_code)]
fn list_domains() {}

#[utoipa::path(
    post,
    path = "/api/domains",
    tag = "domains",
    params(("project" = String, Query, description = "Identifiant du projet")),
    request_body = NewDomain,
    responses(
        (status = 201, description = "Domaine ajoute (DNS a verifier)", body = Domain),
        (status = 403, description = "Role insuffisant (developer et plus)"),
        (status = 404, description = "Projet ou service introuvable"),
        (status = 409, description = "Hote deja utilise ou reserve au panneau"),
        (status = 422, description = "Nom invalide (sslip.io et nip.io compris), ou port interne manquant"),
    )
)]
#[allow(dead_code)]
fn create_domain() {}

#[utoipa::path(
    patch,
    path = "/api/domains",
    tag = "domains",
    params(("project" = String, Query, description = "Identifiant du projet")),
    request_body = UpdateProjectDomains,
    responses(
        (status = 200, description = "Domaines du projet", body = ProjectDomains),
        (status = 403, description = "Role insuffisant (developer et plus)"),
        (status = 404, description = "Projet introuvable"),
        (status = 422, description = "IP publique du noeud inconnue"),
    )
)]
#[allow(dead_code)]
fn update_project_domains() {}

#[utoipa::path(
    patch,
    path = "/api/domains/{id}",
    tag = "domains",
    params(("id" = String, Path, description = "Identifiant du domaine")),
    request_body = UpdateDomain,
    responses(
        (status = 200, description = "Domaine modifie", body = Domain),
        (status = 403, description = "Role insuffisant (developer et plus)"),
        (status = 404, description = "Domaine ou service introuvable"),
        (status = 422, description = "Valeur invalide, ou port interne manquant"),
    )
)]
#[allow(dead_code)]
fn update_domain() {}

#[utoipa::path(
    delete,
    path = "/api/domains/{id}",
    tag = "domains",
    params(("id" = String, Path, description = "Identifiant du domaine")),
    responses(
        (status = 204, description = "Domaine supprime"),
        (status = 403, description = "Role insuffisant (developer et plus)"),
        (status = 404, description = "Domaine introuvable"),
        (status = 409, description = "Domaine genere : le desactiver"),
    )
)]
#[allow(dead_code)]
fn delete_domain() {}

#[utoipa::path(
    post,
    path = "/api/domains/{id}/verify",
    tag = "domains",
    params(("id" = String, Path, description = "Identifiant du domaine")),
    responses(
        (status = 200, description = "Resultat de la verification (A/AAAA vers l'IP du noeud) ; DNS correct : en ligne, certificat demande", body = Domain),
        (status = 403, description = "Role insuffisant (developer et plus)"),
        (status = 404, description = "Domaine introuvable"),
        (status = 422, description = "Port interne manquant, ou IP du noeud inconnue"),
    )
)]
#[allow(dead_code)]
fn verify_domain() {}

#[utoipa::path(
    post,
    path = "/api/domains/{id}/renew",
    tag = "domains",
    params(("id" = String, Path, description = "Identifiant du domaine")),
    responses(
        (status = 202, description = "Emission ACME demandee", body = Domain),
        (status = 403, description = "Role insuffisant (developer et plus)"),
        (status = 404, description = "Domaine introuvable"),
        (status = 409, description = "DNS non verifie, ou certificat encore valide a plus de 30 jours"),
    )
)]
#[allow(dead_code)]
fn renew_domain() {}

#[utoipa::path(
    post,
    path = "/api/domains/{id}/primary",
    tag = "domains",
    params(("id" = String, Path, description = "Identifiant du domaine")),
    responses(
        (status = 200, description = "Domaines du projet (un seul principal par environnement)", body = ProjectDomains),
        (status = 403, description = "Role insuffisant (developer et plus)"),
        (status = 404, description = "Domaine introuvable"),
        (status = 409, description = "Domaine redirige ou genere (HTTP seulement)"),
    )
)]
#[allow(dead_code)]
fn primary_domain() {}

#[utoipa::path(
    post,
    path = "/api/domains/{id}/redirect",
    tag = "domains",
    params(("id" = String, Path, description = "Identifiant du domaine")),
    request_body = DomainRedirect,
    responses(
        (status = 200, description = "Domaine modifie", body = Domain),
        (status = 403, description = "Role insuffisant (developer et plus)"),
        (status = 404, description = "Domaine introuvable"),
        (status = 409, description = "Aucun autre domaine principal vers lequel rediriger"),
        (status = 422, description = "Port interne manquant pour servir de nouveau le service"),
    )
)]
#[allow(dead_code)]
fn redirect_domain() {}
