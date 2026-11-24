import type { Middleware } from "openapi-fetch";

// Jeton CSRF de la session, garde en memoire seulement (jamais dans localStorage
// ni sessionStorage) : il est relu par POST /api/auth/login et GET /api/me.
let token: string | null = null;

export function setCsrfToken(value: string | null): void {
  token = value;
}

export function getCsrfToken(): string | null {
  return token;
}

const SAFE_METHODS = new Set(["GET", "HEAD", "OPTIONS"]);

/** Ajoute X-CSRF-Token a toute requete d'ecriture, si une session est connue. */
export const csrfMiddleware: Middleware = {
  onRequest({ request }) {
    if (token && !SAFE_METHODS.has(request.method.toUpperCase())) request.headers.set("X-CSRF-Token", token);
    return request;
  },
};
