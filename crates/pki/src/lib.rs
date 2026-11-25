//! Autorite de certification d'instance : une CA par installation Ciphera,
//! qui signe le certificat du panneau et ceux des agents (mTLS).

use std::fmt;

use rand::TryRng;
use rand::rngs::SysRng;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose, PublicKeyData, SanType, SerialNumber, SubjectPublicKeyInfo,
    string::Ia5String,
};
use sha2::{Digest, Sha256};
use shared::ids::ServerId;
use time::{Duration, OffsetDateTime};
use zeroize::Zeroizing;

/// Prefixe du SAN URI qui porte l'identifiant d'un agent.
pub const AGENT_SAN_PREFIX: &str = "urn:ciphera:agent:";

const CA_VALIDITY_DAYS: i64 = 3650;
const SERVER_VALIDITY_DAYS: i64 = 730;
const AGENT_VALIDITY_DAYS: i64 = 365;

#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pki : {}", self.0)
    }
}

impl std::error::Error for Error {}

impl From<rcgen::Error> for Error {
    fn from(e: rcgen::Error) -> Self {
        Self(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Certificat emis. `key_pem` n'existe que si la CA a genere la cle (certificat
/// du panneau) ; pour un agent la cle ne quitte jamais sa machine.
#[derive(Clone)]
pub struct CertBundle {
    pub cert_pem: String,
    pub key_pem: Option<Zeroizing<String>>,
    /// SHA-256 du DER, en hexadecimal majuscule separe par `:`.
    pub fingerprint_sha256: String,
}

impl fmt::Debug for CertBundle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CertBundle")
            .field("fingerprint_sha256", &self.fingerprint_sha256)
            .field("key_pem", &self.key_pem.as_ref().map(|_| "***"))
            .finish_non_exhaustive()
    }
}

/// Empreinte SHA-256 d'un certificat DER : `AB:CD:...` (majuscules).
pub fn fingerprint_sha256(der: &[u8]) -> String {
    let digest = Sha256::digest(der);
    let mut out = String::with_capacity(digest.len() * 3 - 1);
    for (i, byte) in digest.iter().enumerate() {
        if i > 0 {
            out.push(':');
        }
        out.push_str(&format!("{byte:02X}"));
    }
    out
}

/// Identifiant d'agent porte par un certificat DER : exactement un SAN URI
/// `urn:ciphera:agent:<uuid>`. `None` si absent, multiple ou illisible.
pub fn agent_id_from_cert(der: &[u8]) -> Option<ServerId> {
    use x509_parser::extensions::GeneralName;
    use x509_parser::prelude::{FromDer, X509Certificate};

    let (rest, cert) = X509Certificate::from_der(der).ok()?;
    if !rest.is_empty() {
        return None;
    }
    let san = cert.subject_alternative_name().ok()??;
    let mut ids = san
        .value
        .general_names
        .iter()
        .filter_map(|name| match name {
            GeneralName::URI(uri) => uri.strip_prefix(AGENT_SAN_PREFIX),
            _ => None,
        });
    let id = ids.next()?;
    if ids.next().is_some() {
        return None;
    }
    id.parse().ok()
}

/// Adresse IP portee par un certificat PEM : exactement un SAN IP (celui du
/// panneau, `ciphera init --public-ip`). `None` si absent, multiple ou
/// illisible.
pub fn ip_from_cert_pem(pem: &str) -> Option<std::net::IpAddr> {
    match ips_from_cert_pem(pem).as_slice() {
        [ip] => Some(*ip),
        _ => None,
    }
}

/// Adresses IP des SAN d'un certificat PEM, dans l'ordre (vide si aucune ou
/// certificat illisible).
pub fn ips_from_cert_pem(pem: &str) -> Vec<std::net::IpAddr> {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use x509_parser::extensions::GeneralName;
    use x509_parser::prelude::{FromDer, X509Certificate};

    let Ok((_, pem)) = x509_parser::pem::parse_x509_pem(pem.as_bytes()) else {
        return Vec::new();
    };
    let Ok((_, cert)) = X509Certificate::from_der(&pem.contents) else {
        return Vec::new();
    };
    let Ok(Some(san)) = cert.subject_alternative_name() else {
        return Vec::new();
    };
    san.value
        .general_names
        .iter()
        .filter_map(|name| match name {
            GeneralName::IPAddress(bytes) => match bytes.len() {
                4 => <[u8; 4]>::try_from(*bytes)
                    .ok()
                    .map(|b| IpAddr::V4(Ipv4Addr::from(b))),
                16 => <[u8; 16]>::try_from(*bytes)
                    .ok()
                    .map(|b| IpAddr::V6(Ipv6Addr::from(b))),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// Paire de cles d'un agent (ECDSA P-256), generee sur la machine de l'agent.
/// Seule `public_key_der` est transmise a la CA.
pub struct AgentKey {
    key_pem: Zeroizing<String>,
    public_key_der: Vec<u8>,
}

impl fmt::Debug for AgentKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentKey").finish_non_exhaustive()
    }
}

impl AgentKey {
    pub fn generate() -> Result<AgentKey> {
        let key = KeyPair::generate()?;
        Ok(AgentKey {
            key_pem: Zeroizing::new(key.serialize_pem()),
            public_key_der: key.subject_public_key_info(),
        })
    }

    /// Cle privee PEM (PKCS#8) : a ecrire en 0600 chez l'agent seulement.
    pub fn key_pem(&self) -> &str {
        &self.key_pem
    }

    /// SubjectPublicKeyInfo DER, a envoyer a la CA.
    pub fn public_key_der(&self) -> &[u8] {
        &self.public_key_der
    }
}

pub struct Ca {
    issuer: Issuer<'static, KeyPair>,
    cert_pem: String,
}

impl fmt::Debug for Ca {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ca").finish_non_exhaustive()
    }
}

impl Ca {
    /// Nouvelle CA racine (ECDSA P-256), valable 10 ans.
    pub fn generate(name: &str) -> Result<Ca> {
        let mut params = CertificateParams::default();
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, name);
        params.distinguished_name = dn;
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        validity(&mut params, CA_VALIDITY_DAYS);
        params.serial_number = Some(random_serial()?);
        let key = KeyPair::generate()?;
        let cert_pem = params.self_signed(&key)?.pem();
        Ok(Ca {
            issuer: Issuer::new(params, key),
            cert_pem,
        })
    }

    /// CA existante (`ca/ca.crt` et `ca/ca.key` de `ciphera init`). La cle
    /// doit etre celle du certificat.
    pub fn load(cert_pem: &str, key_pem: &str) -> Result<Ca> {
        let key = KeyPair::from_pem(key_pem)?;
        let cert_key = der_from_pem(cert_pem)
            .and_then(|der| public_key_der(&der))
            .ok_or_else(|| Error("certificat de la CA illisible".into()))?;
        if cert_key != key.subject_public_key_info() {
            return Err(Error(
                "cle de la CA differente de celle de son certificat".into(),
            ));
        }
        let issuer = Issuer::from_ca_cert_pem(cert_pem, key)?;
        Ok(Ca {
            issuer,
            cert_pem: cert_pem.to_string(),
        })
    }

    /// Nouveau certificat d'agent pour la cle publique et l'identifiant du
    /// certificat `old_der` (renouvellement : la cle de l'agent ne change
    /// pas et ne quitte jamais sa machine).
    pub fn renew_agent_cert(&self, old_der: &[u8]) -> Result<CertBundle> {
        let id = agent_id_from_cert(old_der)
            .ok_or_else(|| Error("certificat d'agent sans identifiant".into()))?;
        let spki =
            public_key_der(old_der).ok_or_else(|| Error("certificat d'agent illisible".into()))?;
        self.issue_agent_cert(id, &spki)
    }

    /// Nouveau certificat serveur (nouvelle cle) avec les SAN (noms DNS et
    /// adresses IP) du certificat PEM `old_pem`.
    pub fn renew_server_cert(&self, old_pem: &str) -> Result<CertBundle> {
        let sans = server_sans(old_pem)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| Error("certificat serveur illisible ou sans SAN".into()))?;
        self.issue_server_cert(&sans)
    }

