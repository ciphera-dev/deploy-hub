import type { QueryClient } from "@tanstack/react-query";

import { ME_QUERY_KEY, type Me } from "./api";

// Confirmation renforcée d'une action sensible (deploy hook, suppression de
// l'organisation) : mot de passe, plus le code du second facteur (application
// d'authentification ou code de secours) si le compte en a un.

/** Le compte connu en cache a un second facteur : le champ « code » est affiché d'emblée. */
export function hasSecondFactor(queryClient: QueryClient): boolean {
  const me = queryClient.getQueryData<Me>(ME_QUERY_KEY);
  return !!me && (me.totp_enabled || me.webauthn_enabled);
}

/** Code d'erreur de l'API, s'il y en a un. */
export function errorCode(error: unknown): string | undefined {
  const code = (error as { error?: { code?: unknown } } | undefined)?.error?.code;
  return typeof code === "string" ? code : undefined;
}

/** Message d'un refus 401 de confirmation renforcée. */
export function stepUpRefusal(code: string | undefined): string {
  if (code === "second_factor_required") return "Saisissez aussi le code de votre application d'authentification ou un code de secours.";
  if (code === "invalid_code") return "Code de vérification incorrect.";
  return "Mot de passe incorrect.";
}

/** Le code du second facteur est (désormais) demandé. */
export function asksForCode(code: string | undefined): boolean {
  return code === "second_factor_required" || code === "invalid_code";
}
