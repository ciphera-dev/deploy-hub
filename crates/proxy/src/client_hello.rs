//! Lecture du nom (SNI) d'un ClientHello TLS avant la poignee de main.
//!
//! Le port 443 ne sert un certificat qu'a un nom DNS demande par SNI : une
//! poignee de main sans SNI, ou avec une adresse IP en SNI, ne doit jamais
//! recevoir de certificat (celui du panneau reveleriait son domaine a qui
//! balaie les adresses IP). Le choix du certificat (`acme::CertResolver`)
//! la refuse deja, mais rustls repond alors par l'alerte `access_denied` ;
//! ce module permet de repondre avant rustls par l'alerte prevue pour ce
//! cas, `unrecognized_name` (RFC 6066, section 3).
//!
//! Seul un ClientHello complet dans un seul enregistrement est examine. Tout
//! autre cas (enregistrement fragmente, autre protocole, message mal forme)
//! est laisse a rustls, dont le refus reste la garantie.

use std::net::IpAddr;

/// Taille maximale du contenu d'un enregistrement TLS en clair (RFC 8446,
/// section 5.1).
pub const MAX_RECORD_LEN: usize = 1 << 14;

/// En-tete d'un enregistrement TLS.
pub const RECORD_HEADER_LEN: usize = 5;

/// Alerte fatale `unrecognized_name` (112), en clair : type 21 (alerte),
/// version 1.2, longueur 2, niveau 2 (fatal), description 112.
pub const UNRECOGNIZED_NAME_ALERT: [u8; 7] = [0x15, 0x03, 0x03, 0x00, 0x02, 0x02, 0x70];

/// Ce que le ClientHello demande.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HelloName {
    /// Nom DNS demande par SNI (tel que recu).
    Dns(String),
    /// Aucun nom DNS : SNI absent, adresse IP ou nom invalide.
    None,
    /// Rien a conclure ici : rustls decide.
    Undecided,
}

/// Longueur du contenu annoncee par l'en-tete d'enregistrement `header`
/// (5 octets), si c'est un enregistrement de poignee de main de taille
/// admise.
pub fn handshake_record_len(header: &[u8]) -> Option<usize> {
    if header.len() < RECORD_HEADER_LEN || header[0] != 0x16 || header[1] != 0x03 {
        return None;
    }
    let len = usize::from(u16::from_be_bytes([header[3], header[4]]));
    (len > 0 && len <= MAX_RECORD_LEN).then_some(len)
}

/// Lecteur borne : chaque lecture echoue proprement au-dela des donnees.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(head)
    }

    fn u8(&mut self) -> Option<usize> {
        self.take(1).map(|b| usize::from(b[0]))
    }

    fn u16(&mut self) -> Option<usize> {
        self.take(2)
            .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))
    }

    fn u24(&mut self) -> Option<usize> {
        self.take(3)
            .map(|b| (usize::from(b[0]) << 16) | (usize::from(b[1]) << 8) | usize::from(b[2]))
    }

    /// Bloc prefixe par sa longueur sur 1 ou 2 octets.
    fn block8(&mut self) -> Option<&'a [u8]> {
        let n = self.u8()?;
        self.take(n)
    }

    fn block16(&mut self) -> Option<&'a [u8]> {
        let n = self.u16()?;
        self.take(n)
    }
}

/// Nom demande par le ClientHello contenu dans `record` (contenu d'un seul
/// enregistrement de poignee de main, sans son en-tete).
pub fn requested_name(record: &[u8]) -> HelloName {
    parse(record).unwrap_or(HelloName::Undecided)
}

fn parse(record: &[u8]) -> Option<HelloName> {
    let mut r = Reader(record);
    // Message ClientHello (1), entier dans cet enregistrement.
    if r.u8()? != 1 {
        return None;
    }
    let len = r.u24()?;
    let mut hello = Reader(r.take(len)?);
    hello.take(2 + 32)?; // version, aleas
    hello.block8()?; // identifiant de session
    hello.block16()?; // suites
    hello.block8()?; // compression
    if hello.0.is_empty() {
        // Aucune extension, donc aucun SNI.
        return Some(HelloName::None);
    }
    let mut extensions = Reader(hello.block16()?);
    while !extensions.0.is_empty() {
        let kind = extensions.u16()?;
        let data = extensions.block16()?;
        if kind == 0 {
            return Some(server_name(data));
        }
    }
    Some(HelloName::None)
}

/// Extension `server_name` : premier nom de type `host_name` (0).
fn server_name(data: &[u8]) -> HelloName {
    let mut ext = Reader(data);
    let Some(list) = ext.block16() else {
        return HelloName::None;
    };
    let mut list = Reader(list);
    while !list.0.is_empty() {
        let (Some(kind), Some(name)) = (list.u8(), list.block16()) else {
            return HelloName::None;
        };
        if kind == 0 {
            return dns_name(name);
        }
    }
    HelloName::None
}

