import { api } from "@/lib/api/client";
import { unwrap } from "@/lib/api/errors";
import type { components } from "@/lib/api/schema";

export type Overview = components["schemas"]["Overview"];
export type OverviewPeriod = components["schemas"]["OverviewPeriod"];
export type OverviewIssue = components["schemas"]["OverviewIssue"];
export type KpiSeries = components["schemas"]["KpiSeries"];
export type ServerLoad = components["schemas"]["ServerLoad"];

export const overviewQueryKey = (period: OverviewPeriod) => ["overview", period] as const;

/** GET /api/overview?period= (viewer et plus). Annulée quand l'écran est quitté avant la réponse. */
export async function fetchOverview(period: OverviewPeriod, signal?: AbortSignal): Promise<Overview> {
  return unwrap(await api.GET("/api/overview", { params: { query: { period } }, signal }));
}
