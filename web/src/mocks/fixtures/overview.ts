import type { components } from "@/lib/api/schema";

import { DEPLOYMENTS } from "./deployments";

type ProjectOverview = components["schemas"]["ProjectOverview"];
type ProjectService = components["schemas"]["ProjectService"];

// Donnees de prototypes/dashboard/ProjectOverview.jsx (KPI `req`, `lat`, `err`,
// `up`) et de data.js (`services.web`), ramenees au contrat de l'API. Donnees
// factices pour MSW et les tests, jamais des donnees de production.

const REQUESTS = [42, 38, 35, 31, 29, 33, 41, 55, 68, 74, 79, 82, 80, 77, 81, 85, 88, 84, 79, 72, 66, 58, 51, 47];
const LATENCY = [148, 152, 150, 155, 149, 160, 171, 158, 152, 149, 151, 190, 176, 160, 154, 150, 148, 151, 149, 146, 152, 155, 149, 182];
const ERRORS = [0, 0, 1, 0, 0, 0, 0, 2, 0, 0, 0, 7, 4, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1];
const UPTIME = Array.from({ length: 30 }, (_, i) => (i === 17 ? 96 : 100));

const WEB = "00000000-0000-4000-8000-000000000001";

/** Services de `ciphera-web` (data.js#services.web : web, worker, postgres). */
export const WEB_SERVICES: ProjectService[] = [
  { id: "00000000-0000-4000-8000-0000000000b1", name: "web", environment: "production", state: "running", deployment_id: DEPLOYMENTS[1].id, deployment_number: 4818, image: "ciphera/ciphera-web-production:4818", internal_port: 3000, replicas: { running: 1, desired: 1 }, cpu_percent: 34, memory_bytes: 268_435_456, run_as: null, read_only_rootfs: false, runs_as_root: true },
  { id: "00000000-0000-4000-8000-0000000000b2", name: "worker", environment: "production", state: "stopped", deployment_id: DEPLOYMENTS[1].id, deployment_number: 4818, image: "ciphera/ciphera-web-production:4818", replicas: { running: 0, desired: 1 }, run_as: "1000:1000", read_only_rootfs: true, runs_as_root: false },
  { id: "00000000-0000-4000-8000-0000000000b3", name: "postgres", environment: "production", state: "failed", replicas: { running: 0, desired: 1 }, run_as: null, read_only_rootfs: false, runs_as_root: false },
];

/** Aperçu de `ciphera-web`, avec les séries de la maquette. */
export const WEB_OVERVIEW: ProjectOverview = {
  status: "running",
  production: DEPLOYMENTS[1],
  domain: "app.ciphera.dev",
  metrics: {
    requests: { value: 1_240_000, series: REQUESTS },
    latency_p95_ms: { value: 182, series: LATENCY },
    error_rate: { value: 0.04, series: ERRORS },
    uptime: { value: 99.98, series: UPTIME },
  },
  services: WEB_SERVICES,
  activity: DEPLOYMENTS.filter((d) => d.project_id === WEB).slice(0, 4),
};

/** Aperçu d'un projet sans métriques ni déploiement (tel que le serveur v0.1 le renvoie avant Task 31). */
export function emptyOverview(service: Omit<ProjectService, "run_as" | "read_only_rootfs" | "runs_as_root">): ProjectOverview {
  const none = { value: null, series: [] };
  return { status: "none", production: null, domain: null, metrics: { requests: none, latency_p95_ms: none, error_rate: none, uptime: none }, services: [{ run_as: null, read_only_rootfs: false, runs_as_root: false, ...service }], activity: [] };
}
