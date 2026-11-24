//! Codec du protocole v1 : aller-retour de chaque message, trames trop
//! grandes refusees avant allocation, aucun panic sur des octets quelconques.

use std::net::SocketAddr;

use proptest::prelude::*;
use proto::codec::{self, MAX_FRAME_LEN};
use proto::*;
use shared::ids::{DeploymentId, ProjectId, ServerId, ServiceId};

fn image() -> ImageRef {
    ImageRef("ghcr.io/acme/web@sha256:0123".into())
}

fn control_messages() -> Vec<ControlToAgent> {
    let deployment_id = DeploymentId::new();
    let service_id = ServiceId::new();
    vec![
        ControlToAgent::Hello {
            version: PROTO_VERSION,
        },
        ControlToAgent::Deploy(DeploySpec {
            deployment_id,
            service_id,
            image: image(),
            internal_port: Some(8080),
            project_id: ProjectId::new(),
            healthcheck: Healthcheck::default(),
            compose: None,
            service_name: Some("web".into()),
            env: vec![EnvVar {
                key: "DATABASE_URL".into(),
                value: EnvValue::new("postgres://u:p@db/app".into()),
            }],
        }),
        ControlToAgent::Deploy(DeploySpec {
            deployment_id,
            service_id,
            image: image(),
            internal_port: None,
            project_id: ProjectId::new(),
            healthcheck: Healthcheck {
                path: "/healthz".into(),
                tries: 3,
                interval_secs: 2,
            },
            compose: Some(ComposeDeploy {
                service: "web".into(),
                bundle: DeploymentId::new(),
            }),
            service_name: None,
            env: Vec::new(),
        }),
        ControlToAgent::Stop { deployment_id },
        ControlToAgent::Logs {
            target: LogTarget::Deployment(deployment_id),
            tail: 200,
            follow: true,
        },
        ControlToAgent::Logs {
            target: LogTarget::Service(service_id),
            tail: 0,
            follow: false,
        },
        ControlToAgent::Pull { image: image() },
        ControlToAgent::ApplyRoutes(vec![
            Route {
                host: "app.example.com".into(),
                path_prefix: "/".into(),
                upstreams: vec!["172.18.0.5:8080".parse::<SocketAddr>().unwrap()],
                redirect_to: None,
                tls: TlsMode::Acme,
                service: Some(service_id),
            },
            Route {
                host: "example.com".into(),
                path_prefix: "/".into(),
                upstreams: Vec::new(),
                redirect_to: Some("https://www.example.com".into()),
                tls: TlsMode::None,
                service: None,
            },
        ]),
        ControlToAgent::ApplyRoutes(Vec::new()),
        ControlToAgent::RequestMetrics,
        ControlToAgent::RemoveDeployment { deployment_id },
        ControlToAgent::Build(build_spec(deployment_id)),
        ControlToAgent::Build(BuildSpec {
            source: BuildSource::Image { reference: image() },
            build: BuildMethod::Railpack,
            git_token: None,
            ..build_spec(deployment_id)
        }),
        ControlToAgent::StopLogs {
            target: LogTarget::Service(service_id),
        },
        ControlToAgent::Restart { deployment_id },
        ControlToAgent::StopContainers { deployment_id },
        ControlToAgent::RequestState,
    ]
}

fn build_spec(deployment_id: DeploymentId) -> BuildSpec {
    BuildSpec {
        deployment_id,
        project: "web".into(),
        environment: "production".into(),
        source: BuildSource::Git {
            url: "https://github.com/acme/web".into(),
            branch: "main".into(),
            commit_sha: "a".repeat(40),
        },
        build: BuildMethod::Compose {
            path: "compose.yaml".into(),
            service: "web".into(),
        },
        git_token: Some(GitToken::new("ghs_secret-token".into())),
        root_directory: Some("apps/web".into()),
    }
}

#[test]
fn git_token_is_masked_in_debug_and_display() {
    let spec = build_spec(DeploymentId::new());
    let token = spec.git_token.clone().unwrap();
    assert_eq!(token.expose(), "ghs_secret-token");
    for shown in [
        format!("{spec:?}"),
        format!("{token}"),
        format!("{token:?}"),
    ] {
        assert!(!shown.contains("ghs_secret-token"), "{shown}");
    }
    let msg = ControlToAgent::Build(spec);
    assert!(!format!("{msg:?}").contains("ghs_secret-token"));
}