    /// Nouveau certificat serveur pour la cle publique et les SAN (noms DNS
    /// et adresses IP) du certificat PEM `old_pem` : la cle privee reste ou
    /// elle est (signataire root, `ciphera admin sign-requests`).
    pub fn reissue_server_cert(&self, old_pem: &str) -> Result<CertBundle> {
        let sans = server_sans(old_pem)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| Error("certificat serveur illisible ou sans SAN".into()))?;
        let spki = der_from_pem(old_pem)
            .and_then(|der| public_key_der(&der))
            .ok_or_else(|| Error("certificat serveur illisible".into()))?;
        let public_key = SubjectPublicKeyInfo::from_der(&spki)?;
        let params = server_params(&sans)?;
        let cert = params.signed_by(&public_key, &self.issuer)?;
        Ok(CertBundle {
            cert_pem: cert.pem(),
            key_pem: None,
            fingerprint_sha256: fingerprint_sha256(cert.der()),
        })
    }

    /// Certificat de la CA (public).
    pub fn cert_pem(&self) -> &str {
        &self.cert_pem
    }

    /// Cle privee de la CA, en PEM : a ecrire en 0600 puis a ne plus exposer.
    pub fn key_pem(&self) -> Zeroizing<String> {
        Zeroizing::new(self.issuer.key().serialize_pem())
    }

    /// Certificat serveur (TLS du panneau) avec sa propre cle. Chaque SAN est
    /// un nom DNS ou une adresse IP.
    pub fn issue_server_cert(&self, sans: &[String]) -> Result<CertBundle> {
        let params = server_params(sans)?;
        let key = KeyPair::generate()?;
        let cert = params.signed_by(&key, &self.issuer)?;
        Ok(CertBundle {
            cert_pem: cert.pem(),
            key_pem: Some(Zeroizing::new(key.serialize_pem())),
            fingerprint_sha256: fingerprint_sha256(cert.der()),
        })
    }

    /// Certificat client d'un agent, pour la cle publique qu'il fournit
    /// (SubjectPublicKeyInfo DER). La cle privee n'est jamais vue par la CA.
    pub fn issue_agent_cert(
        &self,
        agent_id: ServerId,
        public_key_der: &[u8],
    ) -> Result<CertBundle> {
        let now = OffsetDateTime::now_utc();
        self.agent_cert(
            agent_id,
            public_key_der,
            now - Duration::days(1),
            now + Duration::days(AGENT_VALIDITY_DAYS),
        )
    }

    /// Comme [`Ca::issue_agent_cert`], valable de `not_before` a `not_after`
    /// (secondes Unix ; simulation du temps : certificat proche de
    /// l'expiration).
    pub fn issue_agent_cert_between(
        &self,
        agent_id: ServerId,
        public_key_der: &[u8],
        not_before: i64,
        not_after: i64,
    ) -> Result<CertBundle> {
        let at =
            |t| OffsetDateTime::from_unix_timestamp(t).map_err(|_| Error("date invalide".into()));
        self.agent_cert(agent_id, public_key_der, at(not_before)?, at(not_after)?)
    }

    fn agent_cert(
        &self,
        agent_id: ServerId,
        public_key_der: &[u8],
        not_before: OffsetDateTime,
        not_after: OffsetDateTime,
    ) -> Result<CertBundle> {
        let public_key = SubjectPublicKeyInfo::from_der(public_key_der)?;
        let id = agent_id.to_string();
        let mut params = CertificateParams::default();
        params.distinguished_name = common_name(&id);
        params.subject_alt_names = vec![SanType::URI(
            Ia5String::try_from(format!("{AGENT_SAN_PREFIX}{id}")).map_err(Error::from)?,
        )];
        params.is_ca = IsCa::ExplicitNoCa;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        params.not_before = not_before;
        params.not_after = not_after;
        params.serial_number = Some(random_serial()?);
        let cert = params.signed_by(&public_key, &self.issuer)?;
        Ok(CertBundle {
            cert_pem: cert.pem(),
            key_pem: None,
            fingerprint_sha256: fingerprint_sha256(cert.der()),
        })
    }
}