/// Un nom DNS en ASCII (point final admis), jamais une adresse IP.
fn dns_name(raw: &[u8]) -> HelloName {
    let Ok(name) = std::str::from_utf8(raw) else {
        return HelloName::None;
    };
    let bare = name.trim_end_matches('.');
    let valid = !bare.is_empty()
        && bare.len() <= 253
        && bare
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_'));
    let ip = bare.parse::<IpAddr>().is_ok()
        || bare
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
            .is_some_and(|s| s.parse::<IpAddr>().is_ok());
    if valid && !ip {
        HelloName::Dns(name.to_string())
    } else {
        HelloName::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ClientHello minimal avec les extensions donnees (type, donnees).
    fn hello(extensions: Option<&[(u16, Vec<u8>)]>) -> Vec<u8> {
        let mut body = vec![0x03, 0x03];
        body.extend([7u8; 32]);
        body.push(0); // session
        body.extend([0x00, 0x02, 0x13, 0x01]); // une suite
        body.extend([0x01, 0x00]); // compression nulle
        if let Some(exts) = extensions {
            let mut all = Vec::new();
            for (kind, data) in exts {
                all.extend(kind.to_be_bytes());
                all.extend((data.len() as u16).to_be_bytes());
                all.extend(data);
            }
            body.extend((all.len() as u16).to_be_bytes());
            body.extend(all);
        }
        let mut msg = vec![1];
        let n = body.len();
        msg.extend([(n >> 16) as u8, (n >> 8) as u8, n as u8]);
        msg.extend(body);
        msg
    }

    fn sni(name: &[u8]) -> (u16, Vec<u8>) {
        let mut entry = vec![0];
        entry.extend((name.len() as u16).to_be_bytes());
        entry.extend(name);
        let mut data = (entry.len() as u16).to_be_bytes().to_vec();
        data.extend(entry);
        (0, data)
    }

    #[test]
    fn dns_name_is_read_from_the_server_name_extension() {
        let alpn = (16u16, vec![0, 3, 2, b'h', b'2']);
        let rec = hello(Some(&[alpn, sni(b"Panel.Example.org.")]));
        assert_eq!(
            requested_name(&rec),
            HelloName::Dns("Panel.Example.org.".into())
        );
    }

    #[test]
    fn missing_sni_ip_or_invalid_name_means_no_name() {
        assert_eq!(requested_name(&hello(None)), HelloName::None);
        assert_eq!(requested_name(&hello(Some(&[]))), HelloName::None);
        let alpn = (16u16, vec![0, 3, 2, b'h', b'2']);
        assert_eq!(requested_name(&hello(Some(&[alpn]))), HelloName::None);
        for bad in [
            &b"203.0.113.7"[..],
            b"2001:db8::1",
            b"[2001:db8::1]",
            b"203.0.113.7.",
            b"",
            b"bad name",
            b"\xff\xfe",
        ] {
            assert_eq!(
                requested_name(&hello(Some(&[sni(bad)]))),
                HelloName::None,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn truncated_or_foreign_messages_are_left_to_rustls() {
        let rec = hello(Some(&[sni(b"panel.example.org")]));
        for cut in [0, 1, 4, 10, rec.len() - 1] {
            assert_eq!(requested_name(&rec[..cut]), HelloName::Undecided, "{cut}");
        }
        let mut server_hello = rec.clone();
        server_hello[0] = 2;
        assert_eq!(requested_name(&server_hello), HelloName::Undecided);
    }

    #[test]
    fn record_header_is_bounded() {
        assert_eq!(
            handshake_record_len(&[0x16, 0x03, 0x01, 0x02, 0x00]),
            Some(512)
        );
        assert_eq!(
            handshake_record_len(&[0x16, 0x03, 0x01, 0x40, 0x00]),
            Some(MAX_RECORD_LEN)
        );
        assert_eq!(handshake_record_len(&[0x16, 0x03, 0x01, 0x40, 0x01]), None);
        assert_eq!(handshake_record_len(&[0x16, 0x03, 0x01, 0x00, 0x00]), None);
        assert_eq!(handshake_record_len(b"GET /"), None);
        assert_eq!(handshake_record_len(&[0x16, 0x03]), None);
    }

    #[test]
    fn alert_is_a_fatal_unrecognized_name() {
        assert_eq!(UNRECOGNIZED_NAME_ALERT[0], 21);
        assert_eq!(&UNRECOGNIZED_NAME_ALERT[5..], &[2, 112]);
    }
}