fn agent_messages() -> Vec<AgentToControl> {
    let deployment_id = DeploymentId::new();
    let service_id = ServiceId::new();
    let mut messages = vec![
        AgentToControl::Hello {
            version: PROTO_VERSION,
            agent_id: ServerId::new(),
        },
        AgentToControl::LogLine(LogLine {
            target: LogTarget::Deployment(deployment_id),
            stream: LogStream::Stderr,
            timestamp_ms: 1_763_700_000_000,
            line: "listening on :8080, pret".into(),
        }),
        AgentToControl::Metrics(NodeMetrics {
            cpu_percent: 12.5,
            mem_used_bytes: 1 << 30,
            mem_total_bytes: 4 << 30,
            disk_used_bytes: 10 << 30,
            disk_total_bytes: 40 << 30,
            net_rx_bytes: 123,
            net_tx_bytes: 456,
            containers: vec![ContainerMetrics {
                deployment_id,
                service_id,
                cpu_percent: 3.5,
                mem_used_bytes: 64 << 20,
                running: true,
            }],
        }),
        AgentToControl::State(AgentState {
            deployments: vec![RunningDeployment {
                deployment_id,
                service_id,
                image: image(),
                healthy: true,
                serving: true,
            }],
            in_progress: vec![DeploymentId::new()],
            outcomes: vec![
                DeploymentOutcome {
                    deployment_id,
                    stage: Stage::Switch,
                    status: StageStatus::Succeeded,
                },
                DeploymentOutcome {
                    deployment_id: DeploymentId::new(),
                    stage: Stage::Healthcheck,
                    status: StageStatus::Failed {
                        message: "5 essai(s) : statut HTTP 503".into(),
                    },
                },
            ],
        }),
        AgentToControl::State(AgentState::default()),
        AgentToControl::Error {
            code: error_code::VERSION_MISMATCH,
            message: "version 2 non prise en charge".into(),
        },
    ];
    for stage in [
        Stage::Clone,
        Stage::Build,
        Stage::Image,
        Stage::Start,
        Stage::Healthcheck,
        Stage::Switch,
    ] {
        for status in [
            StageStatus::Started,
            StageStatus::Succeeded,
            StageStatus::Failed {
                message: "healthcheck : 503".into(),
            },
            StageStatus::Cancelled,
        ] {
            messages.push(AgentToControl::DeployProgress {
                deployment_id,
                stage,
                status,
            });
        }
    }
    messages
}

#[test]
fn roundtrip_all_messages() {
    for msg in control_messages() {
        let frame = codec::encode(&msg).unwrap();
        assert_eq!(codec::decode::<ControlToAgent>(&frame).unwrap(), msg);
    }
    for msg in agent_messages() {
        let frame = codec::encode(&msg).unwrap();
        assert_eq!(codec::decode::<AgentToControl>(&frame).unwrap(), msg);
    }
    for msg in [
        ProxyReply::Applied { routes: 2 },
        ProxyReply::Rejected {
            reason: "hote invalide".into(),
        },
    ] {
        let frame = codec::encode(&msg).unwrap();
        assert_eq!(codec::decode::<ProxyReply>(&frame).unwrap(), msg);
    }
}

#[test]
fn frame_is_big_endian_length_then_payload() {
    let frame = codec::encode(&ControlToAgent::RequestMetrics).unwrap();
    let len = u32::from_be_bytes(frame[..4].try_into().unwrap()) as usize;
    assert_eq!(len, frame.len() - 4);
}

#[test]
fn hello_is_first_variant_and_version_first_field() {
    // Un pair d'une autre version doit pouvoir lire le Hello : variante 0,
    // version en premier champ, quel que soit le reste du protocole.
    let frame = codec::encode(&AgentToControl::Hello {
        version: 7,
        agent_id: ServerId::new(),
    })
    .unwrap();
    assert_eq!(&frame[4..6], &[0, 7]);
    let frame = codec::encode(&ControlToAgent::Hello { version: 7 }).unwrap();
    assert_eq!(&frame[4..], &[0, 7]);
}

#[test]
fn stop_logs_is_appended_after_build() {
    // Variante ajoutee apres la v1 initiale : rang postcard 9, apres `Build`
    // (8), sans changer le rang des ordres existants.
    let frame = codec::encode(&ControlToAgent::StopLogs {
        target: LogTarget::Deployment(DeploymentId::new()),
    })
    .unwrap();
    assert_eq!(frame[4], 9);
    let frame = codec::encode(&ControlToAgent::RequestMetrics).unwrap();
    assert_eq!(&frame[4..], &[6]);
}

