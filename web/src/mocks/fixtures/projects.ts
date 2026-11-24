import type { components } from "@/lib/api/schema";

type ProjectSummary = components["schemas"]["ProjectSummary"];
type Project = components["schemas"]["Project"];

// Donnees de prototypes/dashboard/data.js (`projects`, `services`), ramenees au
// contrat de l'API : un seul environnement (`production`), depots GitHub.
// Donnees factices pour MSW et les tests, jamais des donnees de production.

const T = "2026-05-15T10:00:00.000Z";

export const PROJECT_SUMMARIES: ProjectSummary[] = [
  { id: "00000000-0000-4000-8000-000000000001", name: "ciphera-web", environment: "production", status: "running", source: { type: "git", repository: "ciphera/web", branch: "main" }, build: { kind: "railpack" }, services: 3, created_at: T, updated_at: T },
  { id: "00000000-0000-4000-8000-000000000002", name: "billing-api", environment: "production", status: "failed", source: { type: "git", repository: "ciphera/billing", branch: "main" }, build: { kind: "dockerfile", path: "Dockerfile" }, services: 2, created_at: T, updated_at: T },
  { id: "00000000-0000-4000-8000-000000000003", name: "dashboard-v2", environment: "production", status: "building", source: { type: "git", repository: "ciphera/dashboard", branch: "feat/charts" }, build: { kind: "railpack" }, services: 1, created_at: T, updated_at: T },
  { id: "00000000-0000-4000-8000-000000000004", name: "auth-service", environment: "production", status: "running", source: { type: "git", repository: "ciphera/auth", branch: "main" }, build: { kind: "dockerfile", path: "Dockerfile" }, services: 2, created_at: T, updated_at: T },
  { id: "00000000-0000-4000-8000-000000000005", name: "marketing-site", environment: "production", status: "none", source: { type: "git", repository: "ciphera/site", branch: "develop" }, build: { kind: "compose", path: "compose.yaml", service: "site" }, services: 1, created_at: T, updated_at: T },
  { id: "00000000-0000-4000-8000-000000000006", name: "cache", environment: "production", status: "none", source: { type: "image", image: "redis:7" }, build: null, services: 1, created_at: T, updated_at: T },
];

/** Detail d'un projet de la liste (un service par environnement, comme le serveur v0.1). */
export function projectDetail(summary: ProjectSummary): Project {
  return {
    id: summary.id,
    name: summary.name,
    status: summary.status,
    source: summary.source,
    build: summary.build,
    environments: [{ id: summary.id.replace(/1$/, "a"), name: summary.environment }],
    services: [{ id: summary.id.replace(/1$/, "b"), name: summary.build?.kind === "compose" ? summary.build.service : summary.name, environment: summary.environment }],
    created_at: summary.created_at,
    updated_at: summary.updated_at,
  };
}