/// Le certificat DER `der` est-il emis par la CA de certificat DER
/// `ca_der` : emetteur egal au sujet de la CA et signature verifiee par sa
/// cle publique. Les dates ne sont pas controlees (un certificat expire
/// reste reconnu, pour son renouvellement).
pub fn issued_by(ca_der: &[u8], der: &[u8]) -> bool {
    use x509_parser::prelude::{FromDer, X509Certificate};
    let (Ok((_, ca)), Ok((rest, cert))) = (
        X509Certificate::from_der(ca_der),
        X509Certificate::from_der(der),
    ) else {
        return false;
    };
    rest.is_empty()
        && cert.issuer() == ca.subject()
        && cert.verify_signature(Some(ca.public_key())).is_ok()
}

/// SubjectPublicKeyInfo DER d'un certificat DER.
pub fn public_key_der(der: &[u8]) -> Option<Vec<u8>> {
    use x509_parser::prelude::{FromDer, X509Certificate};
    let (_, cert) = X509Certificate::from_der(der).ok()?;
    Some(cert.public_key().raw.to_vec())
}

/// Debut et fin de validite d'un certificat DER (secondes Unix).
pub fn validity_unix(der: &[u8]) -> Option<(i64, i64)> {
    use x509_parser::prelude::{FromDer, X509Certificate};
    let (_, cert) = X509Certificate::from_der(der).ok()?;
    let v = cert.validity();
    Some((v.not_before.timestamp(), v.not_after.timestamp()))
}

