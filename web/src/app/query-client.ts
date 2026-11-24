import { MutationCache, QueryCache, QueryClient } from "@tanstack/react-query";

import { ApiError } from "@/lib/api/errors";

/** Erreurs qui remplacent l'ecran courant (403, API injoignable, 5xx). */
export type FatalKind = "forbidden" | "offline" | "server";

export type ErrorClass = "unauthorized" | FatalKind | null;

export interface ErrorPolicyHandlers {
  /** 401 : la session n'est plus valide. */
  onUnauthorized: () => void;
  /** Requete (lecture) en 403, reseau ou 5xx : ecran d'erreur plein page. */
  onFatal: (kind: FatalKind) => void;
  /** Mutation en 403, reseau ou 5xx : message ephemere, la page (et ses formulaires) reste montee. */
  onMutationError: (kind: FatalKind) => void;
}

const AUTH_FAILURE_PATHS = ["/api/auth/", "/api/setup/"];

/** fetch rejette avec un TypeError quand l'API est injoignable (reseau coupe, DNS, connexion refusee). */
export function classifyError(error: unknown): ErrorClass {
  if (error instanceof ApiError) {
    // /api/auth/* : un 401 y signifie « identifiants refuses », pas « session expiree » ; l'ecran de connexion le traite.
    // /api/setup/* : un 401 y signifie « jeton d'installation invalide » (aucune session n'existe encore).
    if (error.status === 401) return AUTH_FAILURE_PATHS.some((prefix) => error.path.startsWith(prefix)) ? null : "unauthorized";
    if (error.status === 403) return "forbidden";
    if (error.status >= 500) return "server";
    return null;
  }
  if (error instanceof TypeError) return "offline";
  return null;
}

export function createQueryClient(handlers: ErrorPolicyHandlers): QueryClient {
  const handle = (error: unknown, onKind: (kind: FatalKind) => void) => {
    const kind = classifyError(error);
    if (kind === "unauthorized") handlers.onUnauthorized();
    else if (kind) onKind(kind);
  };
  return new QueryClient({
    queryCache: new QueryCache({ onError: (e) => handle(e, handlers.onFatal) }),
    mutationCache: new MutationCache({ onError: (e) => handle(e, handlers.onMutationError) }),
    defaultOptions: {
      queries: {
        // Pas de nouvelle tentative sur une erreur client (4xx) : elle ne se resoudra pas seule.
        retry: (count, error) => !(error instanceof ApiError && error.status < 500) && count < 1,
        refetchOnWindowFocus: false,
      },
    },
  });
}
