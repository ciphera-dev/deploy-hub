//! Vue d'ensemble de l'instance (`GET /api/overview`).

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::deployments::Deployment;

/// Periode du graphe.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum OverviewPeriod {
    #[serde(rename = "7d")]
    Days7,
    #[default]
    #[serde(rename = "30d")]
    Days30,
}

impl OverviewPeriod {
    /// Nombre de jours (un point par jour).
    pub fn days(self) -> i64 {
        match self {
            OverviewPeriod::Days7 => 7,
            OverviewPeriod::Days30 => 30,
        }
    }
}

/// Genre d'element « A traiter ».
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    /// Dernier deploiement de la production en echec.
    ProjectFailed,
    /// Conteneur servi arrete ou en echec hors du panneau.
    ProjectDown,
    /// Agent du serveur deconnecte.
    ServerUnreachable,
}

/// Element « A traiter ».
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct OverviewIssue {
    pub kind: IssueKind,
    /// Identifiant du projet ou du serveur.
    pub id: String,
    /// Nom du projet ou du serveur.
    pub name: String,
}

/// Point d'une serie (un jour).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct ChartPoint {
    /// Jour (UTC, `AAAA-MM-JJ`).
    pub date: String,
    pub value: f64,
}

/// Indicateur du graphe. Sans source de donnees : total absent, serie vide.
#[derive(Clone, Debug, Default, Serialize, Deserialize, ToSchema)]
pub struct KpiSeries {
    /// Total sur la periode.
    pub total: Option<f64>,
    /// Variation par rapport a la periode precedente (absente sans base de
    /// comparaison).
    pub change_percent: Option<f64>,
    pub points: Vec<ChartPoint>,
}

/// Indicateurs du graphe de la vue d'ensemble.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct OverviewChart {
    pub deployments: KpiSeries,
    /// Requetes servies (aucune source en v0.1 : vide).
    pub requests: KpiSeries,
    /// Latence p95 en millisecondes (aucune source en v0.1 : vide).
    pub latency_p95_ms: KpiSeries,
}

/// Charge d'un serveur (derniere mesure de moins de 2 minutes).
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct ServerLoad {
    pub id: String,
    pub name: String,
    /// Agent connecte.
    pub online: bool,
    /// `local` ou `remote`.
    pub kind: String,
    pub cpu_percent: Option<f64>,
    pub memory_percent: Option<f64>,
    /// Cause du dernier refus de l'agent (version du protocole differente :
    /// mise a jour incomplete), tant qu'il n'est pas reconnecte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_error: Option<String>,
}

/// Reponse de `GET /api/overview`.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct Overview {
    /// Elements a traiter (vide : « Rien a signaler »).
    pub issues: Vec<OverviewIssue>,
    pub projects_online: i64,
    pub projects_total: i64,
    pub servers_online: i64,
    pub servers_total: i64,
    /// Deploiements crees aujourd'hui (UTC).
    pub deployments_today: i64,
    pub period: OverviewPeriod,
    pub chart: OverviewChart,
    /// Les 5 derniers deploiements, le plus recent d'abord. Absent sans la
    /// lecture des deploiements.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub recent_deployments: Option<Vec<Deployment>>,
    /// Absent sans la lecture des serveurs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub servers: Option<Vec<ServerLoad>>,
}

/// Vue d'ensemble de l'instance (viewer et plus).
#[utoipa::path(
    get,
    path = "/api/overview",
    tag = "overview",
    params(("period" = Option<OverviewPeriod>, Query, description = "Periode du graphe (30d par defaut)")),
    responses(
        (status = 200, description = "Vue d'ensemble", body = Overview),
        (status = 401, description = "Session absente"),
        (status = 422, description = "Periode inconnue"),
    )
)]
#[allow(dead_code)]
pub fn get_overview() {}
