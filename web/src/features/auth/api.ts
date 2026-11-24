import { api } from "@/lib/api/client";
import { setCsrfToken } from "@/lib/api/csrf";
import { ApiError, unwrap } from "@/lib/api/errors";
import type { components } from "@/lib/api/schema";

import { assertionJson, isCancellation, requestOptions, webauthnSupported } from "./webauthn";

export type Me = components["schemas"]["Me"];
export type LoginPayload = components["schemas"]["LoginRequest"];

export const ME_QUERY_KEY = ["me"] as const;

/** Reponse de GET /api/me pour une session ouverte par le mot de passe, en attente du second facteur. */
export const MFA_PENDING = "mfa_pending" as const;

/** Resultat d'une connexion : session ouverte, ou refus a afficher par l'ecran. */
export type LoginResult =
  | { ok: true; mfaRequired: boolean }
  | { ok: false; reason: "invalid" | "unverified" | "rate_limited" | "unavailable"; retryAfter?: number };

/** Connexion : les refus sont rendus par l'ecran, pas par la politique d'erreur globale. */
export async function login(body: LoginPayload): Promise<LoginResult> {
  let result;
  try {
    result = await api.POST("/api/auth/login", { body });
  } catch {
    return { ok: false, reason: "unavailable" };
  }
  const { status } = result.response;
  if (result.data) {
    setCsrfToken(result.data.csrf_token);
    return { ok: true, mfaRequired: result.data.mfa_required };
  }
  if (status === 401 || status === 422) return { ok: false, reason: "invalid" };
  if (status === 403 && errorCode(result.error) === "email_not_verified") return { ok: false, reason: "unverified" };
  if (status === 429) {
    const retryAfter = Number(result.response.headers.get("Retry-After"));
    return { ok: false, reason: "rate_limited", retryAfter: Number.isFinite(retryAfter) && retryAfter > 0 ? retryAfter : undefined };
  }
  return { ok: false, reason: "unavailable" };
}

/** Code d'erreur stable du corps `{ error: { code } }`, s'il y en a un. */
function errorCode(error: unknown): string | undefined {
  const code = (error as { error?: { code?: unknown } } | undefined)?.error?.code;
  return typeof code === "string" ? code : undefined;
}

/**
 * Compte connecte ; `null` sans session valide (401) ; `MFA_PENDING` si la
 * session attend le second facteur. Retient le jeton CSRF.
 */
export async function fetchMe(): Promise<Me | null | typeof MFA_PENDING> {
  const result = await api.GET("/api/me");
  if (result.response.status === 401) {
    setCsrfToken(null);
    return errorCode(result.error) === "mfa_required" ? MFA_PENDING : null;
  }
  const me = unwrap(result);
  setCsrfToken(me.csrf_token);
  return me;
}

export async function logout(): Promise<void> {
  const result = await api.POST("/api/auth/logout");
  if (!result.response.ok) throw new ApiError(result.response.status, "/api/auth/logout");
  setCsrfToken(null);
}

/** Second facteur saisi a la connexion. */
export type SecondFactor = { kind: "totp"; code: string } | { kind: "recovery"; code: string } | { kind: "webauthn" };

export type SecondFactorResult =
  | { ok: true }
  | { ok: false; reason: "invalid" | "expired" | "rate_limited" | "unavailable" | "cancelled"; retryAfter?: number };

function retryAfterOf(response: Response): number | undefined {
  const retryAfter = Number(response.headers.get("Retry-After"));
  return Number.isFinite(retryAfter) && retryAfter > 0 ? retryAfter : undefined;
}

/** Passkeys utilisables depuis l'hote courant (faux si le panneau est joint par une adresse IP). */
export async function fetchWebauthnStatus(): Promise<boolean> {
  if (!webauthnSupported()) return false;
  try {
    const result = await api.GET("/api/auth/webauthn/status");
    return result.data?.available ?? false;
  } catch {
    return false;
  }
}

export type MfaMethods = components["schemas"]["MfaMethods"];

/** Seconds facteurs de la session en attente ; par defaut ceux d'avant les passkeys. */
export async function fetchMfaMethods(): Promise<MfaMethods> {
  const fallback = { totp: true, webauthn: false, recovery: true };
  try {
    const result = await api.GET("/api/auth/mfa/methods");
    const methods = result.data ?? fallback;
    return { ...methods, webauthn: methods.webauthn && webauthnSupported() };
  } catch {
    return fallback;
  }
}

/**
 * Ceremonie d'assertion complete : options, cle de l'utilisateur, verification.
 * `second_factor` : session en attente de MFA ; `login` : connexion sans mot de passe.
 */
