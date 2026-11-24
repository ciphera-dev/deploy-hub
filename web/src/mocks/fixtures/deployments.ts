import type { components } from "@/lib/api/schema";

type Deployment = components["schemas"]["Deployment"];
type Step = components["schemas"]["DeploymentStep"];
type Stage = components["schemas"]["DeploymentStage"];

// Donnees de prototypes/dashboard/ProjectDeployments.jsx (`DEPS`) et de
// Pages2.jsx (`DEPS`), ramenees au contrat de l'API (25b). Donnees factices
// pour MSW et les tests, jamais des donnees de production.

const NOW = Date.parse("2026-11-22T12:00:00.000Z");
const ago = (seconds: number) => new Date(NOW - seconds * 1000).toISOString();

const WEB = "00000000-0000-4000-8000-000000000001";
const BILLING = "00000000-0000-4000-8000-000000000002";

const PIPELINE: Stage[] = ["clone", "build", "image", "start", "healthcheck", "switch"];

/** Etapes terminees avec succes pour les durees donnees (ms), dans l'ordre du pipeline. */
function done(durations: number[], startedAgo: number): Step[] {
  return durations.map((duration_ms, i) => ({
    stage: PIPELINE[i],
    status: "succeeded",
    started_at: ago(startedAgo - i),
    finished_at: ago(startedAgo - i - 1),
    duration_ms,
  }));
}

function base(over: Partial<Deployment> & Pick<Deployment, "id" | "number" | "status">): Deployment {
  return {
    project_id: WEB,
    project_name: "ciphera-web",
    environment_id: "00000000-0000-4000-8000-0000000000a1",
    environment: "production",
    trigger: "push",
    commit_sha: "a3f91c2e0d77f215b2e0c85fa3c10d4b6e9a1f02",
    branch: "main",
    steps: [],
    cancel_requested: false,
    current: false,
    can_rollback: false,
    created_at: ago(600),
    ...over,
  };
}

export const DEPLOYMENTS: Deployment[] = [
  base({
    id: "10000000-0000-4000-8000-000000004821",
    number: 4821,
    status: "running",
    stage: "build",
    commit_sha: "c81e0a9d2f4b6a1e3c5d7f9b0a2c4e6f8a0b1c2d",
    created_by: "Lina Moreau",
    created_at: ago(120),
    started_at: ago(118),
    steps: [
      { stage: "clone", status: "succeeded", started_at: ago(118), finished_at: ago(115), duration_ms: 3000 },
      { stage: "build", status: "running", started_at: ago(115) },
    ],
  }),
  base({
    id: "10000000-0000-4000-8000-000000004818",
    number: 4818,
    status: "success",
    stage: "switch",
    current: true,
    can_rollback: true,
    created_by: "Alex Martin",
    trigger: "manual",
    created_at: ago(4 * 3600),
    started_at: ago(4 * 3600 - 1),
    finished_at: ago(4 * 3600 - 97),
    duration_ms: 96_000,
    steps: done([3000, 62_000, 9000, 8000, 4000, 2000], 4 * 3600),
  }),
  base({
    id: "10000000-0000-4000-8000-000000004812",
    number: 4812,
    status: "success",
    stage: "switch",
    can_rollback: true,
    commit_sha: "0d77f21b2e0c85fa3c10d4b6e9a1f02c81e0a9d2",
    created_at: ago(26 * 3600),
    started_at: ago(26 * 3600 - 1),
    finished_at: ago(26 * 3600 - 126),
    duration_ms: 125_000,
    steps: done([3000, 88_000, 12_000, 10_000, 9000, 3000], 26 * 3600),
  }),
  base({
    id: "10000000-0000-4000-8000-000000004809",
    number: 4809,
    status: "cancelled",
    trigger: "rollback",
    rollback_of: "10000000-0000-4000-8000-000000004812",
    created_by: "Lina Moreau",
    created_at: ago(2 * 86400),
    started_at: ago(2 * 86400 - 1),
    finished_at: ago(2 * 86400 - 19),
    duration_ms: 18_000,
    stage: "start",
    steps: [{ stage: "start", status: "cancelled", started_at: ago(2 * 86400 - 1), finished_at: ago(2 * 86400 - 19), duration_ms: 18_000 }],
  }),
  base({
    id: "10000000-0000-4000-8000-000000004807",
    number: 4807,
    status: "failed",
    stage: "build",
    error: "pnpm install a échoué (ERR_PNPM_LOCKFILE)",
    commit_sha: "9fa3c10d4b6e9a1f02c81e0a9d2f4b6a1e3c5d7f",
    created_at: ago(2 * 86400 + 3600),
    started_at: ago(2 * 86400 + 3599),
    finished_at: ago(2 * 86400 + 3558),
    duration_ms: 41_000,
    steps: [
      { stage: "clone", status: "succeeded", started_at: ago(2 * 86400 + 3599), finished_at: ago(2 * 86400 + 3596), duration_ms: 3000 },
      { stage: "build", status: "failed", started_at: ago(2 * 86400 + 3596), finished_at: ago(2 * 86400 + 3558), duration_ms: 38_000, message: "pnpm install a échoué (ERR_PNPM_LOCKFILE)" },
    ],
  }),
  base({
    id: "10000000-0000-4000-8000-000000004815",
    number: 4815,
    status: "success",
    stage: "switch",
    current: true,
    project_id: BILLING,
    project_name: "billing-api",
    branch: "main",
    commit_sha: "5f2a0c1d4b6e9a1f02c81e0a9d2f4b6a1e3c5d7f",
    created_at: ago(6 * 3600),
    started_at: ago(6 * 3600 - 1),
    finished_at: ago(6 * 3600 - 65),
    duration_ms: 64_000,
    steps: done([2000, 38_000, 9000, 8000, 4000, 3000], 6 * 3600),
  }),
];

export const NOW_ISO = new Date(NOW).toISOString();

/** Deploiement en file, tel que renvoye par `POST /api/projects/{id}/deployments`. */
export function queuedDeployment(projectId: string, number = 4900): Deployment {
  const project = DEPLOYMENTS.find((d) => d.project_id === projectId) ?? DEPLOYMENTS[0];
  return base({ id: `10000000-0000-4000-8000-00000000${number}`, number, status: "queued", trigger: "manual", project_id: project.project_id, project_name: project.project_name, created_by: "Alice Martin", created_at: NOW_ISO });
}
