//! Plafonds de connexions clientes (global et par IP) et bornes de temps
//! (premier octet, poignee de main TLS, lecture des en-tetes), contre
//! l'epuisement des descripteurs et de la memoire (slowloris).
//!
//! Pingora ne previent pas a la fermeture d'une connexion : le filtre de
//! connexion (`ConnectionFilter`, appele juste apres `accept`, avant TLS)
//! admet ou refuse, et l'application qui enveloppe le proxy (`Capped`)
//! compte la connexion active jusqu'a la fin de son traitement.
//!
//! Entre les deux, une connexion admise est « en attente ». En HTTP, elle
//! devient active aussitot (l'attente expire apres 5 s si la tache n'a pas
//! demarre). En HTTPS, elle attend sa poignee de main TLS, bornee par
//! `HandshakeDeadline` (10 s, au lieu des 60 s fixes de Pingora) : passe ce
//! delai, la connexion est coupee. Une poignee de main ratee n'a aucun
//! rappel qui la libere : l'admission expire apres le delai de poignee de
//! main plus 1 s (11 s). Ces attentes TLS ont leur propre plafond global
//! (`max_pending_tls`, 1024) et par IP (`max_pending_tls_per_ip`, 32), et ne
//! consomment pas la capacite des connexions actives : un flot de poignees
//! de main ratees ne bloque jamais le port 80, et ne bloque une IP (un NAT)
//! que pour ses nouvelles connexions TLS et pendant 11 s au plus. Les IPv6
//! sont regroupees par /64.
//!
//! Plafond global plein (32 sources de 32 attentes suffisent, un /56 IPv6
//! en a 256) : le nouveau venu n'est pas refuse, la plus ancienne poignee de
//! main surveillee est coupee et son admission liberee. Pour priver les
//! autres clients de TLS, un attaquant doit donc ouvrir plus de 1 024
//! poignees de main pendant la duree d'une poignee de main legitime (bien
//! moins d'une seconde), au lieu d'une centaine par seconde.
//!
//! Lecture des en-tetes : Pingora n'a qu'un delai entre deux lectures (60 s)
//! et accepte jusqu'a 1 Mio d'en-tetes. `Capped` borne la duree totale de
//! l'attente d'une requete, en-tetes compris (`header_timeout`, 20 s, depuis
//! l'ouverture de la connexion ou la fin de la reponse precedente) : la
//! connexion est coupee si les en-tetes ne sont pas tous lus a l'echeance.
//! Le proxy (`Forwarder::early_request_filter`) desarme l'echeance des que
//! les en-tetes sont lus : un corps ou une reponse lents ne sont pas
//! concernes. Le plafond de 1 Mio de Pingora est garde : une connexion ne
//! retient donc pas plus de 1 Mio pendant plus de 20 s.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::os::fd::{AsRawFd, BorrowedFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use pingora::apps::ServerApp;
use pingora::listeners::{ConnectionFilter, PreTlsProcess};
use pingora::protocols::l4::stream::Stream as L4Stream;
use pingora::protocols::{ALPN, GetSocketDigest, Stream, UniqueID, UniqueIDType};
use pingora::server::ShutdownWatch;
use tokio::sync::oneshot;