export async function assertWithPasskey(mode: "login" | "second_factor", remember = false): Promise<SecondFactorResult> {
  let ceremonyId: string;
  let credential: PublicKeyCredential;
  try {
    const options = await api.POST("/api/auth/webauthn/assert/options", { body: { mode } });
    if (!options.data) {
      const { status } = options.response;
      if (status === 401) return { ok: false, reason: "expired" };
      if (status === 409 && mode === "second_factor" && errorCode(options.error) === "mfa_not_pending") return { ok: true };
      if (status === 429) return { ok: false, reason: "rate_limited", retryAfter: retryAfterOf(options.response) };
      return { ok: false, reason: "unavailable" };
    }
    ceremonyId = options.data.ceremony_id;
    const got = await navigator.credentials.get({ publicKey: requestOptions(options.data.options) });
    if (!got) return { ok: false, reason: "cancelled" };
    credential = got as PublicKeyCredential;
  } catch (error) {
    return { ok: false, reason: isCancellation(error) ? "cancelled" : "unavailable" };
  }
  let result;
  try {
    result = await api.POST("/api/auth/webauthn/assert/verify", { body: { ceremony_id: ceremonyId, credential: assertionJson(credential), remember } });
  } catch {
    return { ok: false, reason: "unavailable" };
  }
  if (result.data) {
    setCsrfToken(result.data.csrf_token);
    return { ok: true };
  }
  const { status } = result.response;
  if (status === 401) return { ok: false, reason: errorCode(result.error) === "webauthn_failed" ? "invalid" : "expired" };
  if (status === 409 && errorCode(result.error) === "mfa_not_pending") return { ok: true };
  if (status === 429) return { ok: false, reason: "rate_limited", retryAfter: retryAfterOf(result.response) };
  return { ok: false, reason: "unavailable" };
}

/** Verifie le second facteur d'une session en attente de MFA ; la session tourne (nouveau jeton CSRF). */
export async function verifySecondFactor(factor: SecondFactor): Promise<SecondFactorResult> {
  if (factor.kind === "webauthn") return assertWithPasskey("second_factor");
  const body = { code: factor.code };
  let result;
  try {
    result = factor.kind === "totp" ? await api.POST("/api/auth/mfa/verify", { body }) : await api.POST("/api/auth/recovery-code", { body });
  } catch {
    return { ok: false, reason: "unavailable" };
  }
  const { status } = result.response;
  if (result.data) {
    setCsrfToken(result.data.csrf_token);
    return { ok: true };
  }
  // Session deja complete (double envoi) : rien a verifier.
  if (status === 409) return { ok: true };
  if (status === 401) return { ok: false, reason: errorCode(result.error) === "invalid_code" ? "invalid" : "expired" };
  if (status === 422) return { ok: false, reason: "invalid" };
  if (status === 429) {
    const retryAfter = Number(result.response.headers.get("Retry-After"));
    return { ok: false, reason: "rate_limited", retryAfter: Number.isFinite(retryAfter) && retryAfter > 0 ? retryAfter : undefined };
  }
  return { ok: false, reason: "unavailable" };
}

export type InvitationInfo = components["schemas"]["InvitationInfo"];

/** Invitation lue par l'ecran d'acceptation, ou la raison de son refus. */
export type InvitationLookup = { ok: true; invitation: InvitationInfo } | { ok: false; reason: "invalid" | "expired" | "unavailable" };

export async function fetchInvitation(token: string): Promise<InvitationLookup> {
  let result;
  try {
    result = await api.GET("/api/auth/invitations/{token}", { params: { path: { token } } });
  } catch {
    return { ok: false, reason: "unavailable" };
  }
  if (result.data) return { ok: true, invitation: result.data };
  const { status } = result.response;
  if (status === 404) return { ok: false, reason: "invalid" };
  if (status === 410) return { ok: false, reason: "expired" };
  return { ok: false, reason: "unavailable" };
}

/** Creation d'un compte (invitation acceptee ou inscription) : refus a afficher par l'ecran. */
export type SignupResult =
  | { ok: true }
  | { ok: false; reason: "invalid" | "expired" | "taken" | "smtp_required" | "rejected" | "disabled" | "rate_limited" | "unavailable"; message?: string; retryAfter?: number };

/** Message FR du serveur (validation), s'il y en a un. */
function errorMessage(error: unknown): string | undefined {
  const message = (error as { error?: { message?: unknown } } | undefined)?.error?.message;
  return typeof message === "string" ? message : undefined;
}

function signupFailure(response: Response, error: unknown): SignupResult {
  const { status } = response;
  if (status === 404) return { ok: false, reason: "invalid" };
  if (status === 410) return { ok: false, reason: "expired" };
  if (status === 409) return { ok: false, reason: errorCode(error) === "smtp_required" ? "smtp_required" : "taken" };
  if (status === 403) return { ok: false, reason: "disabled" };
  if (status === 422) return { ok: false, reason: "rejected", message: errorMessage(error) };
  if (status === 429) return { ok: false, reason: "rate_limited", retryAfter: retryAfterOf(response) };
  return { ok: false, reason: "unavailable" };
}

/** Accepte une invitation : le compte est cree, sans session (la connexion suit). */
export async function acceptInvitation(token: string, body: components["schemas"]["AcceptInvitation"]): Promise<SignupResult> {
  let result;
  try {
    result = await api.POST("/api/auth/invitations/{token}/accept", { params: { path: { token } }, body });
  } catch {
    return { ok: false, reason: "unavailable" };
  }
  return result.response.ok ? { ok: true } : signupFailure(result.response, result.error);
}

