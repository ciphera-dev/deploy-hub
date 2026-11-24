/** Erreur HTTP de l'API : seul le statut est conserve (le corps peut contenir des details internes). */
export class ApiError extends Error {
  readonly status: number;
  /** Chemin de la requete (sans requete ni fragment), ex. /api/auth/login */
  readonly path: string;

  constructor(status: number, path = "") {
    super(`HTTP ${status}`);
    this.name = "ApiError";
    this.status = status;
    this.path = path;
  }
}

function pathOf(url: string): string {
  try {
    return new URL(url).pathname;
  } catch {
    return "";
  }
}

interface FetchResult<T> {
  data?: T;
  error?: unknown;
  response: Response;
}

/** Transforme le resultat d'openapi-fetch en valeur ou en ApiError, pour react-query. */
export function unwrap<T>(result: FetchResult<T>): T {
  if (!result.response.ok) throw new ApiError(result.response.status, pathOf(result.response.url));
  return result.data as T;
}
