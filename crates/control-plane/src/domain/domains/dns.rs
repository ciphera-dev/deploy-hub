//! Resolution DNS des domaines (verification A/AAAA vers l'IP du noeud).
//! Injectable : les tests fournissent leur propre resolveur.

use std::future::Future;
use std::io;
use std::net::IpAddr;
use std::pin::Pin;
use std::time::Duration;

/// Delai maximal d'une resolution.
pub const RESOLVE_TIMEOUT: Duration = Duration::from_secs(5);

/// Adresses (A et AAAA) d'un nom.
pub trait DnsResolver: Send + Sync {
    fn resolve<'a>(
        &'a self,
        host: &'a str,
    ) -> Pin<Box<dyn Future<Output = io::Result<Vec<IpAddr>>> + Send + 'a>>;
}

/// Resolveur du systeme (`getaddrinfo`), borne par [`RESOLVE_TIMEOUT`].
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemResolver;

impl DnsResolver for SystemResolver {
    fn resolve<'a>(
        &'a self,
        host: &'a str,
    ) -> Pin<Box<dyn Future<Output = io::Result<Vec<IpAddr>>> + Send + 'a>> {
        Box::pin(async move {
            let found = tokio::time::timeout(RESOLVE_TIMEOUT, tokio::net::lookup_host((host, 0)))
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "delai depasse"))?;
            let mut ips: Vec<IpAddr> = match found {
                Ok(addrs) => addrs.map(|a| a.ip()).collect(),
                // Nom inconnu : aucune adresse (verdict negatif). Toute autre
                // erreur est une panne du resolveur, jamais un verdict.
                Err(e) if unknown_name(&e) => Vec::new(),
                Err(e) => return Err(e),
            };
            ips.sort();
            ips.dedup();
            Ok(ips)
        })
    }
}

/// L'erreur de `getaddrinfo` dit-elle que le nom n'existe pas (ou n'a pas
/// d'adresse), par opposition a une panne (serveur DNS injoignable, reponse
/// temporaire) ?
fn unknown_name(e: &io::Error) -> bool {
    // WSAHOST_NOT_FOUND, WSANO_DATA (Windows).
    if matches!(e.raw_os_error(), Some(11001 | 11004)) {
        return true;
    }
    let text = e.to_string().to_ascii_lowercase();
    [
        "name or service not known",
        "no address associated",
        "name does not resolve",
        "nodename nor servname",
        "no data",
    ]
    .iter()
    .any(|m| text.contains(m))
}

/// Verdict de la verification : `None` si le nom pointe vers le noeud,
/// sinon la raison. **Chaque** enregistrement A et AAAA doit etre l'IP du
/// noeud de sa famille (`v4`, `v6`) : une autre adresse recevrait une partie
/// du trafic, et Let's Encrypt essaie l'IPv6 d'abord quand un AAAA existe.
/// Un AAAA sans IPv6 du noeud configuree (ou un A sans IPv4) est refuse.
pub fn verdict(
    host: &str,
    v4: Option<IpAddr>,
    v6: Option<IpAddr>,
    found: &[IpAddr],
) -> Option<String> {
    let list = |ips: &[&IpAddr]| {
        ips.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    if found.is_empty() {
        let (record, target) = match (v4, v6) {
            (Some(ip), _) => ("A", ip),
            (None, Some(ip)) => ("AAAA", ip),
            (None, None) => return Some("IP publique du nœud inconnue.".into()),
        };
        return Some(format!(
            "Aucun enregistrement {record} pour {host} : ajoutez un enregistrement {record} vers {target}."
        ));
    }
    for (record, node, family) in [("A", v4, true), ("AAAA", v6, false)] {
        let records: Vec<&IpAddr> = found.iter().filter(|ip| ip.is_ipv4() == family).collect();
        if records.is_empty() {
            continue;
        }
        let Some(node) = node else {
            let version = if family { "IPv4" } else { "IPv6" };
            return Some(format!(
                "{host} a un enregistrement {record} ({}) : supprimez l'enregistrement {record} ou configurez l'{version} du nœud.",
                list(&records)
            ));
        };
        let wrong: Vec<&IpAddr> = records.into_iter().filter(|ip| **ip != node).collect();
        if !wrong.is_empty() {
            return Some(format!(
                "{host} pointe vers {} au lieu de {node} : corrigez l'enregistrement {record}.",
                list(&wrong)
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ips(list: &[&str]) -> Vec<IpAddr> {
        list.iter().map(|i| i.parse().unwrap()).collect()
    }

    #[test]
    fn only_an_unknown_name_is_a_verdict_other_errors_are_outages() {
        let other = |m: &str| io::Error::other(m.to_string());
        for unknown in [
            "failed to lookup address information: Name or service not known",
            "failed to lookup address information: No address associated with hostname",
            "failed to lookup address information: nodename nor servname provided, or not known",
            "failed to lookup address information: Name does not resolve",
        ] {
            assert!(unknown_name(&other(unknown)), "{unknown}");
        }
        for outage in [
            "failed to lookup address information: Temporary failure in name resolution",
            "failed to lookup address information: Servname not supported",
            "connection refused",
        ] {
            assert!(!unknown_name(&other(outage)), "{outage}");
        }
        assert!(unknown_name(&io::Error::from_raw_os_error(11001)));
        assert!(!unknown_name(&io::Error::from(io::ErrorKind::TimedOut)));
    }

    #[test]
    fn verdict_requires_every_a_and_aaaa_record_to_be_a_node_ip() {
        let v4: IpAddr = "203.0.113.7".parse().unwrap();
        let v6: IpAddr = "2001:db8::7".parse().unwrap();
        let only4 = |found: &[&str]| verdict("a.example", Some(v4), None, &ips(found));
        assert_eq!(only4(&["203.0.113.7"]), None);
        // AAAA sans IPv6 du noeud : refuse, avec la marche a suivre.
        let aaaa = only4(&["203.0.113.7", "2001:db8::1"]).unwrap();
        assert!(
            aaaa.contains("supprimez l'enregistrement AAAA ou configurez l'IPv6 du nœud"),
            "{aaaa}"
        );
        assert!(aaaa.contains("2001:db8::1"), "{aaaa}");
        let wrong = only4(&["203.0.113.7", "198.51.100.1"]).unwrap();
        assert!(wrong.contains("198.51.100.1") && wrong.contains("203.0.113.7"));
        let none = only4(&[]).unwrap();
        assert!(none.contains("Aucun enregistrement A"), "{none}");

        // Noeud double pile : chaque enregistrement doit viser son IP.
        let both = |found: &[&str]| verdict("a.example", Some(v4), Some(v6), &ips(found));
        assert_eq!(both(&["203.0.113.7", "2001:db8::7"]), None);
        assert_eq!(both(&["2001:db8::7"]), None);
        let bad6 = both(&["203.0.113.7", "2001:db8::1"]).unwrap();
        assert!(
            bad6.contains("2001:db8::1") && bad6.contains("AAAA"),
            "{bad6}"
        );

        // Noeud IPv6 seulement : un enregistrement A est refuse.
        let a = verdict(
            "a.example",
            None,
            Some(v6),
            &ips(&["2001:db8::7", "203.0.113.7"]),
        )
        .unwrap();
        assert!(a.contains("enregistrement A"), "{a}");
    }
}
