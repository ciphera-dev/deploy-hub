import type { WireLogLine } from "@/features/logs/useLogStream";

// Lignes de prototypes/dashboard/ProjectLogs.jsx (SAMPLES), tirées dans un
// ordre fixe au lieu de Math.random. Données factices pour les tests,
// jamais des données de production.

export const LOG_SAMPLES: Record<string, [string, string][]> = {
  web: [
    ["info", "GET / 200 12ms"],
    ["info", "GET /api/projects 200 34ms"],
    ["info", "GET /_next/static/chunks/app.js 304 2ms"],
    ["debug", "cache hit key=dashboard:alex"],
    ["warn", "Slow query 812ms: SELECT * FROM deployments ORDER BY created_at"],
    ["info", "POST /api/deploy 202 48ms"],
    ["error", "Unhandled rejection: fetch failed (ECONNRESET) upstream=billing-api"],
    ["info", "GET /health 200 1ms"],
  ],
  worker: [
    ["info", "job deploy#4821 picked (SKIP LOCKED)"],
    ["info", "job deploy#4821 step=build status=running"],
    ["debug", "buildkit session 7f3a opened"],
    ["info", "job email#9912 sent to lina@ciphera.dev"],
    ["warn", "retrying job webhook#331 (attempt 2/5)"],
    ["info", "job deploy#4821 done in 96s"],
  ],
  postgres: [
    ["info", "checkpoint starting: time"],
    ["info", "checkpoint complete: wrote 412 buffers (2.5%)"],
    ["warn", "duration: 812.4 ms statement: SELECT * FROM deployments"],
    ["info", "connection authorized: user=ciphera database=web"],
    ["debug", "autovacuum: processing table public.events"],
  ],
};

/** Les lignes d'un conteneur (échantillon `sample`), espacées de 2,3 s et finissant à `end`. */
export function logLines(container: string, sample: keyof typeof LOG_SAMPLES, end = Date.now()): WireLogLine[] {
  const s = LOG_SAMPLES[sample]!;
  return s.map(([level, message], i) => ({ ts: new Date(end - (s.length - 1 - i) * 2300).toISOString(), level, container, message }));
}
