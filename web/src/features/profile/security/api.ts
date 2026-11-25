import { api } from "@/lib/api/client";
import { setCsrfToken } from "@/lib/api/csrf";
import { unwrap } from "@/lib/api/errors";
import { notifySessionExpired } from "@/lib/api/session";
import type { components } from "@/lib/api/schema";
import { creationOptions, isCancellation, registrationJson } from "@/features/auth/webauthn";

export type TotpStart = components["schemas"]["TotpStart"];

export type FactorFailure = "password" | "rate_limited" | "unavailable";
export type StartResult = { ok: true; setup: TotpStart } | { ok: false; reason: FactorFailure };

/** Seul un 401 `invalid_credentials` est un mot de passe faux ; les autres 401 (session expiree) restent generiques. */
function failure(status: number, error?: unknown): FactorFailure {
  const code = (error as { error?: { code?: unknown } } | undefined)?.error?.code;
  // 401 sans `invalid_credentials` : la session a expire, on passe par la page de reconnexion.
  if (status === 401 && code !== "invalid_credentials") notifySessionExpired();
  return status === 401 && code === "invalid_credentials" ? "password" : status === 429 ? "rate_limited" : "unavailable";
}

/**
 * Nouvelle graine TOTP (remplace un enrolement non confirme). Le mot de passe
 * actuel est toujours redemande, enrolement impose compris.
 */
export async function startTotp(password: string): Promise<StartResult> {
  let result;
  try {
    result = await api.POST("/api/me/mfa/totp/start", { body: { current_password: password } });
  } catch {
    return { ok: false, reason: "unavailable" };
  }
  if (result.data) return { ok: true, setup: result.data };
  return { ok: false, reason: failure(result.response.status, result.error) };
}

export type ConfirmResult = { ok: true; recoveryCodes: string[] } | { ok: false; reason: "invalid" | "unavailable" };

/** Confirme la TOTP avec un code de l'application ; la session tourne (nouveau jeton CSRF). */
export async function confirmTotp(code: string): Promise<ConfirmResult> {
  let result;
  try {
    result = await api.POST("/api/me/mfa/totp/confirm", { body: { code } });
  } catch {
    return { ok: false, reason: "unavailable" };
  }
  if (result.data) {
    setCsrfToken(result.data.csrf_token);
    return { ok: true, recoveryCodes: result.data.recovery_codes };
  }
  return { ok: false, reason: result.response.status === 422 ? "invalid" : "unavailable" };
}

export type Passkey = components["schemas"]["WebauthnCredential"];

export const PASSKEYS_QUERY_KEY = ["me", "webauthn"] as const;

/** Cles de securite du compte connecte. */
export async function listPasskeys(): Promise<Passkey[]> {
  return unwrap(await api.GET("/api/me/webauthn")).credentials;
}

export type RegisterResult = { ok: true; recoveryCodes: string[] } | { ok: false; reason: "cancelled" | "exists" | "invalid" | "password" | "rate_limited" | "unavailable" };

/**
 * Enregistre une cle : options du serveur, creation par le navigateur,
 * verification ; la session tourne. `recoveryCodes` : codes de secours emis
 * quand le compte n'en avait plus (premiere cle sans application), vide sinon.
 */
export async function registerPasskey(name: string, password: string): Promise<RegisterResult> {
  try {
    const options = await api.POST("/api/me/webauthn/register/options", { body: { current_password: password } });
    if (!options.data) return { ok: false, reason: failure(options.response.status, options.error) };
    const created = await navigator.credentials.create({ publicKey: creationOptions(options.data.options) });
    if (!created) return { ok: false, reason: "cancelled" };
    const result = await api.POST("/api/me/webauthn/register/verify", {
      body: { ceremony_id: options.data.ceremony_id, name, credential: registrationJson(created as PublicKeyCredential) },
    });
    if (result.data) {
      setCsrfToken(result.data.csrf_token);
      return { ok: true, recoveryCodes: result.data.recovery_codes };
    }
    const { status } = result.response;
    return { ok: false, reason: status === 409 ? "exists" : status === 422 ? "invalid" : "unavailable" };
  } catch (error) {
    // InvalidStateError : la cle est deja enregistree (excludeCredentials).
    if (error instanceof DOMException && error.name === "InvalidStateError") return { ok: false, reason: "exists" };
    return { ok: false, reason: isCancellation(error) ? "cancelled" : "unavailable" };
  }
}

export type DeleteResult = { ok: true } | { ok: false; reason: "password" | "required_by_org" | "rate_limited" | "session_expired" | "unavailable" };

/** Supprime une cle du compte (mot de passe redemande). */
export async function deletePasskey(id: string, password: string): Promise<DeleteResult> {
  let result;
  try {
    result = await api.DELETE("/api/me/webauthn/{id}", { params: { path: { id } }, body: { password } });
  } catch {
    return { ok: false, reason: "unavailable" };
  }
  const { status } = result.response;
  // 404 : deja supprimee (autre onglet) ; la liste sera relue.
  if (status === 204 || status === 404) return { ok: true };
  if (status === 401) return { ok: false, reason: failure(status, result.error) === "password" ? "password" : "session_expired" };
  if (status === 409) return { ok: false, reason: "required_by_org" };
  if (status === 429) return { ok: false, reason: "rate_limited" };
  return { ok: false, reason: "unavailable" };
}

/** Graine en base32, lue dans l'URI otpauth, groupee par 4 pour la saisie manuelle. */
export function manualKey(otpauthUri: string): string {
  const query = otpauthUri.split("?")[1] ?? "";
  const secret = new URLSearchParams(query).get("secret") ?? "";
  return secret.match(/.{1,4}/g)?.join(" ") ?? "";
}