#[test]
fn restart_is_appended_after_stop_logs() {
    // Rang postcard 10, apres `StopLogs` (9) : les rangs existants ne bougent pas.
    let frame = codec::encode(&ControlToAgent::Restart {
        deployment_id: DeploymentId::new(),
    })
    .unwrap();
    assert_eq!(frame[4], 10);
}

#[test]
fn stop_containers_is_appended_after_restart() {
    // Rang postcard 11, apres `Restart` (10) : les rangs existants ne bougent pas.
    let msg = ControlToAgent::StopContainers {
        deployment_id: DeploymentId::new(),
    };
    let frame = codec::encode(&msg).unwrap();
    assert_eq!(frame[4], 11);
    assert_eq!(codec::decode::<ControlToAgent>(&frame).unwrap(), msg);
}

#[test]
fn request_state_is_appended_after_remove_certificate() {
    // Rang postcard 15, apres `RemoveCertificate` (14) : les rangs existants
    // ne bougent pas.
    let frame = codec::encode(&ControlToAgent::RemoveCertificate { host: "a.b".into() }).unwrap();
    assert_eq!(frame[4], 14);
    let frame = codec::encode(&ControlToAgent::RequestState).unwrap();
    assert_eq!(frame[4], 15);
    assert_eq!(
        codec::decode::<ControlToAgent>(&frame).unwrap(),
        ControlToAgent::RequestState
    );
}

#[test]
fn renewed_certificate_is_appended_after_request_state() {
    // Rang postcard 16, apres `RequestState` (15) ; version 5.
    let msg = ControlToAgent::RenewedCertificate {
        cert_pem: "-----BEGIN CERTIFICATE-----".into(),
    };
    let frame = codec::encode(&msg).unwrap();
    assert_eq!(frame[4], 16);
    assert_eq!(codec::decode::<ControlToAgent>(&frame).unwrap(), msg);
    assert_eq!(proto::PROTO_VERSION, 5);
}

#[test]
fn deploy_without_service_name_from_an_older_peer_still_decodes() {
    // Champ ajoute en fin de `DeploySpec` (dernier champ de `Deploy`) : une
    // trame d'avant ce champ se lit avec `service_name: None`.
    let msg = ControlToAgent::Deploy(DeploySpec {
        deployment_id: DeploymentId::new(),
        service_id: ServiceId::new(),
        image: image(),
        internal_port: Some(8080),
        project_id: ProjectId::new(),
        healthcheck: Healthcheck::default(),
        compose: None,
        service_name: None,
        env: Vec::new(),
    });
    let frame = codec::encode(&msg).unwrap();
    // `service_name` (None = 0) puis `env` (vide = 0) : une trame d'avant
    // les deux champs est la meme charge sans ses deux derniers octets.
    assert_eq!(&frame[frame.len() - 2..], &[0, 0]);
    let old = &frame[4..frame.len() - 2];
    let mut old_frame = (old.len() as u32).to_be_bytes().to_vec();
    old_frame.extend_from_slice(old);
    assert_eq!(codec::decode::<ControlToAgent>(&old_frame).unwrap(), msg);
    // Une charge tronquee ailleurs reste refusee.
    let cut = &frame[4..frame.len() - 3];
    let mut cut_frame = (cut.len() as u32).to_be_bytes().to_vec();
    cut_frame.extend_from_slice(cut);
    assert!(codec::decode::<ControlToAgent>(&cut_frame).is_err());
}

#[test]
fn deploy_without_env_from_an_older_peer_still_decodes() {
    // `env` ajoute apres `service_name`, en fin de `DeploySpec` : une trame
    // d'avant ce champ (charge sans son dernier octet, `env` vide = 0) se
    // lit avec un environnement vide.
    let msg = ControlToAgent::Deploy(DeploySpec {
        deployment_id: DeploymentId::new(),
        service_id: ServiceId::new(),
        image: image(),
        internal_port: Some(8080),
        project_id: ProjectId::new(),
        healthcheck: Healthcheck::default(),
        compose: None,
        service_name: Some("web".into()),
        env: Vec::new(),
    });
    let frame = codec::encode(&msg).unwrap();
    assert_eq!(frame.last(), Some(&0));
    let old = &frame[4..frame.len() - 1];
    let mut old_frame = (old.len() as u32).to_be_bytes().to_vec();
    old_frame.extend_from_slice(old);
    assert_eq!(codec::decode::<ControlToAgent>(&old_frame).unwrap(), msg);
}

