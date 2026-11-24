//! Appels du routeur sans reseau (`oneshot`), avec cookie de session,
//! jeton CSRF et adresse du pair.

use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, header};
use control_plane::app::build_router;
use control_plane::state::AppState;
use serde_json::Value;
use tower::ServiceExt;

pub const DIRECT: &str = "203.0.113.7:50000";

#[derive(Default)]
pub struct Req<'a> {
    pub cookie: Option<&'a str>,
    pub csrf: Option<&'a str>,
    pub peer: Option<&'a str>,
    /// En-tete `Host` (absent par defaut).
    pub host: Option<&'a str>,
    pub body: Option<Value>,
    /// Corps brut et son `Content-Type` (prioritaire sur `body`).
    pub bytes: Option<(&'a str, Vec<u8>)>,
    /// En-tetes supplementaires.
    pub headers: Vec<(&'a str, String)>,
}

pub struct Reply {
    pub status: u16,
    pub set_cookie: Option<String>,
    pub retry_after: Option<String>,
    pub cache_control: Option<String>,
    pub raw: String,
    pub headers: header::HeaderMap,
    pub bytes: Vec<u8>,
}

impl Reply {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.raw).unwrap_or(Value::Null)
    }

    pub fn code(&self) -> Value {
        self.json()["error"]["code"].clone()
    }

    /// Valeur du cookie de session pose par la reponse.
    pub fn token(&self) -> String {
        let c = self.set_cookie.as_deref().expect("Set-Cookie");
        let v = c
            .strip_prefix("__Host-ciphera_session=")
            .expect("nom du cookie");
        v.split(';').next().unwrap().to_string()
    }

    pub fn csrf(&self) -> String {
        self.json()["csrf_token"]
            .as_str()
            .expect("csrf_token")
            .to_string()
    }
}

pub async fn call(state: &AppState, method: &str, path: &str, r: Req<'_>) -> Reply {
    let mut req = Request::builder().method(method).uri(path);
    if let Some(c) = r.cookie {
        req = req.header(header::COOKIE, format!("__Host-ciphera_session={c}"));
    }
    if let Some(h) = r.host {
        req = req.header(header::HOST, h);
    }
    if let Some(t) = r.csrf {
        req = req.header("x-csrf-token", t);
    }
    for (name, value) in &r.headers {
        req = req.header(*name, value);
    }
    let body = match (r.bytes, r.body) {
        (Some((content_type, data)), _) => {
            req = req.header(header::CONTENT_TYPE, content_type);
            Body::from(data)
        }
        (None, Some(b)) => {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(b.to_string())
        }
        (None, None) => Body::empty(),
    };
    let mut req = req.body(body).unwrap();
    let peer: SocketAddr = r.peer.unwrap_or(DIRECT).parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let res = build_router(state.clone()).oneshot(req).await.unwrap();
    let status = res.status().as_u16();
    let (set_cookie, retry_after, cache_control) = {
        let get = |name| {
            res.headers()
                .get(name)
                .map(|v: &header::HeaderValue| v.to_str().unwrap().to_string())
        };
        (
            get(header::SET_COOKIE),
            get(header::RETRY_AFTER),
            get(header::CACHE_CONTROL),
        )
    };
    let headers = res.headers().clone();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    Reply {
        status,
        set_cookie,
        retry_after,
        cache_control,
        raw: String::from_utf8_lossy(&bytes).into_owned(),
        headers,
        bytes: bytes.to_vec(),
    }
}
