import createClient from "openapi-fetch";

import { csrfMiddleware } from "./csrf";
import type { paths } from "./schema";

// Les chemins OpenAPI portent deja le prefixe /api : la base est la racine de
// l'origine courante (une URL absolue, car Request refuse les URL relatives hors navigateur).
export const api = createClient<paths>({
  baseUrl: globalThis.location?.origin ?? "",
  credentials: "same-origin",
  // Resolu a chaque appel (et non a l'import) : les tests peuvent installer un fetch simule apres coup.
  fetch: (request) => globalThis.fetch(request),
});
api.use(csrfMiddleware);
