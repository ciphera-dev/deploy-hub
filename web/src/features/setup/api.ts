import { api } from "@/lib/api/client";
import { ApiError, unwrap } from "@/lib/api/errors";
import type { components } from "@/lib/api/schema";

export type SetupCheck = components["schemas"]["SetupCheck"];
export type SetupPayload = components["schemas"]["SetupComplete"];

/**
 * Refus propres a l'assistant (jeton invalide, jeton absent, assistant deja termine) :
 * ils sont rendus par l'ecran, pas par la politique d'erreur globale (ecran 500, redirection).
 */
export type Rejected = { rejected: 401 | 410 | 503 };

const isRejection = (status: number): status is Rejected["rejected"] => status === 401 || status === 410 || status === 503;

const header = (token: string) => ({ "X-Setup-Token": token });

export async function fetchStatus(): Promise<{ initialized: boolean }> {
  return unwrap(await api.GET("/api/setup/status"));
}

export async function fetchChecks(token: string): Promise<{ checks: SetupCheck[] } | Rejected> {
  const result = await api.POST("/api/setup/checks", { params: { header: header(token) } });
  if (isRejection(result.response.status)) return { rejected: result.response.status as Rejected["rejected"] };
  return unwrap(result);
}

export async function submitSetup(token: string, body: SetupPayload): Promise<{ created: true } | Rejected> {
  const result = await api.POST("/api/setup/complete", { params: { header: header(token) }, body });
  if (isRejection(result.response.status)) return { rejected: result.response.status as Rejected["rejected"] };
  // 422 et autres refus : ApiError (statut seul), affiche par l'ecran de finalisation.
  if (!result.response.ok) throw new ApiError(result.response.status, "/api/setup/complete");
  return { created: true };
}
