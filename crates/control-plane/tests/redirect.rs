mod common;

use common::{plain_get, serve_redirect, serve_redirect_with};

#[tokio::test]
async fn http_redirects_to_https() {
    let (addr, _h) = serve_redirect(8443).await;
    let host = format!("panel.example.com:{}", addr.port());
    let r = plain_get(addr, &host, "/projects/abc?x=1").await;
    assert_eq!(r.status, 301);
    assert_eq!(
        r.header("location"),
        Some("https://panel.example.com:8443/projects/abc?x=1")
    );
    assert!(r.body.is_empty());
}

#[tokio::test]
async fn redirect_keeps_ipv6_literal_and_drops_port_443() {
    let (addr, _h) = serve_redirect(443).await;
    let r = plain_get(addr, "[2001:db8::1]:80", "/").await;
    assert_eq!(r.header("location"), Some("https://[2001:db8::1]/"));
}

#[tokio::test]
async fn redirect_refuses_hostile_host_headers() {
    let (addr, _h) = serve_redirect(8443).await;
    for host in ["evil.com/x", "a b", "x.com@evil.com", "x..com", "[zz]"] {
        let r = plain_get(addr, host, "/").await;
        assert_eq!(r.status, 400, "{host}");
        assert!(r.header("location").is_none(), "{host}");
    }
}

#[tokio::test]
async fn redirect_goes_to_the_confirmed_panel_domain_once_8443_is_closed() {
    use control_plane::domain::domains::panel::{Panel, PanelSwitch};
    let switch = PanelSwitch::default();
    let panel = Panel {
        id: uuid::Uuid::new_v4(),
        host: "panel.example.org".into(),
        confirmed: false,
    };
    let (addr, _h) = serve_redirect_with(8443, switch.clone()).await;
    // Domaine pas encore confirme : 8443 reste la cible.
    switch.set(Some(panel.clone()));
    let r = plain_get(addr, "203.0.113.7", "/projects?x=1").await;
    assert_eq!(
        r.header("location"),
        Some("https://203.0.113.7:8443/projects?x=1")
    );
    // Confirme : 8443 est ferme, un hote nomme va vers le domaine du
    // panneau ; un hote IP recoit 404, sans `Location` ni nom.
    switch.set(Some(Panel {
        confirmed: true,
        ..panel
    }));
    for host in ["panel.example.org", "panel.example.com:80"] {
        let r = plain_get(addr, host, "/projects?x=1").await;
        assert_eq!(r.status, 301, "{host}");
        assert_eq!(
            r.header("location"),
            Some("https://panel.example.org/projects?x=1"),
            "{host}"
        );
    }
    for host in [
        "203.0.113.7",
        "203.0.113.7:80",
        "[2001:db8::1]",
        "[2001:db8::1]:80",
    ] {
        let r = plain_get(addr, host, "/projects?x=1").await;
        assert_eq!(r.status, 404, "{host}");
        assert!(r.header("location").is_none(), "{host}");
        assert!(
            !String::from_utf8_lossy(&r.body).contains("example"),
            "{host}"
        );
    }
    // Un Host douteux reste refuse.
    assert_eq!(plain_get(addr, "evil.com/x", "/").await.status, 400);
}