/// Connexions clientes simultanees, toutes IP confondues (une connexion
/// transferee occupe deux descripteurs ; `LimitNOFILE=65536`).
pub const MAX_CONNECTIONS: usize = 16_384;
/// Connexions simultanees d'une meme IP (ou d'un meme /64 IPv6).
pub const MAX_CONNECTIONS_PER_IP: usize = 256;
/// Poignees de main TLS en cours ou ratees (non liberees), toutes IP
/// confondues ; en plus de `MAX_CONNECTIONS`.
pub const MAX_PENDING_TLS: usize = 1024;
/// Poignees de main TLS en cours ou ratees d'une meme IP (ou /64), dans la
/// limite de `MAX_CONNECTIONS_PER_IP`.
pub const MAX_PENDING_TLS_PER_IP: usize = 32;
/// Delai laisse au client pour envoyer son premier octet.
pub const FIRST_BYTE_TIMEOUT: Duration = Duration::from_secs(10);
/// Duree totale d'une poignee de main TLS.
pub const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Duree totale de l'attente d'une requete, jusqu'a la fin de ses en-tetes.
pub const HEADER_TIMEOUT: Duration = Duration::from_secs(20);
/// Marge ajoutee au delai de poignee de main avant d'oublier une admission
/// TLS jamais terminee.
const PENDING_TLS_MARGIN: Duration = Duration::from_secs(1);
/// Duree de vie d'une admission HTTP pas encore prise en charge.
const PENDING_PLAIN_TTL: Duration = Duration::from_secs(5);
/// Intervalle minimal entre deux purges des admissions expirees.
const SWEEP_EVERY: Duration = Duration::from_secs(1);

/// Plafonds appliques par le proxy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnLimits {
    pub max_connections: usize,
    pub max_connections_per_ip: usize,
    pub max_pending_tls: usize,
    pub max_pending_tls_per_ip: usize,
    pub first_byte_timeout: Duration,
    pub tls_handshake_timeout: Duration,
    pub header_timeout: Duration,
}

impl Default for ConnLimits {
    fn default() -> Self {
        Self {
            max_connections: MAX_CONNECTIONS,
            max_connections_per_ip: MAX_CONNECTIONS_PER_IP,
            max_pending_tls: MAX_PENDING_TLS,
            max_pending_tls_per_ip: MAX_PENDING_TLS_PER_IP,
            first_byte_timeout: FIRST_BYTE_TIMEOUT,
            tls_handshake_timeout: TLS_HANDSHAKE_TIMEOUT,
            header_timeout: HEADER_TIMEOUT,
        }
    }
}

impl ConnLimits {
    fn ttl(&self, kind: Kind) -> Duration {
        match kind {
            Kind::Plain => PENDING_PLAIN_TTL,
            Kind::Tls => self.tls_handshake_timeout + PENDING_TLS_MARGIN,
        }
    }
}

/// Ecouteur d'une connexion : HTTP en clair ou TLS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Plain,
    Tls,
}