/** Inscription publique ouverte ? Faux en cas de doute (invitation seulement). */
export async function fetchPublicSignup(): Promise<boolean> {
  try {
    const result = await api.GET("/api/auth/register");
    return result.data?.public_signup ?? false;
  } catch {
    return false;
  }
}

export async function register(body: components["schemas"]["RegisterRequest"]): Promise<SignupResult> {
  let result;
  try {
    result = await api.POST("/api/auth/register", { body });
  } catch {
    return { ok: false, reason: "unavailable" };
  }
  return result.response.ok ? { ok: true } : signupFailure(result.response, result.error);
}

/** Lien de verification d'adresse lu par l'ecran, ou la raison de son refus. */
export type EmailVerificationLookup =
  | { ok: true; purpose: "signup" | "change"; email: string }
  | { ok: false; reason: "invalid" | "expired" | "unavailable" };

export async function fetchEmailVerification(token: string): Promise<EmailVerificationLookup> {
  let result;
  try {
    result = await api.GET("/api/auth/email-verification/{token}", { params: { path: { token } } });
  } catch {
    return { ok: false, reason: "unavailable" };
  }
  if (result.data) return { ok: true, purpose: result.data.purpose === "change" ? "change" : "signup", email: result.data.email };
  const { status } = result.response;
  if (status === 404) return { ok: false, reason: "invalid" };
  if (status === 410) return { ok: false, reason: "expired" };
  return { ok: false, reason: "unavailable" };
}

export type EmailConfirmResult = { ok: true } | { ok: false; reason: "invalid" | "expired" | "taken" | "unavailable" };

/** Confirme l'adresse : active le compte (inscription) ou remplace l'adresse et ferme les autres sessions (changement). */
export async function confirmEmailVerification(token: string): Promise<EmailConfirmResult> {
  let result;
  try {
    result = await api.POST("/api/auth/email-verification/{token}/confirm", { params: { path: { token } } });
  } catch {
    return { ok: false, reason: "unavailable" };
  }
  const { status } = result.response;
  if (result.response.ok) return { ok: true };
  if (status === 404) return { ok: false, reason: "invalid" };
  if (status === 410) return { ok: false, reason: "expired" };
  if (status === 409) return { ok: false, reason: "taken" };
  return { ok: false, reason: "unavailable" };
}

/** Envoi d'un lien par e-mail possible (SMTP et origine publique configures) ? Faux en cas de doute. */
export async function fetchPasswordResetStatus(): Promise<boolean> {
  try {
    const result = await api.GET("/api/auth/password/forgot");
    return result.data?.email_available ?? false;
  } catch {
    return false;
  }
}

export type ForgotResult = { ok: true } | { ok: false; reason: "rate_limited" | "unavailable"; retryAfter?: number };

/** Demande un lien de reinitialisation : meme reponse que le compte existe ou non. */
export async function requestPasswordReset(email: string): Promise<ForgotResult> {
  let result;
  try {
    result = await api.POST("/api/auth/password/forgot", { body: { email } });
  } catch {
    return { ok: false, reason: "unavailable" };
  }
  const { status } = result.response;
  if (status === 202) return { ok: true };
  if (status === 429) return { ok: false, reason: "rate_limited", retryAfter: retryAfterOf(result.response) };
  return { ok: false, reason: "unavailable" };
}

/** Lien de reinitialisation lu par l'ecran, ou la raison de son refus. */
export type ResetLookup = { ok: true } | { ok: false; reason: "invalid" | "expired" | "unavailable" };

export async function fetchResetToken(token: string): Promise<ResetLookup> {
  let result;
  try {
    result = await api.GET("/api/auth/password/reset/{token}", { params: { path: { token } } });
  } catch {
    return { ok: false, reason: "unavailable" };
  }
  if (result.data) return { ok: true };
  const { status } = result.response;
  if (status === 404) return { ok: false, reason: "invalid" };
  if (status === 410) return { ok: false, reason: "expired" };
  return { ok: false, reason: "unavailable" };
}

export type ResetResult =
  | { ok: true }
  | { ok: false; reason: "invalid" | "expired" | "rejected" | "rate_limited" | "unavailable"; message?: string; retryAfter?: number };

/** Nouveau mot de passe : les sessions du compte sont fermees, aucune n'est ouverte. */
export async function resetPassword(token: string, password: string): Promise<ResetResult> {
  let result;
  try {
    result = await api.POST("/api/auth/password/reset", { body: { token, password } });
  } catch {
    return { ok: false, reason: "unavailable" };
  }
  const { status } = result.response;
  if (result.response.ok) return { ok: true };
  if (status === 404) return { ok: false, reason: "invalid" };
  if (status === 410) return { ok: false, reason: "expired" };
  if (status === 422) return { ok: false, reason: "rejected", message: errorMessage(result.error) };
  if (status === 429) return { ok: false, reason: "rate_limited", retryAfter: retryAfterOf(result.response) };
  return { ok: false, reason: "unavailable" };
}