/// Part de la validite apres laquelle un certificat est renouvele (2/3).
pub const RENEW_AFTER: (i64, i64) = (2, 3);

/// Le certificat DER doit-il etre renouvele a `now` (secondes Unix) : deux
/// tiers de sa validite ecoules (ou deja expire). `None` : illisible.
pub fn renewal_due(der: &[u8], now: i64) -> Option<bool> {
    let (from, to) = validity_unix(der)?;
    let at = from + (to - from) * RENEW_AFTER.0 / RENEW_AFTER.1;
    Some(now >= at)
}

/// Instant (secondes Unix) a partir duquel le certificat DER est a
/// renouveler.
pub fn renewal_at(der: &[u8]) -> Option<i64> {
    let (from, to) = validity_unix(der)?;
    Some(from + (to - from) * RENEW_AFTER.0 / RENEW_AFTER.1)
}

/// DER du premier certificat d'un PEM.
pub fn der_from_pem(pem: &str) -> Option<Vec<u8>> {
    let (_, pem) = x509_parser::pem::parse_x509_pem(pem.as_bytes()).ok()?;
    Some(pem.contents)
}

/// SAN DNS et IP d'un certificat PEM, dans l'ordre.
fn server_sans(pem: &str) -> Option<Vec<String>> {
    use x509_parser::extensions::GeneralName;
    use x509_parser::prelude::{FromDer, X509Certificate};
    let der = der_from_pem(pem)?;
    let (_, cert) = X509Certificate::from_der(&der).ok()?;
    let san = cert.subject_alternative_name().ok()??;
    let mut out = Vec::new();
    for name in &san.value.general_names {
        match name {
            GeneralName::DNSName(n) => out.push((*n).to_string()),
            GeneralName::IPAddress(_) => {}
            _ => return None,
        }
    }
    out.extend(ips_from_cert_pem(pem).iter().map(ToString::to_string));
    Some(out)
}

/// Parametres d'un certificat serveur : SAN, CA:false, signature
/// numerique, authentification serveur, 730 jours, numero aleatoire.
fn server_params(sans: &[String]) -> Result<CertificateParams> {
    let mut params = CertificateParams::new(sans.to_vec())?;
    params.distinguished_name = common_name(sans.first().map_or("ciphera", String::as_str));
    params.is_ca = IsCa::ExplicitNoCa;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    validity(&mut params, SERVER_VALIDITY_DAYS);
    params.serial_number = Some(random_serial()?);
    Ok(params)
}

fn common_name(name: &str) -> DistinguishedName {
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, name);
    dn
}

fn validity(params: &mut CertificateParams, days: i64) {
    // Debut un jour avant maintenant : tolere un decalage d'horloge.
    let now = OffsetDateTime::now_utc();
    params.not_before = now - Duration::days(1);
    params.not_after = now + Duration::days(days);
}

/// Numero de serie aleatoire de 16 octets, bit de poids fort efface (entier
/// positif sur 128 bits au plus). Sans cela rcgen le derive de la cle publique.
fn random_serial() -> Result<SerialNumber> {
    let mut bytes = [0u8; 16];
    SysRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| Error("generateur aleatoire du systeme indisponible".into()))?;
    bytes[0] &= 0x7f;
    Ok(SerialNumber::from_slice(&bytes))
}