/// Cle de comptage : l'IPv4 (ou IPv4 mappee) telle quelle, l'IPv6 par /64.
fn bucket(ip: IpAddr) -> IpAddr {
    match ip.to_canonical() {
        IpAddr::V6(v6) => {
            let s = v6.segments();
            IpAddr::V6(Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
        }
        v4 => v4,
    }
}

#[derive(Default)]
struct Counts {
    active: usize,
    plain: VecDeque<Instant>,
    tls: VecDeque<Instant>,
}

impl Counts {
    fn total(&self) -> usize {
        self.active + self.plain.len() + self.tls.len()
    }

    fn queue(&mut self, kind: Kind) -> &mut VecDeque<Instant> {
        match kind {
            Kind::Plain => &mut self.plain,
            Kind::Tls => &mut self.tls,
        }
    }

    /// Retire les admissions expirees ; renvoie leur nombre par sorte.
    fn expire(&mut self, now: Instant, limits: &ConnLimits) -> (usize, usize) {
        let mut gone = (0, 0);
        for (kind, n) in [(Kind::Plain, &mut gone.0), (Kind::Tls, &mut gone.1)] {
            let ttl = limits.ttl(kind);
            let q = self.queue(kind);
            while q.front().is_some_and(|t| now.duration_since(*t) >= ttl) {
                q.pop_front();
                *n += 1;
            }
        }
        gone
    }
}

struct State {
    per_ip: HashMap<IpAddr, Counts>,
    active: usize,
    pending_plain: usize,
    pending_tls: usize,
    last_sweep: Instant,
}

impl State {
    fn forget(&mut self, (plain, tls): (usize, usize)) {
        self.pending_plain -= plain;
        self.pending_tls -= tls;
    }
}

/// Poignee de main TLS surveillee.
struct Handshake {
    serial: u64,
    /// Utilisee ou fermee : la tache d'echeance s'arrete sans couper.
    done: oneshot::Sender<()>,
    /// Double du descripteur : `shutdown` fait echouer la poignee de main.
    socket: Arc<std::net::TcpStream>,
    /// Cle de comptage de la source (son admission) ; sans adresse
    /// (aucune admission comptee), la poignee de main n'est pas evincable.
    key: Option<IpAddr>,
}

/// Echeances par connexion (cle : descripteur, unique parmi les connexions
/// ouvertes) : poignees de main TLS surveillees (et leur ordre d'arrivee,
/// par numero de serie) et en-tetes lus.
#[derive(Default)]
struct Watches {
    next: u64,
    handshakes: HashMap<UniqueIDType, Handshake>,
    order: BTreeMap<u64, UniqueIDType>,
    headers: HashMap<UniqueIDType, Arc<AtomicBool>>,
}

impl Watches {
    fn take(&mut self, id: UniqueIDType) -> Option<Handshake> {
        let h = self.handshakes.remove(&id)?;
        self.order.remove(&h.serial);
        Some(h)
    }

    /// Retire la plus ancienne poignee de main surveillee et evincable
    /// (source connue).
    fn take_oldest(&mut self) -> Option<Handshake> {
        let (_, id) = self.order.pop_first()?;
        self.handshakes.remove(&id)
    }
}

/// Compteurs partages par les filtres et l'application.
pub struct ConnLimiter {
    limits: ConnLimits,
    state: Mutex<State>,
    watches: Mutex<Watches>,
}

impl std::fmt::Debug for ConnLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnLimiter")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl ConnLimiter {
    pub fn new(limits: ConnLimits) -> Self {
        Self {
            limits,
            state: Mutex::new(State {
                per_ip: HashMap::new(),
                active: 0,
                pending_plain: 0,
                pending_tls: 0,
                last_sweep: Instant::now(),
            }),
            watches: Mutex::new(Watches::default()),
        }
    }

    pub fn limits(&self) -> ConnLimits {
        self.limits
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // Les compteurs restent coherents meme si un detenteur a panique.
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn watches(&self) -> MutexGuard<'_, Watches> {
        self.watches.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Admission d'une connexion qui vient d'etre acceptee sur un ecouteur
    /// de sorte `kind`.
    pub fn admit(&self, ip: IpAddr, kind: Kind) -> bool {
        let now = Instant::now();
        let limits = &self.limits;
        let mut guard = self.lock();
        let st = &mut *guard;
        if now.duration_since(st.last_sweep) >= SWEEP_EVERY {
            let mut gone = (0, 0);
            st.per_ip.retain(|_, c| {
                let (p, t) = c.expire(now, limits);
                gone.0 += p;
                gone.1 += t;
                c.total() > 0
            });
            st.forget(gone);
            st.last_sweep = now;
        }
        let key = bucket(ip);
        let counts = st.per_ip.entry(key).or_default();
        let gone = counts.expire(now, limits);
        let ip_total = counts.total();
        let ip_tls = counts.tls.len();
        st.forget(gone);
        // Les attentes TLS ne comptent pas dans la capacite des actives.
        let mut full = st.active + st.pending_plain >= limits.max_connections
            || ip_total >= limits.max_connections_per_ip
            || (kind == Kind::Tls && ip_tls >= limits.max_pending_tls_per_ip);
        if !full && kind == Kind::Tls && st.pending_tls >= limits.max_pending_tls {
            // Plafond global des attentes TLS : la plus ancienne poignee de
            // main surveillee cede sa place (une seule par admission).
            if let Some(old) = self.evict_oldest_handshake()
                && let Some(c) = st.per_ip.get_mut(&old)
            {
                if c.tls.pop_front().is_some() {
                    st.pending_tls -= 1;
                }
                if c.total() == 0 && old != key {
                    st.per_ip.remove(&old);
                }
            }
            full = st.pending_tls >= limits.max_pending_tls;
        }
        let counts = st.per_ip.entry(key).or_default();
        if full {
            if counts.total() == 0 {
                st.per_ip.remove(&key);
            }
            return false;
        }
        counts.queue(kind).push_back(now);
        match kind {
            Kind::Plain => st.pending_plain += 1,
            Kind::Tls => st.pending_tls += 1,
        }
        true
    }

    /// Debut du traitement d'une connexion admise (poignee de main faite) :
    /// elle passe d'« en attente » a active jusqu'a la fin du garde.
    pub fn enter(self: &Arc<Self>, ip: IpAddr, kind: Kind) -> ConnGuard {
        let key = bucket(ip);
        let mut st = self.lock();
        let counts = st.per_ip.entry(key).or_default();
        let promoted = counts.queue(kind).pop_front().is_some();
        counts.active += 1;
        if promoted {
            match kind {
                Kind::Plain => st.pending_plain -= 1,
                Kind::Tls => st.pending_tls -= 1,
            }
        }
        st.active += 1;
        ConnGuard {
            limiter: self.clone(),
            key,
        }
    }

    /// Poignees de main TLS en attente, toutes IP confondues.
    pub fn pending_tls(&self) -> usize {
        self.lock().pending_tls
    }

    /// Connexions comptees pour cette IP (actives et en attente).
    pub fn count(&self, ip: IpAddr) -> usize {
        self.lock().per_ip.get(&bucket(ip)).map_or(0, Counts::total)
    }

    /// Surveille la poignee de main TLS de la connexion `id` : passe le
    /// delai, `socket` (un double du descripteur) est coupe dans les deux
    /// sens, ce qui fait echouer la poignee de main en cours.
    /// `peer` : adresse de la source (cle de son admission, absente si le
    /// filtre n'a rien compte).
    fn watch_handshake(
        self: &Arc<Self>,
        id: UniqueIDType,
        socket: std::net::TcpStream,
        peer: Option<IpAddr>,
    ) {
        let (tx, rx) = oneshot::channel();
        let socket = Arc::new(socket);
        let serial = {
            let mut w = self.watches();
            w.next += 1;
            let serial = w.next;
            // Un ancien enregistrement du meme descripteur appartient a une
            // connexion deja fermee : sa surveillance s'arrete.
            w.take(id);
            w.handshakes.insert(
                id,
                Handshake {
                    serial,
                    done: tx,
                    socket: socket.clone(),
                    key: peer.map(bucket),
                },
            );
            if peer.is_some() {
                w.order.insert(serial, id);
            }
            serial
        };
        let limiter = self.clone();
        let timeout = self.limits.tls_handshake_timeout;
        tokio::spawn(async move {
            // `Ok` : poignee de main terminee, surveillance remplacee ou
            // poignee de main deja coupee (eviction).
            if tokio::time::timeout(timeout, rx).await.is_err() {
                let _ = socket.shutdown(std::net::Shutdown::Both);
                tracing::debug!("poignee de main TLS trop longue : connexion coupee");
                let mut w = limiter.watches();
                if w.handshakes.get(&id).is_some_and(|h| h.serial == serial) {
                    w.take(id);
                }
            }
        });
    }

    /// Coupe la plus ancienne poignee de main surveillee ; renvoie la cle
    /// de sa source.
    fn evict_oldest_handshake(&self) -> Option<IpAddr> {
        let h = self.watches().take_oldest()?;
        let _ = h.socket.shutdown(std::net::Shutdown::Both);
        tracing::debug!("plafond des poignees de main TLS : la plus ancienne est coupee");
        h.key
    }

    /// Poignee de main de la connexion `id` terminee : plus d'echeance.
    fn handshake_done(&self, id: UniqueIDType) {
        if let Some(h) = self.watches().take(id) {
            let _ = h.done.send(());
        }
    }

    /// Nouvelle attente de requete sur la connexion `id`.
    fn arm_headers(&self, id: UniqueIDType) -> Arc<AtomicBool> {
        let read = Arc::new(AtomicBool::new(false));
        self.watches().headers.insert(id, read.clone());
        read
    }

    fn disarm_headers(&self, id: UniqueIDType) {
        self.watches().headers.remove(&id);
    }

    /// En-tetes de la requete en cours sur la connexion `id` entierement
    /// lus (appele par le proxy) : l'echeance de lecture ne s'applique plus.
    pub fn headers_read(&self, id: UniqueIDType) {
        if let Some(read) = self.watches().headers.get(&id) {
            read.store(true, Ordering::Release);
        }
    }
}

/// Connexion active ; liberee a la destruction.
pub struct ConnGuard {
    limiter: Arc<ConnLimiter>,
    key: IpAddr,
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        let mut st = self.limiter.lock();
        st.active = st.active.saturating_sub(1);
        if let Some(c) = st.per_ip.get_mut(&self.key) {
            c.active = c.active.saturating_sub(1);
            if c.total() == 0 {
                st.per_ip.remove(&self.key);
            }
        }
    }
}