#[test]
fn env_values_never_appear_in_debug() {
    let spec = DeploySpec {
        deployment_id: DeploymentId::new(),
        service_id: ServiceId::new(),
        image: image(),
        internal_port: None,
        project_id: ProjectId::new(),
        healthcheck: Healthcheck::default(),
        compose: None,
        service_name: None,
        env: vec![EnvVar {
            key: "API_KEY".into(),
            value: EnvValue::new("sk_test_TopSecretValue".into()),
        }],
    };
    let dbg = format!("{spec:?}");
    assert!(dbg.contains("API_KEY"), "{dbg}");
    assert!(!dbg.contains("TopSecretValue"), "{dbg}");
    assert_eq!(spec.env[0].value.expose(), "sk_test_TopSecretValue");
    // Aller-retour : la valeur passe bien dans la trame (lien mTLS).
    let msg = ControlToAgent::Deploy(spec);
    let frame = codec::encode(&msg).unwrap();
    assert_eq!(codec::decode::<ControlToAgent>(&frame).unwrap(), msg);
}

#[test]
fn build_without_root_directory_from_an_older_peer_still_decodes() {
    // Champ ajoute en fin de `BuildSpec` (dernier champ de `Build`) : une
    // trame d'avant ce champ est la meme charge sans son dernier octet
    // (`None` = 0), et se lit avec `root_directory: None`.
    let msg = ControlToAgent::Build(BuildSpec {
        root_directory: None,
        ..build_spec(DeploymentId::new())
    });
    let frame = codec::encode(&msg).unwrap();
    assert_eq!(frame.last(), Some(&0));
    let old = &frame[4..frame.len() - 1];
    let mut old_frame = (old.len() as u32).to_be_bytes().to_vec();
    old_frame.extend_from_slice(old);
    assert_eq!(codec::decode::<ControlToAgent>(&old_frame).unwrap(), msg);
    // Une charge tronquee ailleurs reste refusee.
    let cut = &frame[4..frame.len() - 3];
    let mut cut_frame = (cut.len() as u32).to_be_bytes().to_vec();
    cut_frame.extend_from_slice(cut);
    assert!(codec::decode::<ControlToAgent>(&cut_frame).is_err());
}

#[test]
fn metrics_without_containers_from_an_older_agent_still_decode() {
    // `containers` ajoute en fin de `NodeMetrics` (dernier champ de
    // `Metrics`) : une trame d'avant ce champ est la meme charge sans son
    // dernier octet (liste vide = 0), et se lit avec une liste vide.
    let msg = AgentToControl::Metrics(NodeMetrics {
        cpu_percent: 1.0,
        mem_used_bytes: 2,
        mem_total_bytes: 3,
        disk_used_bytes: 4,
        disk_total_bytes: 5,
        net_rx_bytes: 6,
        net_tx_bytes: 7,
        containers: Vec::new(),
    });
    let frame = codec::encode(&msg).unwrap();
    assert_eq!(frame.last(), Some(&0));
    let old = &frame[4..frame.len() - 1];
    let mut old_frame = (old.len() as u32).to_be_bytes().to_vec();
    old_frame.extend_from_slice(old);
    assert_eq!(codec::decode::<AgentToControl>(&old_frame).unwrap(), msg);
    // Une charge tronquee ailleurs, ou suivie d'octets en trop, reste refusee.
    let cut = &frame[4..frame.len() - 2];
    let mut cut_frame = (cut.len() as u32).to_be_bytes().to_vec();
    cut_frame.extend_from_slice(cut);
    assert!(codec::decode::<AgentToControl>(&cut_frame).is_err());
    let mut longer = frame[4..].to_vec();
    longer.push(0);
    let mut longer_frame = (longer.len() as u32).to_be_bytes().to_vec();
    longer_frame.extend_from_slice(&longer);
    assert!(codec::decode::<AgentToControl>(&longer_frame).is_err());
}

