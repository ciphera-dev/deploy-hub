import type { components } from "@/lib/api/schema";

import { PROJECT_SUMMARIES } from "./projects";

type ProjectVariable = components["schemas"]["ProjectVariable"];
type GlobalVariable = components["schemas"]["GlobalVariable"];

// Donnees de prototypes/dashboard/ProjectVariables.jsx (`INIT`) et
// GlobalVariables.jsx (`INIT`), ramenees au contrat de l'API : un seul
// environnement (`production`) par projet, secrets sans valeur (ecriture
// seule). Donnees factices pour MSW et les tests.

const DAY = 86_400_000;
const ago = (days: number) => new Date(Date.parse("2026-05-15T10:00:00.000Z") - days * DAY).toISOString();

export const PROJECT_VARIABLES: ProjectVariable[] = [
  { key: "DATABASE_URL", value: null, secret: true, inherited: false, pending_apply: false, updated_at: ago(3), updated_by: "Alex Martin" },
  { key: "LOG_LEVEL", value: "warn", secret: false, inherited: false, pending_apply: true, updated_at: ago(1), updated_by: "Alex Martin" },
  { key: "NEXT_PUBLIC_API_URL", value: "https://app.example.com/api", secret: false, inherited: false, pending_apply: false, updated_at: ago(5), updated_by: "Lina Moreau" },
  { key: "NODE_ENV", value: "production", secret: false, inherited: false, pending_apply: false, updated_at: ago(60), updated_by: "Alex Martin" },
  { key: "OTEL_EXPORTER_OTLP_ENDPOINT", value: "http://otel.internal:4317", secret: false, inherited: true, pending_apply: false, updated_at: ago(21), updated_by: "Alex Martin" },
  { key: "PORT", value: "3000", secret: false, inherited: false, pending_apply: false, updated_at: ago(60), updated_by: "Alex Martin" },
  { key: "RESEND_API_KEY", value: null, secret: true, inherited: true, pending_apply: false, updated_at: ago(12), updated_by: "Lina Moreau" },
  { key: "SENTRY_DSN", value: "https://8f2c@o4501.ingest.sentry.io/77", secret: false, inherited: true, pending_apply: false, updated_at: ago(3), updated_by: "Alex Martin" },
  { key: "STRIPE_SECRET_KEY", value: null, secret: true, inherited: false, pending_apply: false, updated_at: ago(12), updated_by: "Yanis Benali" },
];

const [WEB, BILLING, , , MARKETING] = PROJECT_SUMMARIES;
const ALL = ["production", "preview", "staging"];
const g = (n: number, v: Omit<GlobalVariable, "id">): GlobalVariable => ({ id: `00000000-0000-4000-8000-0000000000${(0xe0 + n).toString(16)}`, ...v });

export const GLOBAL_VARIABLES: GlobalVariable[] = [
  g(1, { key: "SENTRY_DSN", value: "https://8f2c@o4501.ingest.sentry.io/77", secret: false, environments: ALL, all_projects: true, projects: [], updated_at: ago(3), updated_by: "Alex Martin" }),
  g(2, { key: "RESEND_API_KEY", value: null, secret: true, environments: ["production"], all_projects: false, projects: [WEB.id, BILLING.id, MARKETING.id], updated_at: ago(12), updated_by: "Lina Moreau" }),
  g(3, { key: "S3_BUCKET", value: "ciphera-assets-eu", secret: false, environments: ["production", "staging"], all_projects: false, projects: [WEB.id, MARKETING.id], updated_at: ago(30), updated_by: "Alex Martin" }),
  g(4, { key: "S3_SECRET_ACCESS_KEY", value: null, secret: true, environments: ["production", "staging"], all_projects: false, projects: [WEB.id, MARKETING.id], updated_at: ago(30), updated_by: "Alex Martin" }),
  g(5, { key: "LOG_LEVEL", value: "info", secret: false, environments: ["production"], all_projects: true, projects: [], updated_at: ago(60), updated_by: "Yanis Benali" }),
  g(6, { key: "LOG_LEVEL", value: "debug", secret: false, environments: ["preview", "staging"], all_projects: true, projects: [], updated_at: ago(60), updated_by: "Yanis Benali" }),
  g(7, { key: "STRIPE_WEBHOOK_SECRET", value: null, secret: true, environments: ["production"], all_projects: false, projects: [BILLING.id], updated_at: ago(6), updated_by: "Yanis Benali" }),
  g(8, { key: "OTEL_EXPORTER_OTLP_ENDPOINT", value: "http://otel.internal:4317", secret: false, environments: ALL, all_projects: true, projects: [], updated_at: ago(21), updated_by: "Alex Martin" }),
];

/** Export `.env` du serveur : globales publiques seulement. */
export function exportedGlobals(env?: string | null): string {
  return GLOBAL_VARIABLES.filter((v) => !v.secret && (!env || v.environments.includes(env)))
    .map((v) => `${v.key}=${v.value}\n`)
    .join("");
}