/// Filtre Pingora d'un service (HTTP ou HTTPS) : refuse la connexion
/// au-dela des plafonds.
#[derive(Debug)]
pub struct LimitFilter {
    pub limiter: Arc<ConnLimiter>,
    pub kind: Kind,
}

#[async_trait]
impl ConnectionFilter for LimitFilter {
    async fn should_accept(&self, addr: Option<&SocketAddr>) -> bool {
        match addr {
            Some(addr) => {
                let ok = self.limiter.admit(addr.ip(), self.kind);
                if !ok {
                    tracing::debug!(ip = %addr.ip(), kind = ?self.kind, "connexion refusee : plafond atteint");
                }
                ok
            }
            None => true,
        }
    }
}

/// Rappel Pingora appele juste avant la poignee de main TLS : la borne a
/// `tls_handshake_timeout` (Pingora la borne sinon a 60 s, fixe).
#[derive(Debug)]
pub struct HandshakeDeadline {
    pub limiter: Arc<ConnLimiter>,
}

#[async_trait]
impl PreTlsProcess for HandshakeDeadline {
    async fn process(&self, stream: &mut L4Stream) -> pingora::Result<()> {
        let fd = stream.as_raw_fd();
        // SAFETY : `fd` est le descripteur de `stream`, ouvert pendant tout
        // cet appel (emprunt mutable du flux) ; il n'est que duplique ici.
        let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
        // Le double garde le socket ouvert apres un echec de la poignee de
        // main, au plus jusqu'a l'echeance : il ne peut donc jamais designer
        // une autre connexion. Sans double, pas de borne (60 s de Pingora).
        //
        // Source : l'adresse gardee par Pingora a `accept` (celle passee au
        // filtre d'admission), jamais `getpeername`, qui echoue (ENOTCONN)
        // sur une connexion deja reinitialisee par le client : sans
        // surveillance, son admission ne serait ni evincable ni liberee
        // avant 11 s.
        let peer = stream
            .get_socket_digest()
            .and_then(|d| d.peer_addr().and_then(|a| a.as_inet()).map(|a| a.ip()));
        match borrowed.try_clone_to_owned() {
            Ok(owned) => {
                self.limiter
                    .watch_handshake(stream.id(), std::net::TcpStream::from(owned), peer)
            }
            Err(e) => tracing::warn!("poignee de main TLS non bornee : {e}"),
        }
        Ok(())
    }
}

