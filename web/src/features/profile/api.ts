import { api } from "@/lib/api/client";
import { unwrap } from "@/lib/api/errors";
import type { components } from "@/lib/api/schema";

export type ProfileUpdate = components["schemas"]["UpdateMe"];
export type SessionInfo = components["schemas"]["SessionInfo"];
export type NotificationPrefs = components["schemas"]["NotificationPrefs"];
export type NotificationPrefsView = components["schemas"]["NotificationPrefsView"];

export const SESSIONS_QUERY_KEY = ["me", "sessions"] as const;
export const PREFS_QUERY_KEY = ["me", "notification-prefs"] as const;

/** Taille maximale d'un avatar envoye (verifiee aussi par le serveur). */
export const AVATAR_MAX_BYTES = 1024 * 1024;
/** Formats acceptes (le serveur lit le format dans les octets). */
export const AVATAR_TYPES = ["image/png", "image/jpeg", "image/webp"] as const;

export type Outcome = { ok: true } | { ok: false; message: string };

function serverMessage(error: unknown): string | undefined {
  const message = (error as { error?: { message?: unknown } } | undefined)?.error?.message;
  return typeof message === "string" ? message : undefined;
}

const UNREACHABLE: Outcome = { ok: false, message: "Serveur injoignable. Réessayez." };
const RATE_LIMITED = "Trop de tentatives. Réessayez dans quelques minutes.";

/** PATCH /api/me : nom, e-mail (mot de passe actuel exige), langue, fuseau horaire. */
export async function updateProfile(body: ProfileUpdate): Promise<Outcome> {
  let result;
  try {
    result = await api.PATCH("/api/me", { body });
  } catch {
    return UNREACHABLE;
  }
  if (result.data) return { ok: true };
  const { status } = result.response;
  if (status === 401) return { ok: false, message: "Mot de passe actuel incorrect." };
  if (status === 429) return { ok: false, message: RATE_LIMITED };
  if (status === 409 || status === 422) return { ok: false, message: serverMessage(result.error) ?? "Valeur refusée." };
  return { ok: false, message: "Modification impossible pour le moment. Réessayez." };
}

/** PUT /api/me/avatar : fichier brut, recadre et re-encode par le serveur. */
export async function uploadAvatar(file: File): Promise<Outcome> {
  if (!(AVATAR_TYPES as readonly string[]).includes(file.type)) return { ok: false, message: "Formats acceptés : PNG, JPEG ou WebP." };
  if (file.size > AVATAR_MAX_BYTES) return { ok: false, message: "L'image dépasse 1 Mo." };
  let result;
  try {
    result = await api.PUT("/api/me/avatar", {
      // Corps binaire tel quel (le schema decrit des octets).
      body: file as unknown as number[],
      bodySerializer: (b: unknown) => b as Blob,
      headers: { "Content-Type": file.type },
    });
  } catch {
    return UNREACHABLE;
  }
  if (result.data) return { ok: true };
  const { status } = result.response;
  if (status === 413) return { ok: false, message: "L'image dépasse 1 Mo." };
  if (status === 415 || status === 422) return { ok: false, message: serverMessage(result.error) ?? "Image refusée." };
  return { ok: false, message: "Envoi impossible pour le moment. Réessayez." };
}

/** DELETE /api/me/avatar : retour aux initiales. */
export async function deleteAvatar(): Promise<Outcome> {
  try {
    const result = await api.DELETE("/api/me/avatar");
    return result.response.ok ? { ok: true } : { ok: false, message: "Suppression impossible pour le moment. Réessayez." };
  } catch {
    return UNREACHABLE;
  }
}

/** POST /api/me/password : la session courante reste ouverte, les autres sont fermees. */
export async function changePassword(currentPassword: string, newPassword: string): Promise<Outcome> {
  let result;
  try {
    result = await api.POST("/api/me/password", { body: { current_password: currentPassword, new_password: newPassword } });
  } catch {
    return UNREACHABLE;
  }
  const { status } = result.response;
  if (status === 204) return { ok: true };
  if (status === 401) return { ok: false, message: "Mot de passe actuel incorrect." };
  if (status === 422) return { ok: false, message: serverMessage(result.error) ?? "Mot de passe refusé." };
  if (status === 429) return { ok: false, message: RATE_LIMITED };
  return { ok: false, message: "Modification impossible pour le moment. Réessayez." };
}

/** DELETE /api/me/mfa/totp : mot de passe redemande. */
export async function disableTotp(password: string): Promise<Outcome> {
  let result;
  try {
    result = await api.DELETE("/api/me/mfa/totp", { body: { password } });
  } catch {
    return UNREACHABLE;
  }
  const { status } = result.response;
  if (status === 204) return { ok: true };
  if (status === 401) return { ok: false, message: "Mot de passe incorrect." };
  if (status === 409) return { ok: false, message: "Votre organisation impose la vérification en deux étapes : gardez au moins une méthode." };
  if (status === 429) return { ok: false, message: RATE_LIMITED };
  return { ok: false, message: "Désactivation impossible pour le moment. Réessayez." };
}

export async function listSessions(): Promise<SessionInfo[]> {
  return unwrap(await api.GET("/api/me/sessions")).sessions;
}

/** Ferme une autre session du compte ; 404 (deja fermee) compte comme un succes. */
export async function revokeSession(id: string): Promise<boolean> {
  try {
    const result = await api.DELETE("/api/me/sessions/{id}", { params: { path: { id } } });
    return result.response.status === 204 || result.response.status === 404;
  } catch {
    return false;
  }
}

export async function fetchPrefs(): Promise<NotificationPrefsView> {
  return unwrap(await api.GET("/api/me/notification-prefs"));
}

export async function savePrefs(body: NotificationPrefs): Promise<NotificationPrefsView> {
  return unwrap(await api.PUT("/api/me/notification-prefs", { body }));
}
