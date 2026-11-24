import type { components } from "@/lib/api/schema";

import { DEPLOYMENTS } from "./deployments";

type Overview = components["schemas"]["Overview"];

// Donnees de prototypes/dashboard/Pages2.jsx#OverviewV2 et data.js
// (`deploys`, `servers`), ramenees au contrat de GET /api/overview. Donnees
// factices pour MSW et les tests, jamais des donnees de production.

const DAY_MS = 86_400_000;
const LAST_DAY = Date.parse("2026-11-22T00:00:00.000Z");
const DEPLOYS = [38, 44, 41, 52, 47, 35, 29, 49, 56, 51, 58, 46, 33, 31, 54, 61, 57, 63, 55, 39, 36, 59, 66, 62, 70, 64, 45, 41, 68, 14];

const day = (i: number, n: number) => new Date(LAST_DAY - (n - 1 - i) * DAY_MS).toISOString().slice(0, 10);

/** Instance de démonstration : un projet en échec, un serveur hors ligne. */
export const OVERVIEW: Overview = {
  issues: [
    { kind: "project_failed", id: "00000000-0000-4000-8000-000000000002", name: "billing-api" },
    { kind: "server_unreachable", id: "00000000-0000-4000-8000-0000000000e3", name: "edge-fra-3" },
  ],
  projects_online: 2,
  projects_total: 6,
  servers_online: 2,
  servers_total: 3,
  deployments_today: 14,
  period: "30d",
  chart: {
    deployments: { total: DEPLOYS.reduce((a, b) => a + b, 0), change_percent: 12.4, points: DEPLOYS.map((value, i) => ({ date: day(i, DEPLOYS.length), value })) },
    requests: { total: null, change_percent: null, points: [] },
    latency_p95_ms: { total: null, change_percent: null, points: [] },
  },
  recent_deployments: DEPLOYMENTS.slice(0, 5),
  servers: [
    { id: "00000000-0000-4000-8000-0000000000e1", name: "prod-eu-1", kind: "local", online: true, cpu_percent: 42, memory_percent: 61.5 },
    { id: "00000000-0000-4000-8000-0000000000e2", name: "prod-eu-2", kind: "remote", online: true, cpu_percent: 88, memory_percent: 70 },
    {
      id: "00000000-0000-4000-8000-0000000000e3",
      name: "edge-fra-3",
      kind: "remote",
      online: false,
      cpu_percent: null,
      memory_percent: null,
      link_error: "Version du protocole incompatible : agent en version 2, plan de contrôle en version 3. Mettez à jour Ciphera sur ce serveur.",
    },
  ],
};

/** Instance neuve : rien à signaler, aucune donnée. */
export function emptyInstanceOverview(period: Overview["period"] = "30d"): Overview {
  const n = period === "7d" ? 7 : 30;
  return {
    issues: [],
    projects_online: 0,
    projects_total: 0,
    servers_online: 0,
    servers_total: 0,
    deployments_today: 0,
    period,
    chart: {
      deployments: { total: 0, change_percent: null, points: Array.from({ length: n }, (_, i) => ({ date: day(i, n), value: 0 })) },
      requests: { total: null, change_percent: null, points: [] },
      latency_p95_ms: { total: null, change_percent: null, points: [] },
    },
    recent_deployments: [],
    servers: [],
  };
}