/// Application Pingora enveloppee : compte la connexion pendant tout son
/// traitement (requetes successives comprises), ferme celle qui n'envoie
/// rien dans le delai du premier octet et celle dont les en-tetes ne sont
/// pas lus a l'echeance.
pub struct Capped<A> {
    inner: Arc<A>,
    limiter: Arc<ConnLimiter>,
}

impl<A> Capped<A> {
    pub fn new(inner: A, limiter: Arc<ConnLimiter>) -> Self {
        Self {
            inner: Arc::new(inner),
            limiter,
        }
    }
}

/// Protocole negocie HTTP/2 : pas d'echeance d'en-tetes par connexion.
fn is_h2(alpn: Option<ALPN>) -> bool {
    matches!(alpn, Some(ALPN::H2))
}

/// Echeance de lecture des en-tetes : ne se termine que si `read` est
/// toujours faux a l'issue de `timeout`.
async fn header_deadline(read: &AtomicBool, timeout: Duration) {
    tokio::time::sleep(timeout).await;
    if read.load(Ordering::Acquire) {
        std::future::pending::<()>().await;
    }
}

#[async_trait]
impl<A: ServerApp + Send + Sync + 'static> ServerApp for Capped<A> {
    async fn process_new(
        self: &Arc<Self>,
        mut stream: Stream,
        shutdown: &ShutdownWatch,
    ) -> Option<Stream> {
        let ip = stream
            .get_socket_digest()
            .and_then(|d| d.peer_addr().and_then(|a| a.as_inet()).map(|a| a.ip()));
        let kind = if stream.get_ssl_digest().is_some() {
            Kind::Tls
        } else {
            Kind::Plain
        };
        let id = stream.id();
        if kind == Kind::Tls {
            self.limiter.handshake_done(id);
        }
        let _guard = ip.map(|ip| self.limiter.enter(ip, kind));
        // Premier octet : un flux qui ne sait pas regarder sans consommer
        // (TLS) repond `false` aussitot.
        let mut first = [0u8; 1];
        let limits = self.limiter.limits;
        match tokio::time::timeout(limits.first_byte_timeout, stream.try_peek(&mut first)).await {
            Ok(Ok(_)) => {}
            Ok(Err(_)) | Err(_) => return None,
        }
        // HTTP/2 (si un jour negocie par ALPN) : plusieurs requetes par
        // connexion et pas de `session.stream()` dans le proxy pour desarmer
        // l'echeance, qui couperait toute connexion h2 a 20 s. Pas
        // d'echeance d'en-tetes ici (Pingora borne h2 a sa facon).
        if is_h2(stream.selected_alpn_proto()) {
            self.inner.process_new(stream, shutdown).await;
            return None;
        }
        let mut next = Some(stream);
        while let Some(stream) = next.take() {
            let read = self.limiter.arm_headers(id);
            tokio::select! {
                reused = self.inner.process_new(stream, shutdown) => next = reused,
                () = header_deadline(&read, limits.header_timeout) => {
                    tracing::debug!("en-tetes non lus dans le delai : connexion coupee");
                }
            }
        }
        self.limiter.disarm_headers(id);
        None
    }

    async fn cleanup(&self) {
        self.inner.cleanup().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use Kind::{Plain, Tls};

    fn limiter(global: usize, per_ip: usize, pending_tls: usize) -> Arc<ConnLimiter> {
        Arc::new(ConnLimiter::new(ConnLimits {
            max_connections: global,
            max_connections_per_ip: per_ip,
            max_pending_tls: pending_tls,
            max_pending_tls_per_ip: per_ip,
            ..ConnLimits::default()
        }))
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn per_ip_cap_counts_pending_and_active_and_releases_on_drop() {
        let l = limiter(100, 2, 100);
        let a = ip("203.0.113.7");
        assert!(l.admit(a, Plain));
        let g1 = l.enter(a, Plain);
        assert!(l.admit(a, Tls));
        assert!(!l.admit(a, Plain), "troisieme connexion de la meme IP");
        assert!(
            l.admit(ip("203.0.113.8"), Plain),
            "une autre IP n'est pas concernee"
        );
        assert_eq!(l.count(a), 2);
        drop(g1);
        assert_eq!(l.count(a), 1);
        assert!(l.admit(a, Plain));
    }

    #[test]
    fn global_cap_applies_across_ips() {
        let l = limiter(3, 10, 100);
        for i in 1..=3 {
            assert!(l.admit(ip(&format!("198.51.100.{i}")), Plain));
        }
        assert!(!l.admit(ip("198.51.100.4"), Plain));
        assert_eq!(l.count(ip("198.51.100.4")), 0, "aucune entree laissee");
        drop(l.enter(ip("198.51.100.1"), Plain));
        assert!(l.admit(ip("198.51.100.4"), Plain));
    }

    #[test]
    fn failed_tls_handshakes_never_consume_the_active_capacity() {
        // Plafond global de 2 actives, 3 attentes TLS.
        let l = limiter(2, 10, 3);
        for i in 1..=3 {
            assert!(l.admit(ip(&format!("192.0.2.{i}")), Tls));
        }
        assert!(!l.admit(ip("192.0.2.9"), Tls), "plafond des attentes TLS");
        // Les attentes TLS (jamais liberees) laissent le port 80 libre.
        assert!(l.admit(ip("198.51.100.1"), Plain));
        let _a = l.enter(ip("198.51.100.1"), Plain);
        assert!(l.admit(ip("198.51.100.2"), Plain));
        let _b = l.enter(ip("198.51.100.2"), Plain);
        assert!(!l.admit(ip("198.51.100.3"), Plain), "plafond des actives");
        // Une poignee de main reussie libere son attente TLS (et devient
        // une active comme les autres).
        drop(_a);
        drop(_b);
        let _t = l.enter(ip("192.0.2.1"), Tls);
        assert!(l.admit(ip("192.0.2.9"), Tls));
    }

    #[test]
    fn pending_tls_per_ip_cap_leaves_plain_and_other_ips_alone() {
        let l = Arc::new(ConnLimiter::new(ConnLimits {
            max_pending_tls_per_ip: 2,
            ..ConnLimits::default()
        }));
        let nat = ip("203.0.113.50");
        assert!(l.admit(nat, Tls));
        assert!(l.admit(nat, Tls));
        assert!(!l.admit(nat, Tls), "troisieme poignee de main du NAT");
        assert!(l.admit(nat, Plain), "HTTP du meme NAT");
        assert!(l.admit(ip("203.0.113.51"), Tls), "autre IP");
        // Une poignee de main terminee libere une place.
        let _done = l.enter(nat, Tls);
        assert!(l.admit(nat, Tls));
    }

    #[test]
    fn pending_tls_admissions_expire_after_the_handshake_timeout() {
        let l = Arc::new(ConnLimiter::new(ConnLimits {
            max_pending_tls_per_ip: 1,
            tls_handshake_timeout: Duration::from_millis(100),
            ..ConnLimits::default()
        }));
        let a = ip("192.0.2.77");
        assert!(l.admit(a, Tls));
        assert!(!l.admit(a, Tls));
        // Delai de poignee de main + marge d'une seconde.
        std::thread::sleep(Duration::from_millis(1200));
        assert!(l.admit(a, Tls));
    }

    #[test]
    fn ipv6_is_bucketed_by_slash_64_and_mapped_ipv4_is_ipv4() {
        let l = limiter(100, 1, 100);
        assert!(l.admit(ip("2001:db8:1:2::1"), Plain));
        assert!(!l.admit(ip("2001:db8:1:2:ffff::9"), Tls));
        assert!(l.admit(ip("2001:db8:1:3::1"), Plain));
        assert!(l.admit(ip("192.0.2.1"), Plain));
        assert!(!l.admit(ip("::ffff:192.0.2.1"), Plain));
    }

    /// Paire de sockets locales : (cote serveur, surveille ; cote client).
    fn socket_pair() -> (std::net::TcpStream, std::net::TcpStream) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let client = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        (server, client)
    }

    /// Le client voit-il sa connexion coupee (fin de flux ou erreur) ?
    fn cut(client: &mut std::net::TcpStream) -> bool {
        use std::io::Read;
        client
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        let mut buf = [0u8; 1];
        match client.read(&mut buf) {
            Ok(0) => true,
            Ok(_) => false,
            Err(e) => !matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ),
        }
    }

    #[test]
    fn header_deadline_never_applies_to_http2_connections() {
        assert!(is_h2(Some(ALPN::H2)));
        assert!(!is_h2(Some(ALPN::H1)));
        assert!(!is_h2(None));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_full_pending_tls_cap_evicts_the_oldest_handshake() {
        // 4 attentes TLS au plus : plafond rempli par 4 sources (comme
        // 32 /64 en production), chacune sous son propre plafond.
        let l = Arc::new(ConnLimiter::new(ConnLimits {
            max_pending_tls: 4,
            max_pending_tls_per_ip: 32,
            ..ConnLimits::default()
        }));
        let mut clients = Vec::new();
        let mut servers = Vec::new();
        for i in 1..=4 {
            let src = ip(&format!("2001:db8:{i}::1"));
            assert!(l.admit(src, Tls));
            let (server, client) = socket_pair();
            l.watch_handshake(1000 + i, server.try_clone().unwrap(), Some(src));
            servers.push(server);
            clients.push(client);
        }
        assert_eq!(l.pending_tls(), 4);
        // Nouveau venu : admis, la plus ancienne poignee de main est coupee
        // et son admission liberee.
        let newcomer = ip("198.51.100.7");
        assert!(l.admit(newcomer, Tls), "nouveau venu admis");
        assert!(cut(&mut clients[0]), "plus ancienne coupee");
        for c in &mut clients[1..] {
            assert!(!cut(c), "les autres continuent");
        }
        assert_eq!(l.pending_tls(), 4, "une place rendue, une prise");
        assert_eq!(l.count(ip("2001:db8:1::1")), 0, "admission evincee liberee");
        assert_eq!(l.count(ip("2001:db8:2::1")), 1);
        assert_eq!(l.count(newcomer), 1);
        // Une seule eviction par admission : le flot ne coupe que ce qu'il
        // remplace.
        let (server, mut client) = socket_pair();
        l.watch_handshake(2000, server.try_clone().unwrap(), Some(newcomer));
        assert!(l.admit(ip("198.51.100.8"), Tls));
        assert!(cut(&mut clients[1]));
        assert!(
            !cut(&mut client),
            "le nouveau venu n'est pas le plus ancien"
        );
        assert_eq!(l.pending_tls(), 4);
        assert_eq!(l.count(ip("2001:db8:2::1")), 0);
        // Poignee de main terminee : son admission devient une active.
        l.handshake_done(2000);
        let guard = l.enter(newcomer, Tls);
        assert_eq!(l.pending_tls(), 3);
        assert_eq!(l.count(newcomer), 1);
        drop(guard);
        assert_eq!(l.count(newcomer), 0);
        drop(servers);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_reset_connection_is_still_watched_and_evictable() {
        use pingora::protocols::{GetSocketDigest, SocketDigest};
        // Client qui se connecte puis reinitialise aussitot (RST) :
        // `getpeername` echoue ensuite (ENOTCONN), mais l'adresse gardee a
        // `accept` (digest de Pingora) suffit a surveiller la poignee de main.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (server, peer) = listener.accept().await.unwrap();
        client.set_zero_linger().unwrap();
        drop(client);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(server.peer_addr().is_err(), "connexion deja reinitialisee");

        let l = Arc::new(ConnLimiter::new(ConnLimits {
            max_pending_tls: 1,
            ..ConnLimits::default()
        }));
        assert!(l.admit(peer.ip(), Tls));
        let mut stream = L4Stream::from(server);
        let digest = SocketDigest::from_raw_fd(stream.as_raw_fd());
        digest.peer_addr.set(Some(peer.into())).unwrap();
        stream.set_socket_digest(digest);
        HandshakeDeadline { limiter: l.clone() }
            .process(&mut stream)
            .await
            .unwrap();
        // Plafond plein : la poignee de main reinitialisee cede sa place.
        assert!(l.admit(ip("198.51.100.9"), Tls), "place rendue");
        assert_eq!(l.count(peer.ip()), 0);
        assert_eq!(l.pending_tls(), 1);
    }
}