#[test]
fn decode_rejects_oversize_frame() {
    assert_eq!(MAX_FRAME_LEN, 4 * 1024 * 1024);
    let mut frame = ((MAX_FRAME_LEN + 1) as u32).to_be_bytes().to_vec();
    frame.extend_from_slice(&[0; 16]);
    assert!(matches!(
        codec::decode::<AgentToControl>(&frame),
        Err(codec::Error::Oversize(n)) if n == MAX_FRAME_LEN + 1
    ));
    assert!(matches!(
        codec::payload_len(u32::MAX.to_be_bytes()),
        Err(codec::Error::Oversize(_))
    ));
}

#[test]
fn encode_rejects_oversize_message() {
    let msg = AgentToControl::Error {
        code: error_code::INTERNAL,
        message: "x".repeat(MAX_FRAME_LEN),
    };
    assert!(matches!(
        codec::encode(&msg),
        Err(codec::Error::Oversize(_))
    ));
}

#[test]
fn decode_rejects_truncated_and_trailing_bytes() {
    let frame = codec::encode(&ControlToAgent::RequestMetrics).unwrap();
    assert!(codec::decode::<ControlToAgent>(&frame[..3]).is_err());
    let mut longer = frame.to_vec();
    longer.push(0);
    assert!(codec::decode::<ControlToAgent>(&longer).is_err());
    let frame = codec::encode(&ControlToAgent::Pull { image: image() }).unwrap();
    assert!(codec::decode::<ControlToAgent>(&frame[..frame.len() - 1]).is_err());
}

#[tokio::test]
async fn read_frame_checks_length_before_reading_payload() {
    // Annonce 4 Gio sans les envoyer : refus immediat, sans allocation.
    let mut input: &[u8] = &u32::MAX.to_be_bytes();
    assert!(matches!(
        codec::read_frame::<_, AgentToControl>(&mut input).await,
        Err(codec::Error::Oversize(_))
    ));
}

#[tokio::test]
async fn write_then_read_frames_over_a_stream() {
    let (mut a, mut b) = tokio::io::duplex(1024);
    codec::write_frame(&mut a, &ControlToAgent::RequestMetrics)
        .await
        .unwrap();
    codec::write_frame(&mut a, &ControlToAgent::Hello { version: 1 })
        .await
        .unwrap();
    drop(a);
    assert_eq!(
        codec::read_frame::<_, ControlToAgent>(&mut b)
            .await
            .unwrap(),
        Some(ControlToAgent::RequestMetrics)
    );
    assert_eq!(
        codec::read_frame::<_, ControlToAgent>(&mut b)
            .await
            .unwrap(),
        Some(ControlToAgent::Hello { version: 1 })
    );
    // Fin propre entre deux trames.
    assert_eq!(
        codec::read_frame::<_, ControlToAgent>(&mut b)
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn read_frame_reports_eof_inside_a_frame() {
    let frame = codec::encode(&ControlToAgent::Pull { image: image() }).unwrap();
    let mut input: &[u8] = &frame[..frame.len() - 2];
    assert!(
        codec::read_frame::<_, ControlToAgent>(&mut input)
            .await
            .is_err()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    #[test]
    fn decode_rejects_garbage_without_panic(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let _ = codec::decode::<ControlToAgent>(&bytes);
        let _ = codec::decode::<AgentToControl>(&bytes);
        // Avec un prefixe de longueur coherent, la charge utile est decodee.
        let mut framed = (bytes.len() as u32).to_be_bytes().to_vec();
        framed.extend_from_slice(&bytes);
        let _ = codec::decode::<ControlToAgent>(&framed);
        let _ = codec::decode::<AgentToControl>(&framed);
    }
}

#[tokio::test]
async fn read_frame_announcing_max_without_payload_is_framing_error() {
    let mut input: &[u8] = &(MAX_FRAME_LEN as u32).to_be_bytes();
    assert!(matches!(
        codec::read_frame::<_, AgentToControl>(&mut input).await,
        Err(codec::Error::Framing)
    ));
}

#[tokio::test]
async fn read_frame_reads_a_frame_close_to_the_maximum() {
    let msg = AgentToControl::Error {
        code: error_code::INTERNAL,
        message: "x".repeat(MAX_FRAME_LEN - 16),
    };
    let frame = codec::encode(&msg).unwrap();
    let mut input: &[u8] = &frame;
    assert_eq!(
        codec::read_frame::<_, AgentToControl>(&mut input)
            .await
            .unwrap(),
        Some(msg)
    );
}
