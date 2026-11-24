//! Healthcheck HTTP d'un nouveau conteneur : `GET <chemin>` en HTTP/1.1 sur
//! son adresse du reseau du projet, sans dependance (requete fixe, seule la
//! ligne de statut est lue).

use std::future::Future;
use std::net::SocketAddr;
use std::time::Duration;

use proto::Healthcheck;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Delai d'un essai (connexion, requete, ligne de statut).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// Octets lus au plus pour la ligne de statut.
const MAX_STATUS_LINE: usize = 256;

/// Un essai de healthcheck. `TcpProbe` en production ; simule dans les
/// tests.
pub trait HttpProbe: Send + Sync + 'static {
    /// `Ok` si `GET path` sur `addr` repond 2xx ou 3xx.
    fn check(
        &self,
        addr: SocketAddr,
        path: &str,
    ) -> impl Future<Output = Result<(), String>> + Send;
}

/// Chemin de healthcheck acceptable : absolu, ASCII visible sans espace
/// (il finit dans la ligne de requete : ni retour a la ligne ni espace),
/// `Healthcheck::MAX_PATH_LEN` octets au plus.
pub fn valid_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= Healthcheck::MAX_PATH_LEN
        && path.bytes().all(|b| b.is_ascii_graphic())
}

/// Reglages du healthcheck acceptables (chemin, nombre d'essais, intervalle).
pub fn valid_healthcheck(h: &Healthcheck) -> bool {
    valid_path(&h.path)
        && (1..=Healthcheck::MAX_TRIES).contains(&h.tries)
        && (1..=Healthcheck::MAX_INTERVAL_SECS).contains(&h.interval_secs)
}

/// Statut d'une ligne `HTTP/1.x NNN ...`.
pub fn parse_status(line: &[u8]) -> Option<u16> {
    let rest = line
        .strip_prefix(b"HTTP/1.1 ")
        .or_else(|| line.strip_prefix(b"HTTP/1.0 "))?;
    let code = rest.get(..3)?;
    if !code.iter().all(u8::is_ascii_digit)
        || rest.get(3).is_some_and(|b| *b != b' ' && *b != b'\r')
    {
        return None;
    }
    std::str::from_utf8(code).ok()?.parse().ok()
}

/// Healthcheck par TCP.
#[derive(Clone, Copy, Debug)]
pub struct TcpProbe {
    pub timeout: Duration,
}

impl Default for TcpProbe {
    fn default() -> Self {
        Self {
            timeout: PROBE_TIMEOUT,
        }
    }
}

impl TcpProbe {
    async fn status(addr: SocketAddr, path: &str) -> Result<u16, String> {
        if !valid_path(path) {
            return Err("chemin de healthcheck refuse".into());
        }
        let mut stream = TcpStream::connect(addr)
            .await
            .map_err(|e| format!("connexion : {e}"))?;
        // Hote fixe : jamais une valeur venue d'ailleurs dans les en-tetes.
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: localhost\r\nUser-Agent: ciphera-agent\r\n\
             Accept: */*\r\nConnection: close\r\n\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|e| format!("envoi : {e}"))?;
        let mut line = Vec::with_capacity(64);
        let mut buf = [0u8; 64];
        while !line.contains(&b'\n') && line.len() < MAX_STATUS_LINE {
            let n = stream
                .read(&mut buf)
                .await
                .map_err(|e| format!("reponse : {e}"))?;
            if n == 0 {
                break;
            }
            line.extend_from_slice(&buf[..n]);
        }
        parse_status(&line).ok_or_else(|| "reponse HTTP illisible".to_string())
    }
}

impl HttpProbe for TcpProbe {
    async fn check(&self, addr: SocketAddr, path: &str) -> Result<(), String> {
        let status = tokio::time::timeout(self.timeout, Self::status(addr, path))
            .await
            .map_err(|_| "delai depasse".to_string())??;
        if (200..400).contains(&status) {
            Ok(())
        } else {
            Err(format!("statut HTTP {status}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_lines_are_parsed() {
        assert_eq!(parse_status(b"HTTP/1.1 200 OK\r\n"), Some(200));
        assert_eq!(parse_status(b"HTTP/1.0 302 Found\r\n"), Some(302));
        assert_eq!(parse_status(b"HTTP/1.1 503\r\n"), Some(503));
        for bad in [
            &b""[..],
            b"HTTP/2 200",
            b"HTTP/1.1 2x0 OK",
            b"HTTP/1.1 2000",
        ] {
            assert_eq!(parse_status(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn paths_and_settings_are_checked() {
        assert!(valid_path("/"));
        assert!(valid_path("/healthz?full=1"));
        for bad in ["", "healthz", "/a b", "/a\r\nHost: x", "/\u{e9}"] {
            assert!(!valid_path(bad), "{bad:?}");
        }
        assert!(!valid_path(&format!(
            "/{}",
            "a".repeat(Healthcheck::MAX_PATH_LEN)
        )));
        assert!(valid_healthcheck(&Healthcheck::default()));
        let h = |tries, interval_secs| Healthcheck {
            path: "/".into(),
            tries,
            interval_secs,
        };
        assert!(!valid_healthcheck(&h(0, 5)));
        assert!(!valid_healthcheck(&h(31, 5)));
        assert!(!valid_healthcheck(&h(5, 0)));
        assert!(!valid_healthcheck(&h(5, 61)));
    }

    #[tokio::test]
    async fn probe_reads_the_status_of_a_real_server() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for reply in [
                "HTTP/1.1 204 No Content\r\n\r\n",
                "HTTP/1.1 500 Oops\r\n\r\n",
            ] {
                let (mut s, _) = listener.accept().await.unwrap();
                let mut req = vec![0u8; 1024];
                let n = s.read(&mut req).await.unwrap();
                let req = String::from_utf8_lossy(&req[..n]).into_owned();
                assert!(req.starts_with("GET /healthz HTTP/1.1\r\n"), "{req}");
                assert!(req.contains("Host: localhost\r\n"), "{req}");
                s.write_all(reply.as_bytes()).await.unwrap();
            }
        });
        let probe = TcpProbe::default();
        probe.check(addr, "/healthz").await.unwrap();
        let err = probe.check(addr, "/healthz").await.unwrap_err();
        assert!(err.contains("500"), "{err}");
        server.await.unwrap();
        // Personne n'ecoute : echec, sans attendre le delai entier.
        assert!(probe.check(addr, "/healthz").await.is_err());
    }
}
