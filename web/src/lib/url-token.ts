import { useState } from "react";

/**
 * Jeton lu dans l'URL d'un lien à usage unique (réinitialisation, invitation,
 * vérification d'adresse). Une fois lu, l'adresse est remplacée par `safePath`
 * (`history.replaceState`, sans entrée d'historique) : le jeton ne reste ni
 * dans la barre d'adresse, ni dans l'historique, ni dans un `Referer` futur.
 * Le jeton est gardé en mémoire pour la suite de la page.
 */
export function useUrlToken(token: string, safePath = "/login"): string {
  const [held] = useState(() => {
    stripUrlToken(safePath);
    return token;
  });
  return held;
}

/** Remplace l'adresse courante par `safePath` ; l'état d'historique du routeur est conservé. */
export function stripUrlToken(safePath: string): void {
  try {
    window.history.replaceState(window.history.state, "", safePath);
  } catch {
    // Contexte sans historique modifiable : le jeton reste dans l'URL, rien de plus à faire.
  }
}
